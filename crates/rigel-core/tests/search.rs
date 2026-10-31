//! The palette's read model: one query over sessions, turns and patches, capped and ordered by
//! the current session first.

use rigel_core::{SessionService, search::workspace};
use tempfile::TempDir;

async fn seed(session: &rigel_core::Session, service: &SessionService, role: &str) -> String {
    let agent = service
        .register_agent(&session.id, "native", Some(role))
        .await
        .unwrap();
    let turn = service
        .append_turn(&agent.id, Some("Fix the parser crash"))
        .await
        .unwrap();
    service
        .append_item(
            &turn.id,
            "message",
            "completed",
            &serde_json::json!({ "text": "the parser crashes on a 40 MB pdf" }),
            &serde_json::json!({ "type": "message" }),
        )
        .await
        .unwrap();
    agent.id
}

#[tokio::test]
async fn searches_sessions_turns_items_and_patches() {
    let tmp = TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("PDF importer"), None).await.unwrap();
    let agent_id = seed(&session, &service, "implementer").await;

    sqlx::query(
        "INSERT INTO patches
           (id, session_id, agent_id, path, base_hash, result_hash, diff, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind("patch-1")
    .bind(&session.id)
    .bind(&agent_id)
    .bind("src/pdf.rs")
    .bind("base")
    .bind("result")
    .bind("@@ -1 +1 @@\n-old\n+new")
    .bind("applied")
    .bind(1_i64)
    .execute(service.pool())
    .await
    .unwrap();

    // A title match.
    let by_title = workspace(service.pool(), "importer", None, 10)
        .await
        .unwrap();
    assert_eq!(by_title.sessions.len(), 1);
    assert_eq!(by_title.sessions[0].id, session.id);
    assert!(
        by_title.turns.is_empty(),
        "the turn prompt does not say importer"
    );

    // A prompt match.
    let by_prompt = workspace(service.pool(), "parser crash", None, 10)
        .await
        .unwrap();
    assert_eq!(by_prompt.turns.len(), 1);
    assert_eq!(by_prompt.turns[0].session_id, session.id);
    assert!(!by_prompt.turns[0].from_item);

    // An item-body match, which must still point at the turn.
    let by_item = workspace(service.pool(), "40 MB", None, 10).await.unwrap();
    assert_eq!(by_item.turns.len(), 1);
    assert!(by_item.turns[0].from_item);
    assert!(by_item.turns[0].snippet.contains("40 MB"));

    // A patch path.
    let by_patch = workspace(service.pool(), "src/pdf", None, 10)
        .await
        .unwrap();
    assert_eq!(by_patch.patches.len(), 1);
    assert_eq!(by_patch.patches[0].path, "src/pdf.rs");
}

#[tokio::test]
async fn an_empty_query_returns_recent_rows_and_the_current_session_sorts_first() {
    let tmp = TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let older = service.start(Some("older"), None).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let newer = service.start(Some("newer"), None).await.unwrap();

    let recent = workspace(service.pool(), "", None, 1).await.unwrap();
    assert_eq!(recent.sessions.len(), 1);
    assert_eq!(recent.sessions[0].id, newer.id);
    assert!(recent.turns.is_empty(), "no query means no turn list");

    let current = workspace(service.pool(), "", Some(&older.id), 5)
        .await
        .unwrap();
    assert_eq!(current.sessions[0].id, older.id);
}

#[tokio::test]
async fn a_percent_is_text_not_a_wildcard() {
    let tmp = TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let percent = service.start(Some("100% done"), None).await.unwrap();
    service.start(Some("plain"), None).await.unwrap();

    let hits = workspace(service.pool(), "%", None, 10).await.unwrap();
    assert_eq!(hits.sessions.len(), 1);
    assert_eq!(hits.sessions[0].id, percent.id);
}

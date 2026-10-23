//! The session log is the product's spine: if these break, replay and resume are fiction.

use rigel_core::{SessionError, SessionService, SessionStatus};
use tempfile::TempDir;

#[tokio::test]
async fn a_session_survives_a_reopen_with_its_original_timestamp() {
    let tmp = TempDir::new().unwrap();

    let started = {
        let service = SessionService::open(tmp.path()).await.unwrap();
        service.start(Some("first"), Some(500)).await.unwrap()
    };
    assert_eq!(started.status, SessionStatus::Active);
    assert!(started.created_at > 0);

    // A second service stands in for a restarted app.
    let reopened = SessionService::open(tmp.path()).await.unwrap();
    let listed = reopened.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, started.id);
    assert_eq!(listed[0].created_at, started.created_at);
    assert_eq!(listed[0].title.as_deref(), Some("first"));
    assert_eq!(listed[0].budget_cents, Some(500));
}

#[tokio::test]
async fn sessions_list_newest_first_and_end_marks_them_ended() {
    let tmp = TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();

    let first = service.start(Some("a"), None).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let second = service.start(Some("b"), None).await.unwrap();

    let listed = service.list().await.unwrap();
    assert_eq!(listed[0].id, second.id);
    assert_eq!(listed[1].id, first.id);

    let ended = service.end(&first.id).await.unwrap();
    assert_eq!(ended.status, SessionStatus::Ended);
    assert!(ended.ended_at.is_some());

    assert!(matches!(
        service.end("missing").await,
        Err(SessionError::SessionNotFound(_))
    ));
}

#[tokio::test]
async fn turns_are_sequenced_per_agent_and_close_exactly_once() {
    let tmp = TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(None, None).await.unwrap();

    let codex = service
        .register_agent(&session.id, "codex", Some("implementer"))
        .await
        .unwrap();
    let claude = service
        .register_agent(&session.id, "claude", Some("reviewer"))
        .await
        .unwrap();

    let t1 = service.append_turn(&codex.id, Some("first")).await.unwrap();
    let t2 = service
        .append_turn(&codex.id, Some("second"))
        .await
        .unwrap();
    let other = service
        .append_turn(&claude.id, Some("review"))
        .await
        .unwrap();

    assert_eq!(t1.seq, 1);
    assert_eq!(t2.seq, 2);
    assert_eq!(other.seq, 1, "sequences are per agent, not global");

    let finished = service.finish_turn(&t1.id, "completed").await.unwrap();
    assert_eq!(finished.status, "completed");
    assert!(finished.completed_at.is_some());

    assert!(matches!(
        service.finish_turn(&t2.id, "done-ish").await,
        Err(SessionError::InvalidTurnStatus(_))
    ));
    assert!(matches!(
        service.append_turn("no-such-agent", None).await,
        Err(SessionError::AgentNotFound(_))
    ));
}

#[tokio::test]
async fn items_keep_order_and_the_raw_payload_untouched() {
    let tmp = TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(None, None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "codex", None)
        .await
        .unwrap();
    let turn = service.append_turn(&agent.id, Some("go")).await.unwrap();

    let body = serde_json::json!({ "text": "hello", "unmapped": false });
    let raw = serde_json::json!({ "type": "item.completed", "item": { "type": "agent_message" } });

    service
        .append_item(&turn.id, "message", "completed", &body, &raw)
        .await
        .unwrap();
    service
        .append_item(
            &turn.id,
            "command",
            "completed",
            &serde_json::json!({ "command": "cargo test" }),
            &serde_json::json!({ "type": "item.completed" }),
        )
        .await
        .unwrap();

    let items = service.items(&turn.id).await.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].seq, 1);
    assert_eq!(items[1].seq, 2);
    assert_eq!(items[0].body, body, "body round-trips through SQLite");
    assert_eq!(items[0].kind, "message");
}

#[tokio::test]
async fn checkpoints_record_the_tree_hash_for_resume() {
    let tmp = TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(None, None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", None)
        .await
        .unwrap();
    let turn = service.append_turn(&agent.id, None).await.unwrap();

    let checkpoint = service
        .checkpoint(
            &session.id,
            Some(&turn.id),
            "9c1f0d",
            &serde_json::json!({ "patches": [] }),
        )
        .await
        .unwrap();

    assert_eq!(checkpoint.tree_hash, "9c1f0d");
    assert_eq!(checkpoint.turn_id.as_deref(), Some(turn.id.as_str()));

    let listed = service.checkpoints(&session.id).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, checkpoint.id);
    let journal = service
        .checkpoint_journal(&checkpoint.id)
        .await
        .unwrap()
        .expect("journal round-trips");
    assert_eq!(journal["patches"], serde_json::json!([]));

    assert!(matches!(
        service
            .checkpoint("missing", None, "x", &serde_json::json!({}))
            .await,
        Err(SessionError::SessionNotFound(_))
    ));
}

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

/// D148: the window's list is an edit over rows that already existed. Renaming clears back to
/// the automatic title, pinning reorders without a second sort key, and archiving hides a row
/// from the default list without ending it or making it unreachable.
#[tokio::test]
async fn a_session_can_be_renamed_pinned_and_archived() {
    let tmp = TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();

    let first = service.start(Some("first"), None).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let second = service.start(Some("second"), None).await.unwrap();

    let renamed = service
        .rename(&second.id, Some("  release check  "))
        .await
        .unwrap();
    assert_eq!(renamed.title.as_deref(), Some("release check"));

    // One line and 120 characters: a row cannot grow into the pane it is drawn in.
    let multiline = format!("release check\n{}", "x".repeat(200));
    let clipped = service
        .rename(&second.id, Some(multiline.as_str()))
        .await
        .unwrap();
    assert_eq!(clipped.title.as_deref(), Some("release check"));
    let long = "y".repeat(200);
    let clipped = service
        .rename(&second.id, Some(long.as_str()))
        .await
        .unwrap();
    assert_eq!(clipped.title.unwrap().chars().count(), 120);

    let cleared = service.rename(&second.id, Some("   ")).await.unwrap();
    assert_eq!(cleared.title, None, "whitespace clears the name");

    let pinned = service.set_pinned(&first.id, true).await.unwrap();
    assert!(pinned.pinned_at.is_some());
    let listed = service.list().await.unwrap();
    assert_eq!(listed[0].id, first.id, "a pinned session sorts first");
    assert_eq!(listed[1].id, second.id);

    // Both edits are database rows, so a restarted window reads the same list.
    let reopened = SessionService::open(tmp.path()).await.unwrap();
    assert_eq!(
        reopened.get(&first.id).await.unwrap().pinned_at,
        pinned.pinned_at
    );

    service.set_pinned(&first.id, false).await.unwrap();
    let listed = service.list().await.unwrap();
    assert_eq!(listed[0].id, second.id, "unpinned returns to newest first");

    let archived = service.set_archived(&second.id, true).await.unwrap();
    assert!(archived.archived_at.is_some());
    assert_eq!(
        service.list().await.unwrap().len(),
        2,
        "list keeps archived rows"
    );
    assert_eq!(
        service.get(&second.id).await.unwrap().status,
        SessionStatus::Active,
        "archiving is not ending"
    );

    assert!(matches!(
        service.rename("missing", Some("x")).await,
        Err(SessionError::SessionNotFound(_))
    ));
    assert!(matches!(
        service.set_pinned("missing", true).await,
        Err(SessionError::SessionNotFound(_))
    ));
    assert!(matches!(
        service.set_archived("missing", true).await,
        Err(SessionError::SessionNotFound(_))
    ));
}

/// D150: a star is a toggle over one target per session, and the label is a single line the
/// inspector can draw without joining the item it names.
#[tokio::test]
async fn a_bookmark_toggles_and_stays_per_session() {
    let tmp = TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("bookmarks"), None).await.unwrap();

    let marked = service
        .toggle_bookmark(
            &session.id,
            "item",
            "item-1",
            "turn 1 · read_file\nsecond line",
        )
        .await
        .unwrap()
        .expect("the first toggle is on");
    assert_eq!(marked.kind, "item");
    assert_eq!(marked.label, "turn 1 · read_file", "one line only");
    assert_eq!(service.bookmarks(&session.id).await.unwrap().len(), 1);

    assert!(
        service
            .toggle_bookmark(&session.id, "item", "item-1", "again")
            .await
            .unwrap()
            .is_none(),
        "starring the same target clears it"
    );
    assert!(service.bookmarks(&session.id).await.unwrap().is_empty());

    // The same id in another kind is another target: a hunk and an item share no namespace.
    service
        .toggle_bookmark(&session.id, "hunk", "item-1", "patch · @@ -1,2 +1,2 @@")
        .await
        .unwrap();
    assert_eq!(service.bookmarks(&session.id).await.unwrap().len(), 1);

    let other = service.start(None, None).await.unwrap();
    assert!(service.bookmarks(&other.id).await.unwrap().is_empty());

    assert!(matches!(
        service
            .toggle_bookmark(&session.id, "file", "item-1", "no")
            .await,
        Err(SessionError::InvalidBookmarkKind(_))
    ));
    assert!(matches!(
        service
            .toggle_bookmark("missing", "item", "item-1", "no")
            .await,
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

#[tokio::test]
async fn applying_a_role_preset_rewrites_the_roster_row_and_rejects_unknown_names() {
    let tmp = TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("roles"), None).await.unwrap();
    let agent = service
        .register_agent_with(&session.id, "native", None, None)
        .await
        .unwrap();

    let applied = service
        .apply_role_preset(&session.id, &agent.id, "reviewer")
        .await
        .unwrap();
    assert_eq!(applied.role.as_deref(), Some("reviewer"));
    assert_eq!(applied.model.as_deref(), Some("openai/gpt-5.4-mini"));

    assert!(matches!(
        service
            .apply_role_preset(&session.id, &agent.id, "nonexistent")
            .await,
        Err(SessionError::InvalidRole(_))
    ));
}

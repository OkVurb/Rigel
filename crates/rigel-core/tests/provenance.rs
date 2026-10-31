//! D156: the chain behind a patch is read from records, never inferred. The journal either
//! names the turn, the item, the tool and the model, or the window prints "unknown".

use rigel_core::{PatchPipeline, ProvenanceError, SessionService};
use serde_json::json;

#[tokio::test]
async fn a_patch_traces_back_to_its_turn_item_tool_and_model() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("notes.txt"), "one\n").unwrap();

    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("provenance"), None).await.unwrap();
    let agent = service
        .register_agent_with(
            &session.id,
            "native",
            Some("writer"),
            Some("anthropic/claude-sonnet-4.5"),
        )
        .await
        .unwrap();
    let pipeline = PatchPipeline::new(tmp.path(), service.pool().clone());

    let turn = service
        .append_turn(&agent.id, Some("edit it"))
        .await
        .unwrap();
    let (text, base) = pipeline.read("notes.txt").unwrap();
    let patch = pipeline
        .propose(
            &session.id,
            &agent.id,
            "notes.txt",
            &base,
            &format!("{text}two\n"),
        )
        .await
        .unwrap();
    service
        .append_item(
            &turn.id,
            "file_change",
            "ok",
            &json!({
                "tool": "edit_file",
                "arguments": { "path": "notes.txt" },
                "result": { "patch_id": patch.id, "applied": true },
            }),
            &json!({}),
        )
        .await
        .unwrap();
    service.finish_turn(&turn.id, "completed").await.unwrap();

    let found = rigel_core::provenance::patch(service.pool(), &patch.id)
        .await
        .unwrap();
    assert!(found.complete, "{found:?}");
    assert_eq!(found.turn_id.as_deref(), Some(turn.id.as_str()));
    assert_eq!(found.turn_seq, Some(1));
    assert_eq!(found.turn_status.as_deref(), Some("completed"));
    assert_eq!(found.item_kind.as_deref(), Some("file_change"));
    assert_eq!(found.tool.as_deref(), Some("edit_file"));
    assert_eq!(found.agent_role.as_deref(), Some("writer"));
    assert_eq!(found.harness.as_deref(), Some("native"));
    assert_eq!(found.model.as_deref(), Some("anthropic/claude-sonnet-4.5"));
    assert_eq!(found.path, "notes.txt");
}

/// A patch the transcript never mentioned — a merge writes one, for instance — is not guessed
/// at: the links stay empty and the window says "unknown".
#[tokio::test]
async fn a_patch_with_no_item_reports_the_chain_as_incomplete() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("notes.txt"), "one\n").unwrap();

    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("provenance"), None).await.unwrap();
    let agent = service
        .register_agent_with(&session.id, "native", Some("writer"), Some("test-model"))
        .await
        .unwrap();
    let pipeline = PatchPipeline::new(tmp.path(), service.pool().clone());

    let (text, base) = pipeline.read("notes.txt").unwrap();
    let patch = pipeline
        .propose(
            &session.id,
            &agent.id,
            "notes.txt",
            &base,
            &format!("{text}two\n"),
        )
        .await
        .unwrap();

    let found = rigel_core::provenance::patch(service.pool(), &patch.id)
        .await
        .unwrap();
    assert!(!found.complete);
    assert!(found.turn_id.is_none());
    assert!(found.item_id.is_none());
    assert!(found.tool.is_none());
    // The agent row still names who proposed it, because the patch row itself does.
    assert_eq!(found.model.as_deref(), Some("test-model"));

    assert!(matches!(
        rigel_core::provenance::patch(service.pool(), "missing").await,
        Err(ProvenanceError::UnknownPatch(_))
    ));
}

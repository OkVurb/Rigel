//! Roadmap 84: one command item's chain is readable without guessing, and unknown links carry
//! their reason. The ledger half is exercised by `audit::chain` and the CLI test.

use rigel_core::SessionService;
use rigel_core::audit;
use serde_json::json;

#[tokio::test]
async fn a_command_provenance_chain_is_complete_when_every_link_was_recorded() {
    let project = tempfile::tempdir().unwrap();
    let service = SessionService::open(project.path()).await.unwrap();
    let session = service.start(Some("provenance"), None).await.unwrap();
    let agent = service
        .register_agent_with(
            &session.id,
            "native",
            Some("commander"),
            Some("openai/gpt-5-nano"),
        )
        .await
        .unwrap();
    let turn = service
        .append_turn(&agent.id, Some("run it"))
        .await
        .unwrap();
    let approval = service
        .record_approval(
            &session.id,
            &agent.id,
            Some(&turn.id),
            "command",
            &json!({ "command": "echo audited" }),
            "allowed",
            Some("once"),
            1_700_000_000_000,
        )
        .await
        .unwrap();
    let item = service
        .append_item(
            &turn.id,
            "command",
            "ok",
            &json!({
                "tool": "run_command",
                "step": 3,
                "tool_call_id": "call-abc",
                "arguments": { "command": "echo audited" },
                "result": {
                    "ok": true,
                    "command": "echo audited",
                    "approval_id": approval.approval_id,
                    "snapshot": { "id": "snap-42" },
                    "destructive": []
                }
            }),
            &json!({}),
        )
        .await
        .unwrap();

    let chain = audit::provenance::command(service.pool(), &item.id)
        .await
        .unwrap();
    assert_eq!(chain.command.value.as_deref(), Some("echo audited"));
    assert_eq!(chain.turn.value.as_deref(), Some(turn.id.as_str()));
    assert_eq!(chain.agent.value.as_deref(), Some(agent.id.as_str()));
    assert_eq!(chain.model.value.as_deref(), Some("openai/gpt-5-nano"));
    assert_eq!(chain.step.value, Some(3));
    assert_eq!(chain.tool_call.value.as_ref().unwrap().id, "call-abc");
    assert_eq!(
        chain.approval.value.as_ref().unwrap().decision.as_deref(),
        Some("allowed")
    );
    assert_eq!(chain.snapshot.value.as_deref(), Some("snap-42"));
    assert!(chain.step.is_known() && chain.approval.is_known());

    let ledger = audit::verify(project.path()).unwrap();
    assert!(ledger.valid, "{ledger:?}");
    assert_eq!(ledger.records, 1);
}

#[tokio::test]
async fn a_missing_link_prints_unknown_with_its_reason() {
    let project = tempfile::tempdir().unwrap();
    let service = SessionService::open(project.path()).await.unwrap();
    let session = service.start(Some("old-item"), None).await.unwrap();
    let agent = service
        .register_agent_with(&session.id, "native", Some("commander"), None)
        .await
        .unwrap();
    let turn = service.append_turn(&agent.id, Some("old")).await.unwrap();
    let item = service
        .append_item(
            &turn.id,
            "command",
            "error",
            &json!({
                "tool": "run_command",
                "arguments": { "command": "echo old" },
                "result": {
                    "ok": false,
                    "command": "echo old",
                    "destructive": []
                }
            }),
            &json!({}),
        )
        .await
        .unwrap();

    let chain = audit::provenance::command(service.pool(), &item.id)
        .await
        .unwrap();
    assert_eq!(chain.step.status, "unknown");
    assert!(chain.step.reason.as_deref().unwrap().contains("model-step"));
    assert_eq!(chain.tool_call.status, "unknown");
    assert!(
        chain
            .tool_call
            .reason
            .as_deref()
            .unwrap()
            .contains("tool-call")
    );
    assert_eq!(chain.approval.status, "unknown");
    assert!(
        chain
            .approval
            .reason
            .as_deref()
            .unwrap()
            .contains("approval")
    );
    assert_eq!(chain.model.status, "unknown");
    assert!(chain.model.reason.as_deref().unwrap().contains("no model"));
    assert_eq!(chain.snapshot.status, "unknown");
    assert!(
        chain
            .snapshot
            .reason
            .as_deref()
            .unwrap()
            .contains("not destructive")
    );
}

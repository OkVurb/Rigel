//! Q242: a project MCP server declared in `.mcp.json` is refused before its first tool call
//! until the user sees the real tool list and approves both the server and that tool.
//!
//! The fixture is a local stdio server; no provider key or network request is involved.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rigel_core::agent::approval::ApprovalBus;
use rigel_core::{ApprovalScope, SessionService};
use rigel_mcp::{McpContext, handle};
use serde_json::{Value, json};

async fn context() -> (tempfile::TempDir, McpContext, String) {
    let tmp = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("mcp"), Some(100)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "claude", Some("worker"))
        .await
        .unwrap();
    let turn = service
        .append_turn(&agent.id, Some("use a project tool"))
        .await
        .unwrap();
    std::fs::write(
        tmp.path().join(".mcp.json"),
        serde_json::to_string(&json!({
            "mcpServers": {
                "demo": {
                    "command": env!("CARGO_BIN_EXE_fake-mcp-server"),
                    "args": []
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let ctx = McpContext {
        project_root: tmp.path().to_path_buf(),
        session_id: session.id,
        agent_id: agent.id,
        index_dir: tmp.path().join("index"),
        approval_wait: Duration::from_secs(5),
        turn_id: Some(turn.id.clone()),
        tool_lists: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
    };
    (tmp, ctx, turn.id)
}

fn request() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "rigel_approve",
            "arguments": {
                "tool_name": "mcp__demo__echo",
                "input": { "text": "hello" }
            }
        }
    })
}

fn decision(response: Value) -> Value {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no tool text in {response}"));
    serde_json::from_str(text).unwrap()
}

#[tokio::test]
async fn a_project_mcp_server_is_refused_until_the_card_approves_it() {
    let (_tmp, ctx, turn) = context().await;
    let response = handle(&ctx, request()).await.unwrap();
    let decision = decision(response);

    assert_eq!(decision["behavior"], "deny");
    assert_eq!(decision["code"], "MCP_SERVER_DENIED");
    assert_eq!(decision["server"], "demo");
    assert!(
        decision["message"]
            .as_str()
            .unwrap()
            .contains("did not approve")
    );

    let service = SessionService::open(&ctx.project_root).await.unwrap();
    let items = service.items(&turn).await.unwrap();
    assert!(
        items.iter().any(|item| {
            item.kind == "approval"
                && item.body["server"] == "demo"
                && item.body["decision"] == "deny"
                && item.body["gate"] == "Q242"
        }),
        "the refusal is recorded in the transcript: {items:?}"
    );
}

#[tokio::test]
async fn an_approved_project_server_shows_its_tools_and_records_the_run() {
    let (_tmp, ctx, turn) = context().await;
    let bus = ApprovalBus::new(&ctx.index_dir);
    let responder = tokio::spawn(async move {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut server_answered = false;
        while Instant::now() < deadline {
            for pending in bus.pending() {
                if pending.kind == "mcp_server" {
                    let tools = pending.detail.as_ref().unwrap()["tools"]
                        .as_array()
                        .unwrap();
                    let names: Vec<&str> = tools
                        .iter()
                        .filter_map(|tool| tool["name"].as_str())
                        .collect();
                    assert!(names.contains(&"echo"), "real tool list: {names:?}");
                    assert!(
                        pending.detail.as_ref().unwrap()["command"]
                            .as_str()
                            .unwrap()
                            .contains("fake-mcp-server")
                    );
                    bus.answer(&pending.id, Some(ApprovalScope::Project))
                        .unwrap();
                    server_answered = true;
                } else if pending.kind == "mcp_tool" {
                    assert!(server_answered, "the server is asked first");
                    assert_eq!(pending.detail.as_ref().unwrap()["tool"], "echo");
                    assert_eq!(
                        pending.detail.as_ref().unwrap()["tools"]
                            .as_array()
                            .unwrap()
                            .len(),
                        2
                    );
                    bus.answer(&pending.id, Some(ApprovalScope::Once)).unwrap();
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the two approval questions did not arrive");
    });

    let response = handle(&ctx, request()).await.unwrap();
    responder.await.unwrap();
    let decision = decision(response);

    assert_eq!(decision["behavior"], "allow");
    assert_eq!(decision["scope"], "once");
    assert_eq!(decision["server"], "demo");
    assert_eq!(decision["tool"], "echo");
    assert!(
        decision["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool == "echo")
    );

    let service = SessionService::open(&ctx.project_root).await.unwrap();
    let items = service.items(&turn).await.unwrap();
    assert!(
        items.iter().any(|item| {
            item.kind == "approval"
                && item.body["server"] == "demo"
                && item.body["mcp_tool"] == "echo"
                && item.body["decision"] == "allow"
        }),
        "the permitted run is recorded in the transcript: {items:?}"
    );
}

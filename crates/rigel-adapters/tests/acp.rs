//! The ACP client against a scripted agent.
//!
//! Cursor is not installed on this machine and Cline is, but both are exercised through the same
//! fixture binary so the suite is deterministic and needs no vendor session: the fixture stands in
//! for the wire protocol. Every shape it prints follows the published v1 schema, and the assertions
//! are about what the client *sends back*: the approval outcome, the cancel notification, the
//! version it accepts. That is the part a fake agent cannot fake for us.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rigel_adapters::acp::{AcpConfig, AcpEvent, ApprovalDecision, ApprovalRequest, Approver};
use rigel_adapters::{AcpClient, Adapter, AuthMode, Harness};
use rigel_core::ItemKind;
use rigel_core::credentials::Secret;

fn fake_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake-acp"))
}

/// Records every approval it is asked about, and answers with one fixed decision.
struct Recorded {
    decision: ApprovalDecision,
    seen: Mutex<Vec<ApprovalRequest>>,
}

#[async_trait]
impl Approver for Recorded {
    async fn approve(&self, request: &ApprovalRequest) -> ApprovalDecision {
        self.seen.lock().unwrap().push(request.clone());
        self.decision
    }
}

/// An approver that never answers, standing in for a UI that has not rendered the prompt yet.
struct NeverAnswers;

#[async_trait]
impl Approver for NeverAnswers {
    async fn approve(&self, _request: &ApprovalRequest) -> ApprovalDecision {
        std::future::pending().await
    }
}

fn config(cwd: &Path, mode: Option<&str>) -> AcpConfig {
    let mut config = AcpConfig::cursor(cwd);
    config.exe = fake_path();
    config.timeout = Duration::from_secs(10);
    config.vendor_login = None;
    config.env.insert(
        "CURSOR_API_KEY".to_string(),
        Secret::new("ck-test-must-not-print"),
    );
    config.env.insert(
        "FAKE_ACP_RECORD".to_string(),
        Secret::new(cwd.join("record.txt").display().to_string()),
    );
    if let Some(mode) = mode {
        config
            .env
            .insert("FAKE_ACP_MODE".to_string(), Secret::new(mode));
    }
    config
}

fn recorded(cwd: &Path) -> String {
    std::fs::read_to_string(cwd.join("record.txt")).unwrap_or_default()
}

#[tokio::test]
async fn a_turn_streams_updates_and_routes_the_approval() {
    let dir = tempfile::tempdir().unwrap();
    let approver = Arc::new(Recorded {
        decision: ApprovalDecision::AllowOnce,
        seen: Mutex::new(Vec::new()),
    });
    let mut client = AcpClient::spawn(config(dir.path(), None), approver.clone())
        .await
        .expect("the agent should initialize");
    let session = client.new_session().await.expect("session/new");

    let mut events = Vec::new();
    let turn = client
        .prompt("say pong", |event| events.push(event))
        .await
        .expect("the turn should finish");

    assert_eq!(session, "sess-1");
    assert_eq!(turn.text, "pong");
    assert_eq!(turn.stop_reason, "end_turn");
    let usage = turn.usage.expect("usage_update");
    assert_eq!(usage.used, 5300);
    assert_eq!(usage.size, 200_000);
    assert_eq!(usage.cost_micros, Some(4500));

    assert_eq!(
        events,
        vec![
            AcpEvent::MessageDelta {
                text: "po".to_string()
            },
            AcpEvent::MessageDelta {
                text: "ng".to_string()
            },
            // P4.8: the agent's `kind` decides the item kind, and the raw input is kept so the
            // command the transcript shows is the command the agent asked to run.
            AcpEvent::ToolCall {
                tool_call_id: "call-1".to_string(),
                title: "Run the test suite".to_string(),
                kind: Some("execute".to_string()),
                item_kind: ItemKind::Command,
                operation: "execute",
                status: Some("pending".to_string()),
                raw_input: Some(serde_json::json!({ "command": "cargo test" })),
            },
            AcpEvent::ToolCallUpdate {
                tool_call_id: "call-1".to_string(),
                status: Some("completed".to_string()),
                text: Some("done".to_string()),
            },
            AcpEvent::Unmapped {
                kind: "future_update_from_a_newer_agent".to_string()
            },
            AcpEvent::Usage {
                used: 5300,
                size: 200_000,
                cost_micros: Some(4500),
            },
        ]
    );

    let approvals = approver.seen.lock().unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].tool_call_id, "call-1");
    assert_eq!(approvals[0].title, "Run the test suite");
    assert_eq!(approvals[0].kind.as_deref(), Some("execute"));
    drop(approvals);

    // What actually went over the wire: argv, injected key, absolute cwd, chosen option.
    let record = recorded(dir.path());
    assert!(record.contains("argv=acp"), "{record}");
    assert!(record.contains("key=set"), "{record}");
    assert!(
        record.contains(&format!("cwd={}", dir.path().display())),
        "{record}"
    );
    assert!(
        record.contains(r#""outcome":"selected""#) && record.contains(r#""optionId":"allow""#),
        "{record}"
    );
}

#[tokio::test]
async fn a_decision_the_agent_did_not_offer_is_cancelled_not_upgraded() {
    let dir = tempfile::tempdir().unwrap();
    let approver = Arc::new(Recorded {
        decision: ApprovalDecision::AllowOnce,
        seen: Mutex::new(Vec::new()),
    });
    let mut client = AcpClient::spawn(config(dir.path(), Some("reject-only")), approver)
        .await
        .expect("the agent should initialize");
    client.new_session().await.unwrap();
    client.prompt("say pong", |_| {}).await.unwrap();

    let record = recorded(dir.path());
    assert!(
        record.contains(r#"outcome={"outcome":"cancelled"}"#),
        "an unoffered allow must never be invented: {record}"
    );
}

#[tokio::test]
async fn a_different_protocol_version_is_refused_at_the_handshake() {
    let dir = tempfile::tempdir().unwrap();
    let approver = Arc::new(Recorded {
        decision: ApprovalDecision::RejectOnce,
        seen: Mutex::new(Vec::new()),
    });
    let error = AcpClient::spawn(config(dir.path(), Some("wrong-version")), approver)
        .await
        .expect_err("protocol 99 must be refused");
    assert!(
        error.to_string().contains("protocol 99"),
        "unexpected: {error}"
    );
}

#[tokio::test]
async fn a_turn_that_hangs_is_cancelled_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let approver = Arc::new(Recorded {
        decision: ApprovalDecision::RejectOnce,
        seen: Mutex::new(Vec::new()),
    });
    let mut config = config(dir.path(), Some("wait-cancel"));
    config.timeout = Duration::from_millis(600);
    let mut client = AcpClient::spawn(config, approver)
        .await
        .expect("the agent should initialize");
    client.new_session().await.unwrap();

    let error = client
        .prompt("say pong", |_| {})
        .await
        .expect_err("a silent agent must not hang the turn");
    assert!(
        error.to_string().contains("timed out and was cancelled"),
        "unexpected: {error}"
    );
    assert!(
        recorded(dir.path()).contains("cancel=received"),
        "the agent must be told to stop: {}",
        recorded(dir.path())
    );
}

#[tokio::test]
async fn a_cancelled_turn_answers_its_pending_permission() {
    let dir = tempfile::tempdir().unwrap();
    let approver: Arc<dyn Approver> = Arc::new(NeverAnswers);
    let mut config = config(dir.path(), Some("permission-hang"));
    config.timeout = Duration::from_millis(600);
    let mut client = AcpClient::spawn(config, approver)
        .await
        .expect("the agent should initialize");
    client.new_session().await.unwrap();
    let error = client
        .prompt("delete build", |_| {})
        .await
        .expect_err("the permission request never resolves");

    assert!(
        error.to_string().contains("timed out and was cancelled"),
        "unexpected: {error}"
    );
    let record = recorded(dir.path());
    assert!(record.contains("cancel=received"), "{record}");
    assert!(
        record.contains(r#"outcome={"outcome":"cancelled"}"#),
        "the pending request must be answered with cancelled: {record}"
    );
}

#[tokio::test]
async fn a_relative_cwd_is_refused_before_spawn() {
    let approver = Arc::new(Recorded {
        decision: ApprovalDecision::RejectOnce,
        seen: Mutex::new(Vec::new()),
    });
    let error = AcpClient::spawn(config(Path::new("relative/path"), None), approver)
        .await
        .expect_err("ACP requires an absolute cwd");
    assert!(error.to_string().contains("absolute cwd"), "{error}");
}

#[tokio::test]
async fn a_subscription_only_agent_is_not_spawned() {
    let dir = tempfile::tempdir().unwrap();
    let approver = Arc::new(Recorded {
        decision: ApprovalDecision::RejectOnce,
        seen: Mutex::new(Vec::new()),
    });
    let mut config = config(dir.path(), None);
    config.env.retain(|name, _| name != "CURSOR_API_KEY");
    let error = AcpClient::spawn(config, approver)
        .await
        .expect_err("no key means no spawn");
    assert!(
        matches!(
            error,
            rigel_adapters::AdapterError::RequiresApiKey {
                harness: Harness::Cursor
            }
        ),
        "unexpected: {error}"
    );
    assert!(
        !dir.path().join("record.txt").exists(),
        "the process must not have started"
    );
}

#[tokio::test]
async fn cline_takes_its_own_key_and_speaks_the_same_protocol() {
    let dir = tempfile::tempdir().unwrap();
    let approver = Arc::new(Recorded {
        decision: ApprovalDecision::AllowOnce,
        seen: Mutex::new(Vec::new()),
    });
    let mut config = AcpConfig::cline(dir.path());
    config.exe = fake_path();
    config.timeout = Duration::from_secs(10);
    config.vendor_login = None;
    config.env.insert(
        "CLINE_API_KEY".to_string(),
        Secret::new("sk-cline-test-must-not-print"),
    );
    config.env.insert(
        "FAKE_ACP_RECORD".to_string(),
        Secret::new(dir.path().join("record.txt").display().to_string()),
    );
    assert_eq!(config.args, vec!["--acp".to_string()]);

    let mut client = AcpClient::spawn(config, approver.clone())
        .await
        .expect("Cline should initialize with its own key");
    client.new_session().await.expect("session/new");
    let turn = client
        .prompt("say pong", |_| {})
        .await
        .expect("the turn should finish");

    assert_eq!(turn.text, "pong");
    assert_eq!(approver.seen.lock().unwrap().len(), 1);
    let record = recorded(dir.path());
    assert!(record.contains("argv=--acp"), "{record}");
    assert!(
        record.contains("cline_key=set"),
        "the Cline key must reach the child: {record}"
    );
    assert!(
        record.contains("key=unset"),
        "Cursor's key must not leak into Cline: {record}"
    );
}

#[tokio::test]
async fn the_probe_reports_the_agents_version_and_auth() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path(), None);
    let info = Adapter::probe(&config).await.unwrap();
    assert_eq!(info.harness, Harness::Cursor);
    assert!(info.installed);
    assert_eq!(info.version.as_deref(), Some("1.4.2"));
    assert_eq!(info.auth_mode, AuthMode::ApiKey);
    assert!(info.capabilities.acp);

    let mut without_key = config;
    without_key.env.retain(|name, _| name != "CURSOR_API_KEY");
    assert_eq!(
        Adapter::probe(&without_key).await.unwrap().auth_mode,
        AuthMode::None
    );
}

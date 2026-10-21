//! The Codex adapter against a scripted binary: both transports, the fallback, and the probe.
//!
//! The real Codex is exercised by `scripts/codex-router-probe.mjs`; these tests must pass on a
//! machine that has never installed it, so the fixture binary stands in for the protocol.

use std::path::PathBuf;
use std::time::Duration;

use rigel_adapters::{
    Adapter, ApprovalPolicy, CodexAdapter, CodexConfig, CodexEvent, SandboxMode, Transport,
};

fn fake_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake-codex"))
}

fn config(cwd: &std::path::Path, transport: Transport) -> CodexConfig {
    CodexConfig {
        exe: fake_path(),
        base_url: "http://127.0.0.1:20128/v1".to_string(),
        model: "gpt-5.6-sol-medium".to_string(),
        cwd: cwd.to_path_buf(),
        sandbox: SandboxMode::ReadOnly,
        approval: ApprovalPolicy::Never,
        transport,
        timeout: Duration::from_secs(10),
    }
}

#[tokio::test]
async fn app_server_streams_deltas_and_reports_usage() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = CodexAdapter::new(config(dir.path(), Transport::AppServer));

    let mut deltas = Vec::new();
    let result = adapter
        .run_turn("say pong", |event| {
            if let CodexEvent::Delta { text } = event {
                deltas.push(text);
            }
        })
        .await
        .expect("app-server turn should complete");

    assert_eq!(deltas, vec!["po".to_string(), "ng".to_string()]);
    assert_eq!(result.text, "pong");
    assert_eq!(result.transport, Transport::AppServer);
    assert_eq!(result.thread_id.as_deref(), Some("th-1"));
    assert!(!result.fell_back);
    let usage = result.usage.expect("usage from turn/completed");
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 2);
}

#[tokio::test]
async fn auto_falls_back_to_exec_when_app_server_dies_before_answering() {
    let dir = tempfile::tempdir().unwrap();
    // The fake exits 9 on app-server when this marker exists; the adapter must then use exec.
    std::fs::write(dir.path().join("FAIL_APP_SERVER"), b"").unwrap();
    let adapter = CodexAdapter::new(config(dir.path(), Transport::Auto));

    let mut notices = Vec::new();
    let result = adapter
        .run_turn("say pong", |event| {
            if let CodexEvent::Notice { message } = event {
                notices.push(message);
            }
        })
        .await
        .expect("exec fallback should answer");

    assert_eq!(result.text, "pong");
    assert_eq!(result.transport, Transport::Exec);
    assert!(result.fell_back);
    assert_eq!(result.thread_id.as_deref(), Some("th-exec"));
    assert!(
        notices
            .iter()
            .any(|note| note.contains("falling back to exec")),
        "expected a fallback notice, got {notices:?}"
    );
    let usage = result.usage.expect("usage from turn.completed");
    assert_eq!(usage.cached_input_tokens, 4);
}

#[tokio::test]
async fn probe_reports_the_installed_version() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = CodexAdapter::new(config(dir.path(), Transport::AppServer));
    let info = adapter
        .probe()
        .await
        .expect("fake binary answers --version");

    assert!(info.installed);
    assert_eq!(info.version.as_deref(), Some("9.9.9"));
    assert!(info.capabilities.json_stream);
    assert!(info.capabilities.programmatic_approval);
    assert!(!info.capabilities.acp);
}

#[tokio::test]
async fn a_missing_binary_is_not_installed() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path(), Transport::AppServer);
    cfg.exe = dir.path().join("does-not-exist");
    let adapter = CodexAdapter::new(cfg);

    assert!(adapter.probe().await.is_err());
}

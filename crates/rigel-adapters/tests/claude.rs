//! The Claude adapter against a scripted binary.
//!
//! These tests must pass on a machine that has never installed Claude Code, so the fixture binary
//! stands in for the protocol. The shapes it prints were copied from a real 2.1.251 turn.
//!
//! The fixture is configured through the injected environment rather than the test process's own
//! variables: cargo runs these tests in parallel threads of one process, so a global variable would
//! leak between them (and it did, before the first green run).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rigel_adapters::{
    Adapter, AdapterError, AuthMode, ClaudeAdapter, ClaudeConfig, ClaudeEvent, Harness,
    PermissionMode,
};
use rigel_core::credentials::Secret;

const KEY: &str = "sk-ant-test-key-that-must-not-print";

fn fake_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake-claude"))
}

/// A config pointed at the fixture, with a key and an argv recorder in the child environment.
fn config(cwd: &Path) -> ClaudeConfig {
    let mut config = ClaudeConfig::new(cwd);
    config.exe = fake_path();
    config.model = Some("free-stack".to_string());
    config.permission_mode = PermissionMode::DontAsk;
    config.timeout = Duration::from_secs(10);
    config.vendor_login = None;
    config
        .env
        .insert("ANTHROPIC_API_KEY".to_string(), Secret::new(KEY));
    config.env.insert(
        "FAKE_CLAUDE_ARGS".to_string(),
        Secret::new(cwd.join("args.txt").display().to_string()),
    );
    config
}

fn with_mode(mut config: ClaudeConfig, mode: &str) -> ClaudeConfig {
    config
        .env
        .insert("FAKE_CLAUDE_MODE".to_string(), Secret::new(mode));
    config
}

async fn run(
    config: ClaudeConfig,
    seen: Option<Arc<Mutex<Vec<String>>>>,
) -> Result<rigel_adapters::ClaudeTurn, AdapterError> {
    ClaudeAdapter::new(config)
        .run_turn("say pong", move |event| {
            let Some(seen) = seen.as_ref() else { return };
            let text = match event {
                ClaudeEvent::Delta { text } | ClaudeEvent::Message { text } => text,
                _ => return,
            };
            seen.lock().unwrap().push(text);
        })
        .await
}

#[tokio::test]
async fn a_turn_streams_deltas_and_reports_usage() {
    let dir = tempfile::tempdir().unwrap();
    let turn = run(config(dir.path()), None)
        .await
        .expect("the turn should complete");

    assert_eq!(turn.text, "pong");
    assert_eq!(turn.session_id.as_deref(), Some("cs-1"));
    let usage = turn.usage.expect("usage from the result line");
    assert_eq!(usage.input_tokens, 1207);
    assert_eq!(usage.output_tokens, 5);
    assert_eq!(usage.cost_micros, Some(6160));

    // The headless contract: prompt on argv, key never on argv, `--bare` so no vendor login.
    let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
    let argv: Vec<&str> = args.lines().collect();
    assert!(argv.contains(&"--bare"), "argv: {args:?}");
    assert!(argv.contains(&"-p"), "argv: {args:?}");
    assert!(argv.contains(&"stream-json"), "argv: {args:?}");
    assert!(
        argv.contains(&"--include-partial-messages"),
        "argv: {args:?}"
    );
    assert!(argv.contains(&"dontAsk"), "argv: {args:?}");
    assert_eq!(argv.last(), Some(&"say pong"), "argv: {args:?}");
    assert!(
        !argv.iter().any(|arg| arg.contains("sk-ant")),
        "the key must never appear on argv: {args:?}"
    );
}

#[tokio::test]
async fn a_turn_reports_each_delta_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    // The fixture is not in echo mode, so the stream is the two "po"/"ng" deltas.
    run(config(dir.path()), Some(seen.clone()))
        .await
        .expect("the turn should complete");
    // Two deltas, then the completed assistant message the CLI repeats in full.
    assert_eq!(
        *seen.lock().unwrap(),
        vec!["po".to_string(), "ng".to_string(), "pong".to_string()]
    );
}

#[tokio::test]
async fn a_missing_key_refuses_before_spawning() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config(dir.path());
    config.env.retain(|name, _| name != "ANTHROPIC_API_KEY");

    let error = run(config, None).await.expect_err("no key means no turn");
    assert!(matches!(
        error,
        AdapterError::RequiresApiKey {
            harness: Harness::Claude
        }
    ));
    assert!(
        !dir.path().join("args.txt").exists(),
        "the harness must not be spawned without a key"
    );
}

#[tokio::test]
async fn a_subscription_identity_is_killed_at_the_first_event() {
    let dir = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let error = run(with_mode(config(dir.path()), "subscription"), None)
        .await
        .expect_err("a subscription identity is not an API key");

    assert!(matches!(
        error,
        AdapterError::RequiresApiKey {
            harness: Harness::Claude
        }
    ));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the child must be killed instead of waited on: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn an_init_without_a_key_source_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let error = run(with_mode(config(dir.path()), "missing-source"), None)
        .await
        .expect_err("a missing source must not be treated as an API key");
    assert!(
        matches!(
            error,
            AdapterError::RequiresApiKey {
                harness: Harness::Claude
            }
        ),
        "unexpected: {error}"
    );
}

#[tokio::test]
async fn the_key_never_reaches_an_event() {
    let dir = tempfile::tempdir().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let turn = run(
        with_mode(config(dir.path()), "echo-key"),
        Some(seen.clone()),
    )
    .await
    .expect("the turn should complete");

    assert!(turn.text.contains("<redacted>"), "{}", turn.text);
    assert!(!turn.text.contains(KEY), "{}", turn.text);
    for text in seen.lock().unwrap().iter() {
        assert!(!text.contains(KEY), "leaked: {text}");
    }
}

#[tokio::test]
async fn a_blocked_turn_surfaces_the_harness_error() {
    let dir = tempfile::tempdir().unwrap();
    let error = run(with_mode(config(dir.path()), "error"), None)
        .await
        .expect_err("an is_error result is a failed turn");

    let AdapterError::Other(message) = error else {
        panic!("expected a turn error");
    };
    assert!(
        message.contains("tool permission denied"),
        "unexpected message: {message}"
    );
}

#[tokio::test]
async fn a_silent_nonzero_exit_is_an_error_not_an_empty_turn() {
    let dir = tempfile::tempdir().unwrap();
    let error = run(with_mode(config(dir.path()), "silent"), None)
        .await
        .expect_err("no message means no turn");

    let AdapterError::Other(message) = error else {
        panic!("expected a turn error");
    };
    assert!(
        message.contains("without a result line"),
        "unexpected: {message}"
    );
}

#[tokio::test]
async fn a_stream_without_a_result_line_is_not_a_completed_turn() {
    let dir = tempfile::tempdir().unwrap();
    let error = run(with_mode(config(dir.path()), "partial"), None)
        .await
        .expect_err("a truncated stream must not be reported as success");
    let AdapterError::Other(message) = error else {
        panic!("expected a turn error");
    };
    assert!(
        message.contains("without a result line"),
        "unexpected: {message}"
    );
}

#[tokio::test]
async fn the_probe_reports_the_fixture_version_and_auth() {
    let dir = tempfile::tempdir().unwrap();
    let info = ClaudeAdapter::new(config(dir.path()))
        .probe()
        .await
        .unwrap();
    assert_eq!(info.harness, Harness::Claude);
    assert!(info.installed);
    assert_eq!(info.version.as_deref(), Some("2.1.251"));
    assert_eq!(info.path.as_deref(), Some(fake_path().as_path()));
    assert_eq!(info.auth_mode, AuthMode::ApiKey);
    assert!(info.capabilities.json_stream);
    assert!(info.capabilities.programmatic_approval);
    assert!(info.install_hint.is_none());

    let mut without_key = config(dir.path());
    without_key
        .env
        .retain(|name, _| name != "ANTHROPIC_API_KEY");
    let info = ClaudeAdapter::new(without_key).probe().await.unwrap();
    assert_eq!(info.auth_mode, AuthMode::None);
}

#[tokio::test]
async fn a_missing_binary_is_probed_not_errored() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config(dir.path());
    config.exe = dir.path().join("definitely-not-claude");
    let info = ClaudeAdapter::new(config).probe().await.unwrap();
    assert!(!info.installed);
    assert!(info.version.is_none());
    assert!(info.install_hint.is_some());
}

#[tokio::test]
async fn the_permission_tool_and_its_mcp_config_are_passed_together() {
    let dir = tempfile::tempdir().unwrap();
    let mcp_config = dir.path().join("rigel-mcp.json");
    std::fs::write(&mcp_config, "{}").unwrap();
    let config = config(dir.path()).with_permission_tool(&mcp_config, "mcp__rigel__rigel_approve");
    run(config, None).await.expect("the turn still answers");

    let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
    let argv: Vec<&str> = args.lines().collect();
    let config_at = argv
        .iter()
        .position(|arg| *arg == "--mcp-config")
        .expect("--mcp-config is passed");
    assert_eq!(argv[config_at + 1], mcp_config.display().to_string());
    let tool_at = argv
        .iter()
        .position(|arg| *arg == "--permission-prompt-tool")
        .expect("--permission-prompt-tool is passed");
    assert_eq!(argv[tool_at + 1], "mcp__rigel__rigel_approve");
    // The prompt must stay last: everything before it is a flag Claude parses.
    assert_eq!(argv.last(), Some(&"say pong"));
}

#[tokio::test]
async fn without_a_permission_tool_no_mcp_arguments_are_invented() {
    let dir = tempfile::tempdir().unwrap();
    run(config(dir.path()), None).await.expect("plain turn");
    let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
    assert!(!args.contains("--mcp-config"), "argv: {args:?}");
    assert!(!args.contains("--permission-prompt-tool"), "argv: {args:?}");
}

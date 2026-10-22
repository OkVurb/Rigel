//! The Claude adapter against a scripted binary.
//!
//! These tests must pass on a machine that has never installed Claude Code, so the fixture binary
//! stands in for the protocol. The shapes it prints were copied from a real 2.1.251 turn.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rigel_adapters::{
    Adapter, AdapterError, AuthMode, ClaudeAdapter, ClaudeConfig, ClaudeEvent, Harness,
    PermissionMode,
};
use rigel_core::credentials::Secret;

fn fake_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake-claude"))
}

/// The fixture reads `FAKE_CLAUDE_MODE` from the process environment, which every test in this
/// binary shares. Serialise them so one test's mode cannot answer another test's turn.
async fn env_lock() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

fn config(cwd: &Path) -> ClaudeConfig {
    let mut config = ClaudeConfig::new(cwd);
    config.exe = fake_path();
    config.model = Some("free-stack".to_string());
    config.permission_mode = PermissionMode::DontAsk;
    config.timeout = Duration::from_secs(10);
    config.vendor_login = None;
    config.env.insert(
        "ANTHROPIC_API_KEY".to_string(),
        Secret::new("sk-ant-test-key-that-must-not-print"),
    );
    config
}

#[tokio::test]
async fn a_turn_streams_deltas_and_reports_usage() {
    let _env = env_lock().await;
    let dir = tempfile::tempdir().unwrap();
    let args_file = dir.path().join("args.txt");
    // SAFETY: the test process is single-threaded with respect to this variable's use here.
    unsafe { std::env::set_var("FAKE_CLAUDE_ARGS", &args_file) };
    unsafe { std::env::remove_var("FAKE_CLAUDE_MODE") };

    let adapter = ClaudeAdapter::new(config(dir.path()));
    let mut deltas = Vec::new();
    let turn = adapter
        .run_turn("say pong", |event| {
            if let ClaudeEvent::Delta { text } = event {
                deltas.push(text);
            }
        })
        .await
        .expect("the turn should complete");

    assert_eq!(deltas, vec!["po".to_string(), "ng".to_string()]);
    assert_eq!(turn.text, "pong");
    assert_eq!(turn.session_id.as_deref(), Some("cs-1"));
    let usage = turn.usage.expect("usage from the result line");
    assert_eq!(usage.input_tokens, 1207);
    assert_eq!(usage.output_tokens, 5);
    assert_eq!(usage.cost_micros, Some(6160));

    // The headless contract: prompt on argv, key never on argv, `--bare` so no vendor login.
    let args = std::fs::read_to_string(&args_file).unwrap();
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
async fn a_missing_key_refuses_before_spawning() {
    let _env = env_lock().await;
    let dir = tempfile::tempdir().unwrap();
    let args_file = dir.path().join("args.txt");
    unsafe { std::env::set_var("FAKE_CLAUDE_ARGS", &args_file) };
    unsafe { std::env::remove_var("FAKE_CLAUDE_MODE") };

    let mut config = config(dir.path());
    config.env.clear();
    let adapter = ClaudeAdapter::new(config);
    let error = adapter
        .run_turn("say pong", |_| {})
        .await
        .expect_err("no key means no turn");
    assert!(matches!(
        error,
        AdapterError::RequiresApiKey {
            harness: Harness::Claude
        }
    ));
    assert!(
        !args_file.exists(),
        "the harness must not be spawned without a key"
    );
}

#[tokio::test]
async fn a_subscription_identity_is_killed_at_the_first_event() {
    let _env = env_lock().await;
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::remove_var("FAKE_CLAUDE_ARGS") };
    unsafe { std::env::set_var("FAKE_CLAUDE_MODE", "subscription") };

    let adapter = ClaudeAdapter::new(config(dir.path()));
    let started = Instant::now();
    let error = adapter
        .run_turn("say pong", |_| {})
        .await
        .expect_err("a subscription identity is not an API key");
    unsafe { std::env::remove_var("FAKE_CLAUDE_MODE") };

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
async fn the_key_never_reaches_an_event() {
    let _env = env_lock().await;
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::remove_var("FAKE_CLAUDE_ARGS") };
    unsafe { std::env::set_var("FAKE_CLAUDE_MODE", "echo-key") };

    let adapter = ClaudeAdapter::new(config(dir.path()));
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    let turn = adapter
        .run_turn("say the key", move |event| {
            let text = match event {
                ClaudeEvent::Delta { text } | ClaudeEvent::Message { text } => text,
                _ => return,
            };
            sink.lock().unwrap().push(text);
        })
        .await
        .expect("the turn should complete");
    unsafe { std::env::remove_var("FAKE_CLAUDE_MODE") };

    assert!(turn.text.contains("<redacted>"), "{}", turn.text);
    assert!(!turn.text.contains("sk-ant-test-key"), "{}", turn.text);
    for text in seen.lock().unwrap().iter() {
        assert!(!text.contains("sk-ant-test-key"), "leaked: {text}");
    }
}

#[tokio::test]
async fn a_blocked_turn_surfaces_the_harness_error() {
    let _env = env_lock().await;
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::remove_var("FAKE_CLAUDE_ARGS") };
    unsafe { std::env::set_var("FAKE_CLAUDE_MODE", "error") };

    let adapter = ClaudeAdapter::new(config(dir.path()));
    let error = adapter
        .run_turn("use a tool", |_| {})
        .await
        .expect_err("an is_error result is a failed turn");
    unsafe { std::env::remove_var("FAKE_CLAUDE_MODE") };

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
    let _env = env_lock().await;
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::remove_var("FAKE_CLAUDE_ARGS") };
    unsafe { std::env::set_var("FAKE_CLAUDE_MODE", "silent") };

    let adapter = ClaudeAdapter::new(config(dir.path()));
    let error = adapter
        .run_turn("say pong", |_| {})
        .await
        .expect_err("no message means no turn");
    unsafe { std::env::remove_var("FAKE_CLAUDE_MODE") };

    let AdapterError::Other(message) = error else {
        panic!("expected a turn error");
    };
    assert!(message.contains("no message"), "unexpected: {message}");
}

#[tokio::test]
async fn the_probe_reports_the_fixture_version_and_auth() {
    let _env = env_lock().await;
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::remove_var("FAKE_CLAUDE_ARGS") };
    unsafe { std::env::remove_var("FAKE_CLAUDE_MODE") };

    let adapter = ClaudeAdapter::new(config(dir.path()));
    let info = adapter.probe().await.unwrap();
    assert_eq!(info.harness, Harness::Claude);
    assert!(info.installed);
    assert_eq!(info.version.as_deref(), Some("2.1.251"));
    assert_eq!(info.path.as_deref(), Some(fake_path().as_path()));
    assert_eq!(info.auth_mode, AuthMode::ApiKey);
    assert!(info.capabilities.json_stream);
    assert!(info.capabilities.programmatic_approval);
    assert!(info.install_hint.is_none());

    let mut without_key = config(dir.path());
    without_key.env.clear();
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

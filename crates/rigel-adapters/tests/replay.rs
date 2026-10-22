//! Golden-fixture replay (P4.7).
//!
//! Each harness has five recorded streams under `tests/fixtures/golden/<harness>/`: start, tool
//! call, approval, error and interrupt. The fixture binaries replay the file verbatim, so these
//! tests run the *production* decode path — same `run_turn` / `prompt` loop the app uses — against
//! a stream that stands still. A change in the vendor's shape, or in how Rigel classifies a line,
//! fails here before it reaches a session.
//!
//! What the fixtures pin, and what they deliberately do not:
//!
//! - Codex's approval request is answered in place: the fixture pins that the question is
//!   recorded with the refusal and that no item claims the command ran. Claude's permission round
//!   trip runs through its own MCP tool, whose contract is tested in `rigel-mcp`, not here.
//! - "Interrupt" is each protocol's own signature: ACP acknowledges `cancelled` on the prompt
//!   response, while Claude and Codex app-server die mid-stream, which is what a killed child
//!   looks like from Rigel's side.
//! - The streams are authored to the documented shapes (Claude from the 21 Oct 2.1.251 capture,
//!   Codex from the `exec --json` / app-server docs and the router probe, ACP from the v1
//!   schema). They pin decoding, not a live capture; re-recording them against installed binaries
//!   is P4.12's job, where a real turn per harness is already required.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rigel_adapters::acp::{AcpConfig, AcpEvent, ApprovalDecision, ApprovalRequest, Approver};
use rigel_adapters::{
    AcpClient, ClaudeAdapter, ClaudeConfig, ClaudeEvent, CodexAdapter, CodexConfig, CodexEvent,
    PermissionMode, Transport,
};
use rigel_core::ItemKind;
use rigel_core::credentials::Secret;

const SCENARIOS: [&str; 5] = ["start", "tool_call", "approval", "error", "interrupt"];

fn golden(harness: &str, scenario: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/golden")
        .join(harness)
        .join(format!("{scenario}.jsonl"))
}

/// Every scenario has a fixture, and every fixture belongs to a scenario. A new recording with no
/// replay case (or a renamed case with no file) fails here instead of being quietly untested.
#[test]
fn every_golden_fixture_has_a_scenario() {
    for harness in ["claude", "codex", "acp"] {
        let dir = golden(harness, "start")
            .parent()
            .expect("golden dir")
            .to_path_buf();
        let mut found: Vec<String> = std::fs::read_dir(&dir)
            .expect("the golden fixture directory")
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                name.strip_suffix(".jsonl").map(str::to_string)
            })
            .collect();
        found.sort();
        let mut expected: Vec<String> = SCENARIOS.iter().map(|name| name.to_string()).collect();
        expected.sort();
        assert_eq!(found, expected, "{harness} fixtures");
    }
}

/* ---------------------------------- Claude --------------------------------- */

fn claude_config(cwd: &Path, scenario: &str) -> ClaudeConfig {
    let mut config = ClaudeConfig::new(cwd);
    config.exe = PathBuf::from(env!("CARGO_BIN_EXE_fake-claude"));
    config.permission_mode = PermissionMode::DontAsk;
    config.timeout = Duration::from_secs(10);
    config.vendor_login = None;
    config
        .env
        .insert("ANTHROPIC_API_KEY".into(), Secret::new("sk-ant-replay"));
    config.env.insert(
        "FAKE_CLAUDE_FIXTURE".into(),
        Secret::new(golden("claude", scenario).display().to_string()),
    );
    config
}

async fn claude_run(
    config: ClaudeConfig,
) -> (
    Result<rigel_adapters::ClaudeTurn, rigel_adapters::AdapterError>,
    Vec<ClaudeEvent>,
) {
    let mut events = Vec::new();
    let outcome = ClaudeAdapter::new(config)
        .run_turn("replay", |event| events.push(event))
        .await;
    (outcome, events)
}

#[tokio::test]
async fn claude_fixtures_replay_through_the_adapter() {
    for scenario in SCENARIOS {
        let dir = tempfile::tempdir().unwrap();
        let (outcome, events) = claude_run(claude_config(dir.path(), scenario)).await;

        match scenario {
            "start" => {
                let turn = outcome.expect("a finished turn");
                assert_eq!(turn.text, "The file is small.", "{scenario}");
                assert_eq!(turn.session_id.as_deref(), Some("cs-9c1"), "{scenario}");
                let usage = turn.usage.expect("usage from the result line");
                assert_eq!(usage.input_tokens, 1207, "{scenario}");
                assert_eq!(usage.cached_input_tokens, 120, "{scenario}");
                assert_eq!(usage.output_tokens, 5, "{scenario}");
                assert_eq!(usage.cost_micros, Some(6160), "{scenario}");
                assert_eq!(
                    events,
                    vec![
                        ClaudeEvent::Started {
                            session_id: "cs-9c1".into(),
                            model: Some("claude-sonnet-4.5".into()),
                        },
                        ClaudeEvent::Delta {
                            text: "The file ".into()
                        },
                        ClaudeEvent::Delta {
                            text: "is small.".into()
                        },
                        ClaudeEvent::Message {
                            text: "The file is small.".into()
                        },
                    ],
                    "{scenario}"
                );
            }
            "tool_call" => {
                // P4.8: the `tool_use` block is a `file_change` item, and the `tool_result` that
                // comes back on the `user` line inherits that kind from the call that produced it.
                let turn = outcome.expect("a finished turn");
                assert_eq!(turn.text, "Reading src/lib.rs.", "{scenario}");
                assert_eq!(
                    events[..3],
                    [
                        ClaudeEvent::Started {
                            session_id: "cs-9c2".into(),
                            model: Some("claude-sonnet-4.5".into()),
                        },
                        ClaudeEvent::Delta {
                            text: "Reading src/lib.rs.".into()
                        },
                        ClaudeEvent::Message {
                            text: "Reading src/lib.rs.".into()
                        },
                    ],
                    "{scenario}"
                );
                let ClaudeEvent::Item(call) = &events[3] else {
                    panic!("{scenario}: expected a tool-call item, got {events:?}");
                };
                assert_eq!(call.kind, ItemKind::FileChange, "{scenario}");
                assert_eq!(call.body["path"], "src/lib.rs", "{scenario}");
                assert_eq!(call.body["operation"], "read", "{scenario}");
                assert_eq!(call.body["tool"], "Read", "{scenario}");
                assert_eq!(call.body["tool_use_id"], "toolu_01", "{scenario}");

                let ClaudeEvent::Item(result) = &events[4] else {
                    panic!("{scenario}: expected a tool-result item, got {events:?}");
                };
                assert_eq!(result.kind, ItemKind::FileChange, "{scenario}");
                assert_eq!(result.body["tool_use_id"], "toolu_01", "{scenario}");
                assert_eq!(result.body["ok"], true, "{scenario}");
                assert_eq!(result.body["result"], "fn main() {}\n", "{scenario}");
                assert_eq!(events.len(), 5, "{scenario}: nothing else was invented");
            }
            "approval" => {
                // Without `--permission-prompt-tool` (P4.6's recorded deviation) the CLI's own
                // policy declines, and the refusal reaches Rigel as a failed result.
                let error = outcome.expect_err("a declined permission is not a turn");
                let text = error.to_string();
                assert!(
                    text.contains("requested permissions to use Bash"),
                    "{scenario}: {text}"
                );
            }
            "error" => {
                let error = outcome.expect_err("a failed result is not a turn");
                assert!(
                    error.to_string().contains("maximum number of turns"),
                    "{scenario}: {error}"
                );
            }
            "interrupt" => {
                // A killed child leaves a stream with no `result` line.
                let error = outcome.expect_err("a truncated stream is not a turn");
                assert!(
                    error.to_string().contains("ended without a result line"),
                    "{scenario}: {error}"
                );
            }
            other => panic!("unhandled Claude scenario {other}"),
        }
    }
}

/* ---------------------------------- Codex ---------------------------------- */

fn codex_config(cwd: &Path, scenario: &str) -> CodexConfig {
    let mut config = CodexConfig::router(cwd);
    config.exe = PathBuf::from(env!("CARGO_BIN_EXE_fake-codex"));
    config.transport = Transport::AppServer;
    config.timeout = Duration::from_secs(10);
    config.vendor_login = None;
    config
        .env
        .insert("OPENAI_API_KEY".into(), Secret::new("sk-replay-router"));
    config.env.insert(
        "FAKE_CODEX_FIXTURE".into(),
        Secret::new(golden("codex", scenario).display().to_string()),
    );
    config
}

async fn codex_run(
    config: CodexConfig,
) -> (
    Result<rigel_adapters::TurnResult, rigel_adapters::AdapterError>,
    Vec<CodexEvent>,
) {
    let mut events = Vec::new();
    let outcome = CodexAdapter::new(config)
        .run_turn("replay", |event| events.push(event))
        .await;
    (outcome, events)
}

#[tokio::test]
async fn codex_fixtures_replay_through_the_adapter() {
    for scenario in SCENARIOS {
        let dir = tempfile::tempdir().unwrap();
        let (outcome, events) = codex_run(codex_config(dir.path(), scenario)).await;

        match scenario {
            "start" => {
                let turn = outcome.expect("a finished turn");
                assert_eq!(turn.text, "The build is green.", "{scenario}");
                assert_eq!(turn.thread_id.as_deref(), Some("th-1"), "{scenario}");
                let usage = turn.usage.expect("usage from the completed turn");
                assert_eq!(usage.input_tokens, 1500, "{scenario}");
                assert_eq!(usage.cached_input_tokens, 100, "{scenario}");
                assert_eq!(usage.output_tokens, 6, "{scenario}");
                assert_eq!(
                    events,
                    vec![
                        CodexEvent::Started {
                            thread_id: "th-1".into(),
                            transport: Transport::AppServer,
                        },
                        CodexEvent::Delta {
                            text: "The build ".into()
                        },
                        CodexEvent::Delta {
                            text: "is green.".into()
                        },
                        CodexEvent::Message {
                            text: "The build is green.".into()
                        },
                    ],
                    "{scenario}"
                );
            }
            "tool_call" => {
                // P4.8: `item/completed` for a `commandExecution` is a `command` item. The
                // `item/started` line before it is not decoded, so one command makes one row.
                let turn = outcome.expect("a finished turn");
                assert_eq!(turn.text, "All tests passed.", "{scenario}");
                assert_eq!(
                    events[..1],
                    [CodexEvent::Started {
                        thread_id: "th-1".into(),
                        transport: Transport::AppServer,
                    }],
                    "{scenario}"
                );
                let CodexEvent::Item(command) = &events[1] else {
                    panic!("{scenario}: expected a command item, got {events:?}");
                };
                assert_eq!(command.kind, ItemKind::Command, "{scenario}");
                assert_eq!(command.body["command"], "cargo test --quiet", "{scenario}");
                assert_eq!(command.body["exit_code"], 0, "{scenario}");
                assert_eq!(command.body["status"], "completed", "{scenario}");
                assert_eq!(
                    events[2],
                    CodexEvent::Message {
                        text: "All tests passed.".into()
                    },
                    "{scenario}"
                );
                assert_eq!(events.len(), 3, "{scenario}: nothing else was invented");
            }
            "approval" => {
                // P4.10: the server-initiated request is answered in place with a scope. With no
                // approver installed the answer is a refusal, which is recorded as an Approval
                // event; no item may claim the command ran.
                let turn = outcome.expect("a finished turn");
                assert_eq!(turn.text, "Waiting for approval.", "{scenario}");
                let asked: Vec<_> = events
                    .iter()
                    .filter_map(|event| match event {
                        CodexEvent::Approval { request, decision } => {
                            Some((request.clone(), *decision))
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(asked.len(), 1, "{scenario}: one question, one answer");
                assert_eq!(
                    asked[0].0.title, "git push --force origin main",
                    "{scenario}"
                );
                assert_eq!(asked[0].0.tool_call_id, "c2", "{scenario}");
                assert_eq!(asked[0].1, ApprovalDecision::RejectOnce, "{scenario}");
                assert!(
                    !events
                        .iter()
                        .any(|event| matches!(event, CodexEvent::Item(_))),
                    "{scenario}: nothing ran, so there is no command item"
                );
            }
            "error" => {
                let error = outcome.expect_err("a failed turn is not a turn");
                assert!(
                    error.to_string().contains("model overloaded"),
                    "{scenario}: {error}"
                );
            }
            "interrupt" => {
                // app-server closed its stdout before `turn/completed`: the killed-child shape.
                let error = outcome.expect_err("a closed stream is not a turn");
                assert!(
                    error.to_string().contains("closed its stdout"),
                    "{scenario}: {error}"
                );
            }
            other => panic!("unhandled Codex scenario {other}"),
        }
    }
}

/* ------------------------------------ ACP ---------------------------------- */

struct AllowOnce {
    seen: Mutex<Vec<ApprovalRequest>>,
}

#[async_trait]
impl Approver for AllowOnce {
    async fn approve(&self, request: &ApprovalRequest) -> ApprovalDecision {
        self.seen.lock().unwrap().push(request.clone());
        ApprovalDecision::AllowOnce
    }
}

fn acp_config(cwd: &Path, scenario: &str) -> AcpConfig {
    let mut config = AcpConfig::cursor(cwd);
    config.exe = PathBuf::from(env!("CARGO_BIN_EXE_fake-acp"));
    config.timeout = Duration::from_secs(10);
    config.vendor_login = None;
    config
        .env
        .insert("CURSOR_API_KEY".into(), Secret::new("ck-replay"));
    config.env.insert(
        "FAKE_ACP_RECORD".into(),
        Secret::new(cwd.join("record.txt").display().to_string()),
    );
    config.env.insert(
        "FAKE_ACP_FIXTURE".into(),
        Secret::new(golden("acp", scenario).display().to_string()),
    );
    config
}

#[tokio::test]
async fn acp_fixtures_replay_through_the_client() {
    for scenario in SCENARIOS {
        let dir = tempfile::tempdir().unwrap();
        let approver = Arc::new(AllowOnce {
            seen: Mutex::new(Vec::new()),
        });
        let mut client = AcpClient::spawn(acp_config(dir.path(), scenario), approver.clone())
            .await
            .expect("the replay agent initializes");
        client.new_session().await.expect("session/new");

        let mut events = Vec::new();
        let outcome = client.prompt("replay", |event| events.push(event)).await;
        let recorded = std::fs::read_to_string(dir.path().join("record.txt")).unwrap_or_default();

        match scenario {
            "start" => {
                let turn = outcome.expect("a finished turn");
                assert_eq!(turn.text, "Reading the file.", "{scenario}");
                assert_eq!(turn.stop_reason, "end_turn", "{scenario}");
                let usage = turn.usage.expect("usage_update");
                assert_eq!(usage.used, 5300, "{scenario}");
                assert_eq!(usage.size, 200_000, "{scenario}");
                assert_eq!(usage.cost_micros, Some(4500), "{scenario}");
                assert_eq!(
                    events,
                    vec![
                        AcpEvent::MessageDelta {
                            text: "Reading ".into()
                        },
                        AcpEvent::MessageDelta {
                            text: "the file.".into()
                        },
                        AcpEvent::Usage {
                            used: 5300,
                            size: 200_000,
                            cost_micros: Some(4500)
                        },
                    ],
                    "{scenario}"
                );
            }
            "tool_call" => {
                let turn = outcome.expect("a finished turn");
                assert_eq!(turn.text, "All green.", "{scenario}");
                assert_eq!(
                    events,
                    vec![
                        AcpEvent::ToolCall {
                            tool_call_id: "call-3".into(),
                            title: "Run the test suite".into(),
                            kind: Some("execute".into()),
                            item_kind: ItemKind::Command,
                            operation: "execute",
                            status: Some("pending".into()),
                            raw_input: Some(serde_json::json!({ "command": "cargo test" })),
                        },
                        AcpEvent::ToolCallUpdate {
                            tool_call_id: "call-3".into(),
                            status: Some("completed".into()),
                            text: Some("ok. 102 passed".into()),
                        },
                        AcpEvent::MessageDelta {
                            text: "All green.".into()
                        },
                    ],
                    "{scenario}"
                );
            }
            "approval" => {
                let turn = outcome.expect("a finished turn");
                assert_eq!(
                    turn.text, "I need to delete the build directory.",
                    "{scenario}"
                );

                let seen = approver.seen.lock().unwrap().clone();
                assert_eq!(seen.len(), 1, "{scenario}: one permission request");
                assert_eq!(seen[0].tool_call_id, "call-4", "{scenario}");
                assert_eq!(seen[0].title, "Delete build", "{scenario}");
                assert_eq!(seen[0].kind.as_deref(), Some("delete"), "{scenario}");
                assert!(
                    seen[0]
                        .options
                        .iter()
                        .any(|option| option.kind == "allow_once"),
                    "{scenario}: the agent offered an allow-once option"
                );
                // The decision went back on the wire as the option the agent offered.
                let outcome = recorded
                    .lines()
                    .find_map(|line| line.strip_prefix("outcome="))
                    .unwrap_or_else(|| panic!("{scenario}: no outcome was recorded: {recorded}"));
                let outcome: serde_json::Value =
                    serde_json::from_str(outcome).expect("the outcome is JSON");
                assert_eq!(outcome["outcome"], "selected", "{scenario}");
                assert_eq!(outcome["optionId"], "allow-once", "{scenario}");
                assert!(
                    events.contains(&AcpEvent::ToolCallUpdate {
                        tool_call_id: "call-4".into(),
                        status: Some("completed".into()),
                        text: Some("removed 42 files".into()),
                    }),
                    "{scenario}: {events:?}"
                );
            }
            "error" => {
                let error = outcome.expect_err("a JSON-RPC error is not a turn");
                assert!(
                    error
                        .to_string()
                        .contains("upstream model refused the request"),
                    "{scenario}: {error}"
                );
            }
            "interrupt" => {
                // The agent acknowledged the cancel; that is a turn that stopped, not a failure.
                let turn = outcome.expect("a cancelled turn still answers");
                assert_eq!(turn.stop_reason, "cancelled", "{scenario}");
                assert_eq!(turn.text, "working", "{scenario}");
            }
            other => panic!("unhandled ACP scenario {other}"),
        }
    }
}

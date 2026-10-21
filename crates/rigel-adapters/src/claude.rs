//! The Claude Code adapter (P4.6).
//!
//! Claude Code is the second external harness. Its headless surface is NDJSON on stdout:
//!
//! ```text
//! claude -p --bare --output-format stream-json --verbose --include-partial-messages "<prompt>"
//! ```
//!
//! `--bare` is what makes D21 enforceable rather than aspirational: the CLI then authenticates
//! strictly with `ANTHROPIC_API_KEY` (or an `apiKeyHelper`), and never reads the OAuth login or
//! the OS keychain. The adapter still injects the key itself, from the per-agent credential, and
//! refuses to spawn when no key exists. A child that somehow reports a different
//! `apiKeySource` is killed at its first event.
//!
//! Approvals are *not* part of this adapter yet: P4.10 wires the permission tool. Until then a
//! tool that needs permission fails closed inside Claude Code, which is the safe default.
//!
//! Events observed against Claude Code 2.1.251 on 21 Oct 2026:
//! `system/init` (session id, model, `apiKeySource`), `stream_event` wrapping Anthropic SSE
//! (`content_block_delta` with `text_delta`), a final `assistant` message, and `result` with
//! `usage` and `total_cost_usd`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use rigel_core::credentials::{CredentialStore, Secret, get_from};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{ChildStdout, Command};
use tokio::time::{Instant, timeout_at};

use crate::{Adapter, AdapterError, AuthMode, Harness, HarnessInfo, credential_spec};

/// The API-key header is the only accepted source; `--bare` guarantees the others are absent.
const ACCEPTED_KEY_SOURCE: &str = "ANTHROPIC_API_KEY";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// Ask through the CLI's own mechanism. Without a permission tool (P4.10) this fails closed.
    Default,
    Plan,
    AcceptEdits,
    DontAsk,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Plan => "plan",
            Self::AcceptEdits => "acceptEdits",
            Self::DontAsk => "dontAsk",
        }
    }
}

#[derive(Clone)]
pub struct ClaudeConfig {
    /// The `claude` binary. Resolved on `PATH` when left as a bare name.
    pub exe: PathBuf,
    pub model: Option<String>,
    pub cwd: PathBuf,
    pub permission_mode: PermissionMode,
    pub timeout: Duration,
    /// Per-agent process environment. Values stay wrapped so debug output is always redacted.
    pub env: BTreeMap<String, Secret>,
    /// Existence means a vendor login exists. Rigel never reads it, and `--bare` never uses it.
    pub vendor_login: Option<PathBuf>,
}

impl fmt::Debug for ClaudeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClaudeConfig")
            .field("exe", &self.exe)
            .field("model", &self.model)
            .field("cwd", &self.cwd)
            .field("permission_mode", &self.permission_mode)
            .field("timeout", &self.timeout)
            .field("env", &self.env)
            .field("vendor_login", &self.vendor_login)
            .finish()
    }
}

impl ClaudeConfig {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            exe: PathBuf::from(if cfg!(windows) {
                "claude.exe"
            } else {
                "claude"
            }),
            model: None,
            cwd: cwd.into(),
            permission_mode: PermissionMode::Default,
            timeout: Duration::from_secs(180),
            env: BTreeMap::new(),
            vendor_login: default_vendor_login(),
        }
    }

    /// Resolve the harness credential into this process environment. Missing storage leaves the
    /// map empty, so `probe()` reports `None` or `Subscription` and a turn refuses before spawn.
    pub fn inject_credential(&mut self, store: &dyn CredentialStore) -> bool {
        let Some(spec) = credential_spec(Harness::Claude, None) else {
            return false;
        };
        self.env.remove(spec.env_var);
        match get_from(store, spec.provider) {
            Ok(secret) => {
                self.env.insert(spec.env_var.to_string(), secret);
                true
            }
            Err(_) => false,
        }
    }

    pub fn credential_provider(&self) -> &'static str {
        credential_spec(Harness::Claude, None)
            .expect("Claude always has a credential specification")
            .provider
    }
}

/// Normalised progress from one Claude turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeEvent {
    Started {
        session_id: String,
        model: Option<String>,
    },
    Delta {
        text: String,
    },
    Message {
        text: String,
    },
    /// A warning or status line the harness emitted. Not fatal.
    Notice {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    /// `total_cost_usd` from the CLI, in micro-dollars. The local ledger stays authoritative (D29).
    pub cost_micros: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ClaudeTurn {
    pub text: String,
    pub usage: Option<ClaudeUsage>,
    pub session_id: Option<String>,
    pub duration: Duration,
}

#[derive(Debug, Clone)]
pub struct ClaudeAdapter {
    config: ClaudeConfig,
}

impl ClaudeAdapter {
    pub fn new(config: ClaudeConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &ClaudeConfig {
        &self.config
    }

    /// Run one prompt to completion, reporting deltas as they arrive.
    pub async fn run_turn<F>(
        &self,
        prompt: &str,
        mut on_event: F,
    ) -> Result<ClaudeTurn, AdapterError>
    where
        F: FnMut(ClaudeEvent),
    {
        if auth_mode(&self.config) != AuthMode::ApiKey {
            return Err(AdapterError::RequiresApiKey {
                harness: Harness::Claude,
            });
        }
        let secrets = self.config.env.values().cloned().collect::<Vec<_>>();
        let started = Instant::now();

        let mut command = self.command();
        command
            .args([
                "-p",
                "--bare",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
            ])
            .args(["--permission-mode", self.config.permission_mode.as_str()]);
        if let Some(model) = &self.config.model {
            command.args(["--model", model]);
        }
        command.arg(prompt);

        let mut child = command
            .spawn()
            .map_err(|error| AdapterError::Other(format!("claude: could not start: {error}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AdapterError::Other("claude: no stdout".to_string()))?;
        let mut lines = BufReader::new(stdout).lines();
        let deadline = Instant::now() + self.config.timeout;

        let mut text = String::new();
        let mut session_id = None;
        let mut usage = None;
        let mut emitted = false;
        let mut finished = false;

        loop {
            let line = next_line(&mut lines, deadline, emitted).await?;
            let Some(line) = line else {
                break;
            };
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };

            match classify(&value) {
                Step::Init {
                    session_id: id,
                    model,
                    api_key_source,
                } => {
                    // Only the documented source is acceptable. Absent counts as unacceptable:
                    // with `--bare` the CLI always reports one, and a silent absence would be
                    // indistinguishable from a subscription turn (D21).
                    if api_key_source.as_deref() != Some(ACCEPTED_KEY_SOURCE) {
                        let _ = child.kill().await;
                        let _ = wait_bounded(&mut child).await;
                        return Err(AdapterError::RequiresApiKey {
                            harness: Harness::Claude,
                        });
                    }
                    if !id.is_empty() {
                        session_id = Some(crate::redact(&id, &secrets));
                        on_event(ClaudeEvent::Started {
                            session_id: crate::redact(&id, &secrets),
                            model,
                        });
                    } else {
                        on_event(ClaudeEvent::Started {
                            session_id: String::new(),
                            model,
                        });
                    }
                }
                Step::Delta(delta) => {
                    emitted = true;
                    text.push_str(&delta);
                    on_event(ClaudeEvent::Delta {
                        text: crate::redact(&delta, &secrets),
                    });
                }
                Step::Message(message) => {
                    emitted = true;
                    text = message.clone();
                    on_event(ClaudeEvent::Message {
                        text: crate::redact(&message, &secrets),
                    });
                }
                Step::Notice(note) => on_event(ClaudeEvent::Notice {
                    message: crate::redact(&note, &secrets),
                }),
                Step::Result {
                    text: result,
                    is_error,
                    subtype,
                    usage: reported,
                    session_id: id,
                } => {
                    finished = true;
                    usage = reported;
                    if id.is_some() {
                        session_id = id.map(|id| crate::redact(&id, &secrets));
                    }
                    if is_error || subtype != "success" {
                        let _ = wait_bounded(&mut child).await;
                        let reason = result
                            .or_else(|| (!subtype.is_empty()).then(|| subtype.clone()))
                            .unwrap_or_else(|| "claude reported an error".to_string());
                        return Err(AdapterError::Other(format!(
                            "claude: {}",
                            crate::redact(&reason, &secrets)
                        )));
                    }
                    if let Some(result) = result
                        && text.is_empty()
                    {
                        text = result;
                        on_event(ClaudeEvent::Message {
                            text: crate::redact(&text, &secrets),
                        });
                    }
                }
                Step::Ignore => {}
            }
        }

        // A child that wrote its whole answer and then failed to exit is reaped anyway; without a
        // bound, one wedged process would hang the turn forever.
        let status = wait_bounded(&mut child).await;
        let status_text = status
            .map(|status| status.to_string())
            .unwrap_or_else(|| "no exit status".to_string());

        if !finished {
            // A stream that stops before its `result` line is a partial answer even when the exit
            // code is 0 (an interrupted or truncated child). Reporting it as a turn would bill a
            // half-written message as success.
            return Err(AdapterError::Other(format!(
                "claude ended without a result line (status {status_text})"
            )));
        }
        if !status.is_some_and(|status| status.success()) {
            return Err(AdapterError::Other(format!(
                "claude exited with {status_text}"
            )));
        }
        if text.trim().is_empty() {
            return Err(AdapterError::Other(format!(
                "claude produced no message (status {status_text})"
            )));
        }

        Ok(ClaudeTurn {
            text: crate::redact(&text, &secrets),
            usage,
            session_id,
            duration: started.elapsed(),
        })
    }

    fn command(&self) -> Command {
        let exe = crate::registry::resolve_exe(Harness::Claude, &self.config.exe);
        let mut command = crate::registry::command_for(&exe);
        command
            .current_dir(&self.config.cwd)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            // Inherited: nothing reads stderr, and an unread pipe can deadlock a chatty child.
            .stderr(Stdio::inherit());
        // An ambient vendor key must not bypass the per-agent resolver.
        command
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("OPENAI_API_KEY")
            .env_remove("CURSOR_API_KEY")
            .envs(
                self.config
                    .env
                    .iter()
                    .map(|(name, value)| (name, value.expose())),
            );
        command
    }
}

#[async_trait::async_trait]
impl Adapter for ClaudeAdapter {
    fn harness(&self) -> Harness {
        Harness::Claude
    }

    async fn probe(&self) -> Result<HarnessInfo, AdapterError> {
        Ok(crate::registry::probe_claude(&self.config).await)
    }
}

/// How Claude proves who it is. A key in the injected environment is the only accepted mode;
/// a vendor login file is checked for existence only and classed as `Subscription`.
pub(crate) fn auth_mode(config: &ClaudeConfig) -> AuthMode {
    let has_key = credential_spec(Harness::Claude, None).is_some_and(|spec| {
        config
            .env
            .get(spec.env_var)
            .is_some_and(|value| !value.expose().trim().is_empty())
    });
    if has_key {
        AuthMode::ApiKey
    } else if config
        .vendor_login
        .as_ref()
        .is_some_and(|path| path.is_file())
    {
        AuthMode::Subscription
    } else {
        AuthMode::None
    }
}

fn default_vendor_login() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .map(|home| home.join(".claude").join(".credentials.json"))
}

/// Wait for the child, but never indefinitely: 10 s after the stream ended, kill and reap it.
async fn wait_bounded(child: &mut tokio::process::Child) -> Option<ExitStatus> {
    match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
        Ok(Ok(status)) => Some(status),
        Ok(Err(_)) => None,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            None
        }
    }
}

async fn next_line(
    lines: &mut Lines<BufReader<ChildStdout>>,
    deadline: Instant,
    emitted: bool,
) -> Result<Option<String>, AdapterError> {
    match timeout_at(deadline, lines.next_line()).await {
        Ok(Ok(line)) => Ok(line),
        Ok(Err(error)) => Err(AdapterError::Other(format!(
            "claude: could not read output: {error}"
        ))),
        Err(_) if !emitted => Err(AdapterError::Other(
            "claude: timed out before answering".to_string(),
        )),
        Err(_) => Err(AdapterError::Other(
            "claude: timed out mid-turn".to_string(),
        )),
    }
}

/// One normalised step from a stream-json line. Kept pure so fixtures can pin the shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Init {
        session_id: String,
        model: Option<String>,
        api_key_source: Option<String>,
    },
    Delta(String),
    Message(String),
    Notice(String),
    Result {
        text: Option<String>,
        is_error: bool,
        subtype: String,
        usage: Option<ClaudeUsage>,
        session_id: Option<String>,
    },
    Ignore,
}

fn classify(value: &Value) -> Step {
    match value.get("type").and_then(Value::as_str) {
        Some("system") => match value.get("subtype").and_then(Value::as_str) {
            // Even a malformed init still carries the auth source, so it is classified rather
            // than dropped: a missing field must not become a licence to use the vendor login.
            Some("init") => Step::Init {
                session_id: value
                    .get("session_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                model: value
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                api_key_source: value
                    .get("apiKeySource")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            },
            Some("status") => {
                let message = value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        format!(
                            "status {}",
                            value
                                .get("status")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown")
                        )
                    });
                Step::Notice(message)
            }
            _ => Step::Ignore,
        },
        Some("stream_event") => {
            let event = &value["event"];
            let kind = event.get("type").and_then(Value::as_str);
            if kind == Some("content_block_delta") {
                let delta = &event["delta"];
                if delta.get("type").and_then(Value::as_str) == Some("text_delta") {
                    return delta
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|text| Step::Delta(text.to_string()))
                        .unwrap_or(Step::Ignore);
                }
            }
            Step::Ignore
        }
        Some("assistant") => {
            let blocks = value["message"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let text = blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            if text.is_empty() {
                Step::Ignore
            } else {
                Step::Message(text)
            }
        }
        Some("result") => Step::Result {
            text: value
                .get("result")
                .and_then(Value::as_str)
                .map(str::to_string),
            is_error: value
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            subtype: value
                .get("subtype")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            usage: usage_of(value),
            session_id: value
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        _ => Step::Ignore,
    }
}

fn usage_of(value: &Value) -> Option<ClaudeUsage> {
    let usage = value.get("usage").filter(|usage| usage.is_object())?;
    let number = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let cost_micros = value
        .get("total_cost_usd")
        .and_then(Value::as_f64)
        .map(|usd| (usd * 1_000_000.0).round() as i64);
    Some(ClaudeUsage {
        input_tokens: number("input_tokens"),
        cached_input_tokens: number("cache_read_input_tokens"),
        output_tokens: number("output_tokens"),
        cost_micros,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_stream_shapes_from_a_real_turn_are_understood() {
        let init = json!({
            "type": "system",
            "subtype": "init",
            "session_id": "s-1",
            "model": "free-stack",
            "apiKeySource": "ANTHROPIC_API_KEY"
        });
        assert_eq!(
            classify(&init),
            Step::Init {
                session_id: "s-1".to_string(),
                model: Some("free-stack".to_string()),
                api_key_source: Some("ANTHROPIC_API_KEY".to_string()),
            }
        );

        let delta = json!({
            "type": "stream_event",
            "event": {
                "type": "content_block_delta",
                "index": 0,
                "delta": { "type": "text_delta", "text": "pong" }
            }
        });
        assert_eq!(classify(&delta), Step::Delta("pong".to_string()));

        let assistant = json!({
            "type": "assistant",
            "message": { "content": [{ "type": "text", "text": "pong" }] }
        });
        assert_eq!(classify(&assistant), Step::Message("pong".to_string()));

        let result = json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": "pong",
            "session_id": "s-1",
            "total_cost_usd": 0.00616,
            "usage": {
                "input_tokens": 1207,
                "cache_read_input_tokens": 0,
                "output_tokens": 5
            }
        });
        assert_eq!(
            classify(&result),
            Step::Result {
                text: Some("pong".to_string()),
                is_error: false,
                subtype: "success".to_string(),
                usage: Some(ClaudeUsage {
                    input_tokens: 1207,
                    cached_input_tokens: 0,
                    output_tokens: 5,
                    cost_micros: Some(6160),
                }),
                session_id: Some("s-1".to_string()),
            }
        );
    }

    #[test]
    fn a_blocked_turn_is_a_result_error() {
        let result = json!({
            "type": "result",
            "subtype": "error_during_execution",
            "is_error": true,
            "result": "tool permission denied"
        });
        let Step::Result {
            is_error, subtype, ..
        } = classify(&result)
        else {
            panic!("expected a result step");
        };
        assert!(is_error);
        assert_eq!(subtype, "error_during_execution");
    }

    #[test]
    fn the_config_debug_is_redacted() {
        let mut config = ClaudeConfig::new(".");
        config.vendor_login = None;
        config.env.insert(
            "ANTHROPIC_API_KEY".to_string(),
            Secret::new("sk-ant-must-not-print"),
        );
        let debug = format!("{config:?}");
        assert!(debug.contains("ANTHROPIC_API_KEY"));
        assert!(!debug.contains("sk-ant-must-not-print"));
    }

    #[test]
    fn only_an_injected_key_counts_as_api_key_auth() {
        let mut config = ClaudeConfig::new(".");
        config.vendor_login = None;
        assert_eq!(auth_mode(&config), AuthMode::None);

        config
            .env
            .insert("ANTHROPIC_API_KEY".to_string(), Secret::new("sk-ant-test"));
        assert_eq!(auth_mode(&config), AuthMode::ApiKey);
    }
}

//! The Codex adapter (D36, P4.5).
//!
//! Codex is the first external harness Rigel drives. Two transports, one trait:
//!
//! | transport | command | why |
//! |---|---|---|
//! | `app-server` | `codex app-server` (JSON-RPC over stdio) | the primary path: streaming deltas, approvals, thread ids |
//! | `exec` | `codex exec --json` (JSONL) | the tested fallback for a version whose app-server drifts |
//!
//! Both were probed against a local OmniRoute router on 21 Oct 2026 (`scripts/codex-router-probe.mjs`).
//! The router's own launcher (`omniroute run codex`) is broken on Windows — it passes `^^^"model_provider…`
//! as argv — so this adapter writes the `-c` overrides itself and never shells out to it.
//!
//! **Auto never double-bills.** If app-server fails *before* the first assistant token, the turn falls
//! back to `exec`; if it fails after, the error is returned and the partial text is kept. Re-running a
//! turn that already reached the model would charge twice for one prompt.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout, Command};
use tokio::time::{Instant, timeout_at};

use crate::{
    Adapter, AdapterError, ApprovalPolicy, AuthMode, Capabilities, Harness, HarnessInfo,
    SandboxMode,
};

/// The model Rigel defaults to. The `-medium` suffix is OmniRoute's effort convention (Q74).
pub const DEFAULT_MODEL: &str = "gpt-5.6-sol-medium";
/// The local router's OpenAI-compatible surface.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:20128/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// Try app-server, fall back to exec only when nothing was generated.
    Auto,
    AppServer,
    Exec,
}

impl Transport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::AppServer => "app-server",
            Self::Exec => "exec",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CodexConfig {
    /// The `codex` binary. Resolved from `PATH` when left as the default.
    pub exe: PathBuf,
    /// Provider base URL. Empty means "let Codex use its own configuration".
    pub base_url: String,
    pub model: String,
    pub cwd: PathBuf,
    pub sandbox: SandboxMode,
    pub approval: ApprovalPolicy,
    pub transport: Transport,
    /// One turn, start to finish. The router can be slow on the first turn of the day.
    pub timeout: Duration,
}

impl CodexConfig {
    /// The credential this config authenticates with. The local OmniRoute router on its own port
    /// (`20128`) means Rigel's router key; anything else — including another loopback service —
    /// means a direct OpenAI key. Both reach Codex through `OPENAI_API_KEY`, which is the variable
    /// Codex accepts for an OpenAI-compatible endpoint.
    pub fn provider(&self) -> &'static str {
        if is_local_omniroute(&self.base_url) {
            rigel_core::credentials::OMNIROUTE
        } else {
            rigel_core::credentials::OPENAI
        }
    }

    /// The normal case: a project, the local router, the default model.
    pub fn router(cwd: impl Into<PathBuf>) -> Self {
        Self {
            exe: PathBuf::from(if cfg!(windows) { "codex.exe" } else { "codex" }),
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            cwd: cwd.into(),
            sandbox: SandboxMode::ReadOnly,
            approval: ApprovalPolicy::Never,
            transport: Transport::Auto,
            timeout: Duration::from_secs(180),
        }
    }
}

/// Normalised progress from one Codex turn. The item pipeline maps these onto `ItemKind`;
/// nothing here knows about sessions or the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexEvent {
    Started {
        thread_id: String,
        transport: Transport,
    },
    Delta {
        text: String,
    },
    Message {
        text: String,
    },
    /// A warning the harness emitted (config drift, MCP auth). Not fatal.
    Notice {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub reasoning_output_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct TurnResult {
    pub text: String,
    pub usage: Option<CodexUsage>,
    pub transport: Transport,
    pub thread_id: Option<String>,
    pub duration: Duration,
    /// True when app-server failed before output and `exec` answered instead.
    pub fell_back: bool,
}

#[derive(Debug, Clone)]
pub struct CodexAdapter {
    config: CodexConfig,
}

impl CodexAdapter {
    pub fn new(config: CodexConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &CodexConfig {
        &self.config
    }

    /// Run one prompt to completion, reporting deltas as they arrive.
    pub async fn run_turn<F>(
        &self,
        prompt: &str,
        mut on_event: F,
    ) -> Result<TurnResult, AdapterError>
    where
        F: FnMut(CodexEvent),
    {
        let started = Instant::now();
        let outcome = match self.config.transport {
            Transport::AppServer => self.app_server_turn(prompt, &mut on_event).await,
            Transport::Exec => self.exec_turn(prompt, &mut on_event).await,
            Transport::Auto => match self.app_server_turn(prompt, &mut on_event).await {
                Ok(result) => Ok(result),
                Err(Failure::BeforeTurn(reason)) => {
                    on_event(CodexEvent::Notice {
                        message: format!("app-server failed ({reason}); falling back to exec"),
                    });
                    self.exec_turn(prompt, &mut on_event)
                        .await
                        .map(|mut result| {
                            result.fell_back = true;
                            result
                        })
                }
                Err(error) => Err(error),
            },
        };

        outcome
            .map(|mut result| {
                result.duration = started.elapsed();
                result
            })
            .map_err(AdapterError::from)
    }

    /// The `-c` overrides for pointing Codex at a router. Written here, never taken from
    /// `omniroute launch-codex` (see the module docs).
    fn overrides(&self) -> Vec<String> {
        if self.config.base_url.trim().is_empty() {
            return Vec::new();
        }
        vec![
            "model_provider=\"omniroute\"".to_string(),
            "model_providers.omniroute.name=\"OmniRoute\"".to_string(),
            format!(
                "model_providers.omniroute.base_url=\"{}\"",
                self.config.base_url
            ),
            "model_providers.omniroute.wire_api=\"responses\"".to_string(),
            "model_providers.omniroute.requires_openai_auth=false".to_string(),
        ]
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.config.exe);
        command
            .current_dir(&self.config.cwd)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited, not piped: nothing reads stderr, and an unread pipe can fill and
            // deadlock a chatty child. Codex's log lines belong in the process's own stderr.
            .stderr(Stdio::inherit());
        command
    }

    async fn app_server_turn<F>(
        &self,
        prompt: &str,
        on_event: &mut F,
    ) -> Result<TurnResult, Failure>
    where
        F: FnMut(CodexEvent),
    {
        let mut command = self.command();
        command.arg("app-server");
        for pair in self.overrides() {
            command.arg("-c").arg(pair);
        }

        let mut child = command
            .spawn()
            .map_err(|error| Failure::BeforeTurn(format!("could not start codex: {error}")))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| Failure::BeforeTurn("codex has no stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Failure::BeforeTurn("codex has no stdout".to_string()))?;
        let mut lines = BufReader::new(stdout).lines();
        let deadline = Instant::now() + self.config.timeout;

        let mut text = String::new();
        let mut thread_id: Option<String> = None;
        let mut usage: Option<CodexUsage> = None;
        let mut emitted = false;

        send(
            &mut stdin,
            1,
            "initialize",
            json!({ "clientInfo": { "name": "rigel", "version": env!("CARGO_PKG_VERSION") } }),
        )
        .await?;

        loop {
            let line = next_line(&mut lines, deadline, emitted).await?;
            let Some(line) = line else {
                // Exiting before the first token means the turn never reached the model, so
                // another transport may safely take the same prompt.
                return Err(if emitted {
                    Failure::AfterTurn("codex app-server closed its stdout".to_string())
                } else {
                    Failure::BeforeTurn("codex app-server exited before answering".to_string())
                });
            };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };

            if let Some(id) = message.get("id").and_then(Value::as_u64) {
                if let Some(error) = message.get("error") {
                    return Err(if emitted {
                        Failure::AfterTurn(format!("JSON-RPC error: {error}"))
                    } else {
                        Failure::BeforeTurn(format!("JSON-RPC error: {error}"))
                    });
                }
                match id {
                    1 => {
                        send(
                            &mut stdin,
                            2,
                            "thread/start",
                            json!({
                                "model": self.config.model,
                                "modelProvider": if self.config.base_url.is_empty() { Value::Null } else { json!("omniroute") },
                                "cwd": self.config.cwd.display().to_string(),
                                "approvalPolicy": approval_value(self.config.approval),
                                "sandbox": sandbox_value(self.config.sandbox),
                            }),
                        )
                        .await?;
                    }
                    2 => {
                        let Some(id) = thread_id_of(&message["result"]) else {
                            return Err(Failure::BeforeTurn(
                                "thread/start returned no thread id".to_string(),
                            ));
                        };
                        thread_id = Some(id.clone());
                        on_event(CodexEvent::Started {
                            thread_id: id.clone(),
                            transport: Transport::AppServer,
                        });
                        send(
                            &mut stdin,
                            3,
                            "turn/start",
                            json!({
                                "threadId": id,
                                "input": [{ "type": "text", "text": prompt }],
                            }),
                        )
                        .await?;
                    }
                    3 => {}
                    _ => {}
                }
                continue;
            }

            let method = message
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let params = &message["params"];
            match classify_notification(method, params) {
                ServerStep::Delta(delta) => {
                    emitted = true;
                    // Keep the deltas as the answer if no final message arrives.
                    text.push_str(&delta);
                    on_event(CodexEvent::Delta { text: delta });
                }
                ServerStep::Message(message) => {
                    emitted = true;
                    text = message.clone();
                    on_event(CodexEvent::Message { text: message });
                }
                ServerStep::Notice(note) => on_event(CodexEvent::Notice { message: note }),
                ServerStep::Usage(parsed) => usage = Some(parsed),
                ServerStep::Done(reported) => {
                    usage = reported.or(usage);
                    return Ok(TurnResult {
                        text,
                        usage,
                        transport: Transport::AppServer,
                        thread_id,
                        duration: Duration::ZERO,
                        fell_back: false,
                    });
                }
                ServerStep::Failed(reason) => return Err(Failure::AfterTurn(reason)),
                ServerStep::Ignore => {}
            }
        }
    }

    async fn exec_turn<F>(&self, prompt: &str, on_event: &mut F) -> Result<TurnResult, Failure>
    where
        F: FnMut(CodexEvent),
    {
        let mut command = self.command();
        command.stdin(Stdio::null());
        command.args(["exec", "--json", "--skip-git-repo-check"]);
        for pair in self.overrides() {
            command.arg("-c").arg(pair);
        }
        command.arg("-m").arg(&self.config.model).arg(prompt);

        let mut child = command
            .spawn()
            .map_err(|error| Failure::BeforeTurn(format!("could not start codex: {error}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Failure::BeforeTurn("codex has no stdout".to_string()))?;
        let mut lines = BufReader::new(stdout).lines();
        let deadline = Instant::now() + self.config.timeout;

        let mut text = String::new();
        let mut usage = None;
        let mut thread_id = None;
        let mut finished = false;

        loop {
            let line = next_line(&mut lines, deadline, !text.is_empty()).await?;
            let Some(line) = line else {
                break;
            };
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match classify_exec(&value) {
                ExecStep::Started { thread } => {
                    thread_id = Some(thread.clone());
                    on_event(CodexEvent::Started {
                        thread_id: thread,
                        transport: Transport::Exec,
                    });
                }
                ExecStep::Message(message) => {
                    text = message.clone();
                    on_event(CodexEvent::Message { text: message });
                }
                ExecStep::Notice(note) => on_event(CodexEvent::Notice { message: note }),
                ExecStep::Usage(parsed) => {
                    usage = Some(parsed);
                    finished = true;
                }
                ExecStep::Failed(reason) => return Err(Failure::AfterTurn(reason)),
                ExecStep::Ignore => {}
            }
        }

        let status = child
            .wait()
            .await
            .map_err(|error| Failure::AfterTurn(format!("codex did not exit cleanly: {error}")))?;

        if text.is_empty() {
            return Err(Failure::BeforeTurn(format!(
                "codex exec produced no message (status {status})"
            )));
        }
        if !finished && !status.success() {
            return Err(Failure::AfterTurn(format!(
                "codex exec exited with {status}"
            )));
        }

        Ok(TurnResult {
            text,
            usage,
            transport: Transport::Exec,
            thread_id,
            duration: Duration::ZERO,
            fell_back: false,
        })
    }
}

#[async_trait::async_trait]
impl Adapter for CodexAdapter {
    fn harness(&self) -> Harness {
        Harness::Codex
    }

    async fn probe(&self) -> Result<HarnessInfo, AdapterError> {
        let output = timeout_at(
            Instant::now() + Duration::from_secs(10),
            Command::new(&self.config.exe).arg("--version").output(),
        )
        .await;

        let (installed, version, path) = match output {
            Ok(Ok(output)) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let version = stdout
                    .split_whitespace()
                    .next_back()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
                (true, version, Some(resolve_in_path(&self.config.exe)))
            }
            _ => (false, None, None),
        };

        if !installed {
            return Err(AdapterError::NotInstalled {
                harness: Harness::Codex,
            });
        }

        Ok(HarnessInfo {
            harness: Harness::Codex,
            installed,
            version,
            path,
            auth_mode: auth_mode(&self.config),
            capabilities: Capabilities {
                acp: false,
                json_stream: true,
                programmatic_approval: true,
                worktree: true,
                // Codex's item stream does not guarantee nested agent events (§4.2).
                nested_agents: false,
            },
            provider: Some(self.config.provider().to_string()),
            install_hint: None,
        })
    }
}

/// How Codex proves who it is. Rigel always supplies an API key (D21); with the router that
/// key is Rigel's own, and the router currently does not enforce it (see §10).
fn auth_mode(config: &CodexConfig) -> AuthMode {
    let key_in_env = rigel_core::credentials::provider_environment_variable(config.provider())
        .is_some_and(|name| {
            std::env::var(name)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        });
    let loopback = config.base_url.contains("127.0.0.1") || config.base_url.contains("localhost");
    if key_in_env || loopback {
        AuthMode::ApiKey
    } else {
        AuthMode::None
    }
}

/// The port OmniRoute serves its OpenAI-compatible surface on. Another loopback service on a
/// different port is not the router, however local it is.
fn is_local_omniroute(base_url: &str) -> bool {
    let loopback = base_url.contains("127.0.0.1") || base_url.contains("localhost");
    loopback && base_url.contains(":20128")
}

#[derive(Debug)]
enum Failure {
    /// Nothing reached the model, so another transport may safely try the same prompt.
    BeforeTurn(String),
    /// The turn started; retrying would bill twice.
    AfterTurn(String),
}

impl From<Failure> for AdapterError {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::BeforeTurn(reason) | Failure::AfterTurn(reason) => {
                AdapterError::Other(format!("codex: {reason}"))
            }
        }
    }
}

async fn send(stdin: &mut ChildStdin, id: u64, method: &str, params: Value) -> Result<(), Failure> {
    let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    stdin
        .write_all(format!("{line}\n").as_bytes())
        .await
        .map_err(|error| Failure::BeforeTurn(format!("could not write to codex: {error}")))?;
    stdin
        .flush()
        .await
        .map_err(|error| Failure::BeforeTurn(format!("could not flush to codex: {error}")))
}

async fn next_line(
    lines: &mut Lines<BufReader<ChildStdout>>,
    deadline: Instant,
    emitted: bool,
) -> Result<Option<String>, Failure> {
    match timeout_at(deadline, lines.next_line()).await {
        Ok(Ok(line)) => Ok(line),
        Ok(Err(error)) => Err(if emitted {
            Failure::AfterTurn(format!("could not read codex output: {error}"))
        } else {
            Failure::BeforeTurn(format!("could not read codex output: {error}"))
        }),
        Err(_) => Err(if emitted {
            Failure::AfterTurn("codex timed out mid-turn".to_string())
        } else {
            Failure::BeforeTurn("codex timed out before answering".to_string())
        }),
    }
}

fn sandbox_value(mode: SandboxMode) -> &'static str {
    match mode {
        SandboxMode::ReadOnly => "read-only",
        SandboxMode::WorkspaceWrite => "workspace-write",
        SandboxMode::FullAccess => "danger-full-access",
    }
}

fn approval_value(policy: ApprovalPolicy) -> &'static str {
    match policy {
        ApprovalPolicy::Never => "never",
        // `Auto` still routes through the harness's own approval request, which Rigel answers
        // deterministically; it is not blanket consent.
        ApprovalPolicy::OnRequest | ApprovalPolicy::Auto => "on-request",
    }
}

fn thread_id_of(result: &Value) -> Option<String> {
    ["threadId", "id"]
        .iter()
        .find_map(|key| result.get(*key).and_then(Value::as_str))
        .map(str::to_string)
        .or_else(|| {
            result
                .get("thread")
                .and_then(|thread| thread.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

#[derive(Debug, PartialEq, Eq)]
enum ServerStep {
    Delta(String),
    Message(String),
    Notice(String),
    /// Codex reports usage mid-stream via `thread/tokenUsage/updated`.
    Usage(CodexUsage),
    Done(Option<CodexUsage>),
    Failed(String),
    Ignore,
}

fn classify_notification(method: &str, params: &Value) -> ServerStep {
    match method {
        "item/agentMessage/delta" => params
            .get("delta")
            .and_then(Value::as_str)
            .map(|text| ServerStep::Delta(text.to_string()))
            .unwrap_or(ServerStep::Ignore),
        "item/completed" => match params["item"]["type"].as_str() {
            Some("agentMessage") => params["item"]["text"]
                .as_str()
                .map(|text| ServerStep::Message(text.to_string()))
                .unwrap_or(ServerStep::Ignore),
            Some("error") => params["item"]["message"]
                .as_str()
                .map(|text| ServerStep::Notice(text.to_string()))
                .unwrap_or(ServerStep::Ignore),
            _ => ServerStep::Ignore,
        },
        // `last` is this turn; `total` is the whole thread, which would double-count.
        "thread/tokenUsage/updated" => usage_camel(&params["tokenUsage"]["last"])
            .map(ServerStep::Usage)
            .unwrap_or(ServerStep::Ignore),
        "turn/completed" => ServerStep::Done(
            params
                .get("usage")
                .or_else(|| params["turn"].get("usage"))
                .and_then(usage_of),
        ),
        "turn/failed" => ServerStep::Failed(
            params
                .get("error")
                .or_else(|| params.get("message"))
                .map(Value::to_string)
                .unwrap_or_else(|| "turn failed".to_string()),
        ),
        _ => ServerStep::Ignore,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ExecStep {
    Started { thread: String },
    Message(String),
    Notice(String),
    Usage(CodexUsage),
    Failed(String),
    Ignore,
}

fn classify_exec(value: &Value) -> ExecStep {
    match value.get("type").and_then(Value::as_str) {
        Some("thread.started") => value
            .get("thread_id")
            .and_then(Value::as_str)
            .map(|thread| ExecStep::Started {
                thread: thread.to_string(),
            })
            .unwrap_or(ExecStep::Ignore),
        Some("item.completed") => match value["item"]["type"].as_str() {
            Some("agent_message") => value["item"]["text"]
                .as_str()
                .map(|text| ExecStep::Message(text.to_string()))
                .unwrap_or(ExecStep::Ignore),
            Some("error") => value["item"]["message"]
                .as_str()
                .map(|text| ExecStep::Notice(text.to_string()))
                .unwrap_or(ExecStep::Ignore),
            _ => ExecStep::Ignore,
        },
        Some("turn.completed") => value
            .get("usage")
            .and_then(usage_of)
            .map(ExecStep::Usage)
            .unwrap_or(ExecStep::Ignore),
        Some("turn.failed") => ExecStep::Failed(
            value
                .get("error")
                .or_else(|| value.get("message"))
                .map(Value::to_string)
                .unwrap_or_else(|| "turn failed".to_string()),
        ),
        _ => ExecStep::Ignore,
    }
}

fn usage_of(value: &Value) -> Option<CodexUsage> {
    if !value.is_object() {
        return None;
    }
    let number = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
    Some(CodexUsage {
        input_tokens: number("input_tokens"),
        cached_input_tokens: number("cached_input_tokens"),
        output_tokens: number("output_tokens"),
        reasoning_output_tokens: number("reasoning_output_tokens"),
    })
}

/// app-server uses camelCase for the same counters `exec` reports in snake_case.
fn usage_camel(value: &Value) -> Option<CodexUsage> {
    if !value.is_object() {
        return None;
    }
    let number = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
    Some(CodexUsage {
        input_tokens: number("inputTokens"),
        cached_input_tokens: number("cachedInputTokens"),
        output_tokens: number("outputTokens"),
        reasoning_output_tokens: number("reasoningOutputTokens"),
    })
}

/// Where the Codex binary is, for callers that need to record it in `adapters.json`.
pub fn resolve_exe(explicit: Option<&Path>) -> PathBuf {
    explicit
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "codex.exe" } else { "codex" }))
}

/// Turn a bare command name into the absolute path `probe()` reports. A relative path is
/// left alone: the caller meant that file.
fn resolve_in_path(exe: &Path) -> PathBuf {
    if exe.components().count() > 1 {
        return exe.to_path_buf();
    }
    let Some(path) = std::env::var_os("PATH") else {
        return exe.to_path_buf();
    };
    std::env::split_paths(&path)
        .map(|dir| dir.join(exe))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| exe.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exec_turn_reports_its_message_usage_and_thread() {
        let started = json!({ "type": "thread.started", "thread_id": "th-1" });
        assert_eq!(
            classify_exec(&started),
            ExecStep::Started {
                thread: "th-1".to_string()
            }
        );

        let message = json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "pong" }
        });
        assert_eq!(
            classify_exec(&message),
            ExecStep::Message("pong".to_string())
        );

        let done = json!({
            "type": "turn.completed",
            "usage": { "input_tokens": 155_895, "cached_input_tokens": 12_928, "output_tokens": 5 }
        });
        assert_eq!(
            classify_exec(&done),
            ExecStep::Usage(CodexUsage {
                input_tokens: 155_895,
                cached_input_tokens: 12_928,
                output_tokens: 5,
                reasoning_output_tokens: 0,
            })
        );
    }

    #[test]
    fn an_exec_error_item_is_a_notice_not_a_failure() {
        let warning = json!({
            "type": "item.completed",
            "item": { "type": "error", "message": "ignoring one unrecognized setting" }
        });
        assert_eq!(
            classify_exec(&warning),
            ExecStep::Notice("ignoring one unrecognized setting".to_string())
        );
    }

    #[test]
    fn app_server_deltas_and_completions_map_to_events() {
        let delta = json!({ "delta": "po" });
        assert_eq!(
            classify_notification("item/agentMessage/delta", &delta),
            ServerStep::Delta("po".to_string())
        );

        let completed = json!({ "item": { "type": "agentMessage", "text": "pong" } });
        assert_eq!(
            classify_notification("item/completed", &completed),
            ServerStep::Message("pong".to_string())
        );

        assert_eq!(
            classify_notification("turn/completed", &json!({})),
            ServerStep::Done(None)
        );
        let with_usage = json!({ "usage": { "input_tokens": 7, "output_tokens": 2 } });
        assert_eq!(
            classify_notification("turn/completed", &with_usage),
            ServerStep::Done(Some(CodexUsage {
                input_tokens: 7,
                output_tokens: 2,
                ..CodexUsage::default()
            }))
        );

        // The app-server reports usage mid-turn, in camelCase, with a thread total beside
        // the per-turn figure. Only `last` belongs to this turn.
        let tokens = json!({
            "threadId": "th-1",
            "turnId": "tn-1",
            "tokenUsage": {
                "last": { "inputTokens": 155_895, "cachedInputTokens": 12_928, "outputTokens": 5, "reasoningOutputTokens": 0 },
                "total": { "inputTokens": 155_895, "cachedInputTokens": 12_928, "outputTokens": 5, "reasoningOutputTokens": 0 }
            }
        });
        assert_eq!(
            classify_notification("thread/tokenUsage/updated", &tokens),
            ServerStep::Usage(CodexUsage {
                input_tokens: 155_895,
                cached_input_tokens: 12_928,
                output_tokens: 5,
                reasoning_output_tokens: 0,
            })
        );
    }

    #[test]
    fn a_thread_id_is_found_in_either_shape() {
        assert_eq!(
            thread_id_of(&json!({ "threadId": "a" })).as_deref(),
            Some("a")
        );
        assert_eq!(
            thread_id_of(&json!({ "thread": { "id": "b" } })).as_deref(),
            Some("b")
        );
        assert_eq!(thread_id_of(&json!({})), None);
    }

    #[test]
    fn router_overrides_match_the_probe_recipe() {
        let adapter = CodexAdapter::new(CodexConfig::router("."));
        let overrides = adapter.overrides();
        assert!(
            overrides
                .iter()
                .any(|pair| pair == "model_provider=\"omniroute\"")
        );
        assert!(
            overrides
                .iter()
                .any(|pair| pair.contains("wire_api=\"responses\""))
        );
        assert!(
            overrides
                .iter()
                .any(|pair| pair.contains("requires_openai_auth=false"))
        );

        let mut direct = CodexConfig::router(".");
        direct.base_url = String::new();
        assert!(CodexAdapter::new(direct).overrides().is_empty());
    }

    #[test]
    fn the_local_router_is_the_only_loopback_that_means_omniroute() {
        assert_eq!(
            CodexConfig::router(".").provider(),
            rigel_core::credentials::OMNIROUTE
        );
        let mut other = CodexConfig::router(".");
        other.base_url = "http://127.0.0.1:8080/v1".to_string();
        assert_eq!(other.provider(), rigel_core::credentials::OPENAI);
        let mut direct = CodexConfig::router(".");
        direct.base_url = "https://api.openai.com/v1".to_string();
        assert_eq!(direct.provider(), rigel_core::credentials::OPENAI);
        let mut empty = CodexConfig::router(".");
        empty.base_url = String::new();
        assert_eq!(empty.provider(), rigel_core::credentials::OPENAI);
    }

    #[test]
    fn a_local_router_counts_as_an_api_key_even_without_one_in_env() {
        let config = CodexConfig::router(".");
        // Whatever the ambient environment holds, loopback means Rigel's own key.
        assert_eq!(auth_mode(&config), AuthMode::ApiKey);
    }
}

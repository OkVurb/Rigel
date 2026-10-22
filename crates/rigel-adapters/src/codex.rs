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

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout, Command};
use tokio::time::{Instant, timeout_at};

use rigel_core::ItemKind;
use rigel_core::credentials::{CredentialStore, Secret, get_from};

use crate::normalise::{self, NormalItem};
use crate::{
    Adapter, AdapterError, ApprovalDecision, ApprovalOption, ApprovalPolicy, ApprovalRequest,
    Approver, AuthMode, DenyAll, Harness, HarnessInfo, SandboxMode, credential_spec,
};

/// The model Rigel defaults to. The `-medium` suffix is OmniRoute's effort convention (Q74).
pub const DEFAULT_MODEL: &str = "gpt-5.6-sol-medium";
/// The local router's OpenAI-compatible surface.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:20128/v1";
/// Codex's available-skills catalogue defaults to 2% of the model window (up to 10 000 tokens).
/// Rigel sends a much smaller ceiling: nothing on the skills shelf is needed for a task Rigel
/// started, and the 21 Oct probe showed the catalogue was the difference between a few hundred
/// and 155 895 input tokens for a one-word answer (P4.19).
pub const DEFAULT_SKILLS_BUDGET: u32 = 1_000;
/// Instructions and tool schemas when there is no earlier turn to measure. Deliberately
/// conservative: the estimate's job is to warn, not to be a bill.
pub const FIXED_CONTEXT_TOKENS: u64 = 1_500;

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

#[derive(Clone)]
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
    /// Ceiling on the available-skills catalogue for this turn. `0` disables host skill
    /// discovery entirely (`features.skip_host_skill_discovery=true`).
    pub skills_budget: u32,
    /// One turn, start to finish. The router can be slow on the first turn of the day.
    pub timeout: Duration,
    /// Per-agent process environment. Values stay wrapped so debug output is always redacted.
    pub env: BTreeMap<String, Secret>,
    /// Existence means the vendor login may be usable as a subscription. Rigel never reads it.
    pub vendor_login: Option<PathBuf>,
}

impl fmt::Debug for CodexConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexConfig")
            .field("exe", &self.exe)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("cwd", &self.cwd)
            .field("sandbox", &self.sandbox)
            .field("approval", &self.approval)
            .field("transport", &self.transport)
            .field("skills_budget", &self.skills_budget)
            .field("timeout", &self.timeout)
            .field("env", &self.env)
            .field("vendor_login", &self.vendor_login)
            .finish()
    }
}

impl CodexConfig {
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
            skills_budget: DEFAULT_SKILLS_BUDGET,
            timeout: Duration::from_secs(180),
            env: BTreeMap::new(),
            vendor_login: default_vendor_login(),
        }
    }

    /// Resolve the configured harness credential into this one process environment. Missing or
    /// unavailable storage leaves the map empty so `probe()` can report `None` or `Subscription`;
    /// starting a turn will refuse before spawning.
    pub fn inject_credential(&mut self, store: &dyn CredentialStore) -> bool {
        let Some(spec) = credential_spec(Harness::Codex, Some(&self.base_url)) else {
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
        credential_spec(Harness::Codex, Some(&self.base_url))
            .expect("Codex always has a credential specification")
            .provider
    }
}

/// Normalised progress from one Codex turn. The item pipeline maps these onto `ItemKind`;
/// nothing here knows about sessions or the database.
#[derive(Debug, Clone, PartialEq)]
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
    /// A tool call, file change, plan or other item already sorted into one of the ten kinds
    /// (§5.2, P4.8).
    Item(NormalItem),
    /// Codex paused on an approval and Rigel answered. Recorded after the fact so the
    /// transcript shows what was asked and what the user said (N2).
    Approval {
        request: ApprovalRequest,
        decision: ApprovalDecision,
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

/// A pre-flight input estimate. It is not a bill: `TurnResult::usage` replaces it once the
/// provider reports what it actually charged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextEstimate {
    pub prompt_tokens: u64,
    pub skills_tokens: u32,
    pub fixed_tokens: u64,
    pub input_tokens: u64,
    pub last_turn_input: Option<u64>,
    /// Which term dominated the sum, so the CLI can print a sentence a person can check.
    pub basis: &'static str,
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

#[derive(Clone)]
pub struct CodexAdapter {
    config: CodexConfig,
    /// Who answers `item/commandExecution/requestApproval` and its file-change twin. The
    /// default refuses, which is the right answer when nothing is watching the screen.
    approver: Arc<dyn Approver>,
}

impl CodexAdapter {
    pub fn new(config: CodexConfig) -> Self {
        Self {
            config,
            approver: Arc::new(DenyAll),
        }
    }

    /// Install the surface that answers Codex's approval requests. Codex only understands
    /// accept / accept-for-session / decline, so a project or always grant is remembered by
    /// Rigel's store and replayed as `accept` for each new request.
    pub fn with_approver(mut self, approver: Arc<dyn Approver>) -> Self {
        self.approver = approver;
        self
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
        if auth_mode(&self.config) != AuthMode::ApiKey {
            return Err(AdapterError::RequiresApiKey {
                harness: Harness::Codex,
            });
        }
        let secrets = self.config.env.values().cloned().collect::<Vec<_>>();
        let mut emit = |event| on_event(redact_event(event, &secrets));
        let started = Instant::now();
        let outcome = match self.config.transport {
            Transport::AppServer => self.app_server_turn(prompt, &mut emit).await,
            Transport::Exec => self.exec_turn(prompt, &mut emit).await,
            Transport::Auto => match self.app_server_turn(prompt, &mut emit).await {
                Ok(result) => Ok(result),
                Err(Failure::BeforeTurn(reason)) => {
                    emit(CodexEvent::Notice {
                        message: format!(
                            "app-server failed ({}); falling back to exec",
                            redact_text(&reason, &secrets)
                        ),
                    });
                    self.exec_turn(prompt, &mut emit).await.map(|mut result| {
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
                result.text = redact_text(&result.text, &secrets);
                result
            })
            .map_err(|failure| AdapterError::from(failure.redacted(&secrets)))
    }

    /// The `-c` overrides for pointing Codex at a router and keeping its context small. Written
    /// here, never taken from `omniroute launch-codex` (see the module docs).
    fn overrides(&self) -> Vec<String> {
        let mut overrides = Vec::new();
        if !self.config.base_url.trim().is_empty() {
            overrides.extend([
                "model_provider=\"omniroute\"".to_string(),
                "model_providers.omniroute.name=\"OmniRoute\"".to_string(),
                format!(
                    "model_providers.omniroute.base_url=\"{}\"",
                    self.config.base_url
                ),
                "model_providers.omniroute.wire_api=\"responses\"".to_string(),
                // A custom provider authenticates with the variable it names, and Codex sends that
                // value as the bearer token. `requires_openai_auth` must stay false here: it selects
                // Codex's own OpenAI auth (the ChatGPT login in `~/.codex/auth.json`), which is the
                // subscription reuse D21 forbids, and it bypasses the per-agent environment.
                "model_providers.omniroute.env_key=\"OPENAI_API_KEY\"".to_string(),
                "model_providers.omniroute.requires_openai_auth=false".to_string(),
            ]);
        }
        // P4.19: the available-skills catalogue is context, not capability. `0` turns host skill
        // discovery off for this turn; anything else is a token ceiling for the catalogue.
        if self.config.skills_budget == 0 {
            overrides.push("features.skip_host_skill_discovery=true".to_string());
        } else {
            overrides.push(format!(
                "skills.max_context_tokens={}",
                self.config.skills_budget
            ));
        }
        overrides
    }

    /// What this turn will cost in input tokens before it is sent. When the caller knows the
    /// previous turn's real count, that is the better basis: the fixed context was already
    /// measured once, and only the new prompt is added. Otherwise the fixed allowance and the
    /// skills ceiling are summed, and the result says so.
    pub fn estimate_input(&self, prompt: &str, last_turn_input: Option<u64>) -> ContextEstimate {
        let prompt_tokens = rigel_core::agent::pricing::tokens_for(prompt.chars().count());
        let skills_tokens = self.config.skills_budget;
        let (input_tokens, fixed_tokens, basis) = match last_turn_input {
            Some(previous) => (previous + prompt_tokens, 0, "last turn + prompt"),
            None => {
                let fixed = FIXED_CONTEXT_TOKENS + u64::from(skills_tokens);
                (
                    prompt_tokens + fixed,
                    FIXED_CONTEXT_TOKENS,
                    "prompt + fixed + skills",
                )
            }
        };
        ContextEstimate {
            prompt_tokens,
            skills_tokens,
            fixed_tokens,
            input_tokens,
            last_turn_input,
            basis,
        }
    }

    fn command(&self) -> Command {
        let exe = crate::registry::resolve_codex_exe(&self.config.exe);
        let mut command = crate::registry::command_for(&exe);
        command
            .current_dir(&self.config.cwd)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited, not piped: nothing reads stderr, and an unread pipe can fill and
            // deadlock a chatty child. Codex's log lines belong in the process's own stderr.
            .stderr(Stdio::inherit());
        // Do not let an ambient vendor key bypass the per-agent resolver. The resolved map is the
        // only way a harness credential crosses the process boundary.
        command
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("CURSOR_API_KEY")
            .envs(
                self.config
                    .env
                    .iter()
                    .map(|(name, value)| (name, value.expose())),
            );
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

            if is_subscription_notification(&message) {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(Failure::RequiresApiKey);
            }

            // A request from the server carries both an `id` and a `method`. It must be answered
            // in place: treating it as a response to one of our ids would re-send a request and
            // leave Codex waiting forever.
            if message.get("method").is_some() {
                let method = message
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if let Some(request) = approval_of(method, &message["params"]) {
                    let decision = self.approver.approve(&request).await;
                    respond(
                        &mut stdin,
                        message.get("id").cloned().unwrap_or(Value::Null),
                        json!({ "decision": codex_decision(decision) }),
                    )
                    .await?;
                    on_event(CodexEvent::Approval { request, decision });
                    continue;
                }
                // Any other server request is answered with an error rather than silence, so
                // the harness does not hang on a feature this adapter does not implement.
                if message.get("id").is_some() {
                    respond_error(
                        &mut stdin,
                        message.get("id").cloned().unwrap_or(Value::Null),
                        -32601,
                        &format!("{method} is not implemented by rigel"),
                    )
                    .await?;
                    continue;
                }
            }

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
                ServerStep::Item(item) => on_event(CodexEvent::Item(item)),
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
            // `exec` has no structured auth event, so the pre-spawn refusal is its only guard;
            // the app-server transport additionally kills a child that reports a ChatGPT login.
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
                ExecStep::Item(item) => on_event(CodexEvent::Item(item)),
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
        // A missing binary is not an error: the probe reports `installed: false` so that Settings
        // can show an install hint.
        Ok(crate::registry::probe_codex(&self.config).await)
    }
}

/// How Codex proves who it is. A loopback URL is not evidence: only an injected, non-empty API
/// key qualifies. A vendor login file is checked for existence only and classed as Subscription.
pub(crate) fn auth_mode(config: &CodexConfig) -> AuthMode {
    let has_key = credential_spec(Harness::Codex, Some(&config.base_url)).is_some_and(|spec| {
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

#[derive(Debug)]
enum Failure {
    /// Nothing reached the model, so another transport may safely try the same prompt.
    BeforeTurn(String),
    /// The turn started; retrying would bill twice.
    AfterTurn(String),
    RequiresApiKey,
}

impl Failure {
    fn redacted(self, secrets: &[Secret]) -> Self {
        match self {
            Self::BeforeTurn(reason) => Self::BeforeTurn(redact_text(&reason, secrets)),
            Self::AfterTurn(reason) => Self::AfterTurn(redact_text(&reason, secrets)),
            Self::RequiresApiKey => Self::RequiresApiKey,
        }
    }
}

impl From<Failure> for AdapterError {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::RequiresApiKey => AdapterError::RequiresApiKey {
                harness: Harness::Codex,
            },
            Failure::BeforeTurn(reason) | Failure::AfterTurn(reason) => {
                AdapterError::Other(format!("codex: {reason}"))
            }
        }
    }
}

fn default_vendor_login() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .map(|home| home.join(".codex").join("auth.json"))
}

fn redact_text(value: &str, secrets: &[Secret]) -> String {
    secrets.iter().fold(value.to_string(), |redacted, secret| {
        let exposed = secret.expose();
        if exposed.is_empty() {
            redacted
        } else {
            redacted.replace(exposed, "<redacted>")
        }
    })
}

fn redact_event(event: CodexEvent, secrets: &[Secret]) -> CodexEvent {
    match event {
        CodexEvent::Started {
            thread_id,
            transport,
        } => CodexEvent::Started {
            thread_id: redact_text(&thread_id, secrets),
            transport,
        },
        CodexEvent::Delta { text } => CodexEvent::Delta {
            text: redact_text(&text, secrets),
        },
        CodexEvent::Message { text } => CodexEvent::Message {
            text: redact_text(&text, secrets),
        },
        CodexEvent::Notice { message } => CodexEvent::Notice {
            message: redact_text(&message, secrets),
        },
        CodexEvent::Item(item) => CodexEvent::Item(NormalItem::new(
            item.kind,
            crate::redact_json(&item.body, secrets),
        )),
        CodexEvent::Approval { request, decision } => CodexEvent::Approval {
            request: ApprovalRequest {
                tool_call_id: request.tool_call_id,
                title: redact_text(&request.title, secrets),
                kind: request.kind,
                raw_input: request
                    .raw_input
                    .map(|input| crate::redact_json(&input, secrets)),
                options: request.options,
            },
            decision,
        },
    }
}

/// Belt-and-braces refusal for a process whose first auth update says it selected ChatGPT.
fn is_subscription_notification(message: &Value) -> bool {
    message.get("method").and_then(Value::as_str) == Some("account/updated")
        && matches!(
            message["params"].get("authMode").and_then(Value::as_str),
            Some("chatgpt" | "subscription")
        )
}

/// Answer a server-initiated request. The id is echoed as it arrived: app-server ids are
/// integers today, but a string id must not be re-typed as a number.
async fn respond(stdin: &mut ChildStdin, id: Value, result: Value) -> Result<(), Failure> {
    let line = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    write_line(stdin, &line).await
}

async fn respond_error(
    stdin: &mut ChildStdin,
    id: Value,
    code: i64,
    message: &str,
) -> Result<(), Failure> {
    let line = json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } });
    write_line(stdin, &line).await
}

async fn write_line(stdin: &mut ChildStdin, line: &Value) -> Result<(), Failure> {
    stdin
        .write_all(format!("{line}\n").as_bytes())
        .await
        .map_err(|error| Failure::BeforeTurn(format!("could not write to codex: {error}")))?;
    stdin
        .flush()
        .await
        .map_err(|error| Failure::BeforeTurn(format!("could not flush to codex: {error}")))
}

async fn send(stdin: &mut ChildStdin, id: u64, method: &str, params: Value) -> Result<(), Failure> {
    let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
    write_line(stdin, &line).await
}

/// Codex's four approval methods, as one request shape. The command is the title because it is
/// what the user has to read; `reason` stands in when a harness version omits the text.
fn approval_of(method: &str, params: &Value) -> Option<ApprovalRequest> {
    let kind = match method {
        "item/commandExecution/requestApproval" | "execCommandApproval" => "command",
        "item/fileChange/requestApproval" | "applyPatchApproval" => "file_change",
        _ => return None,
    };
    let reason = params
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let command = params.get("command").map(|command| match command {
        Value::Array(parts) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    });
    let title = match command.as_deref() {
        Some(text) if !text.trim().is_empty() => text.to_string(),
        _ if !reason.trim().is_empty() => reason.to_string(),
        _ if kind == "file_change" => "apply a file change".to_string(),
        _ => "run a command".to_string(),
    };
    let tool_call_id = params
        .get("itemId")
        .or_else(|| params.get("callId"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(ApprovalRequest {
        tool_call_id,
        title,
        kind: Some(kind.to_string()),
        raw_input: Some(params.clone()),
        // Exactly what Codex can express. A project or always grant is Rigel's own scope and is
        // replayed here as `accept`, never silently widened to the harness's session grant.
        options: vec![
            ApprovalOption {
                option_id: "accept".to_string(),
                name: "Allow once".to_string(),
                kind: "allow_once".to_string(),
            },
            ApprovalOption {
                option_id: "acceptForSession".to_string(),
                name: "Allow for this Codex session".to_string(),
                kind: "allow_always".to_string(),
            },
            ApprovalOption {
                option_id: "decline".to_string(),
                name: "Reject".to_string(),
                kind: "reject_once".to_string(),
            },
        ],
    })
}

fn codex_decision(decision: ApprovalDecision) -> &'static str {
    match decision {
        ApprovalDecision::AllowOnce => "accept",
        ApprovalDecision::AllowAlways => "acceptForSession",
        ApprovalDecision::RejectOnce | ApprovalDecision::RejectAlways => "decline",
        ApprovalDecision::Cancel => "cancel",
    }
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

/// One app-server or `exec` item as one of the ten kinds (§5.2). Both transports are decoded by
/// the same function: `exec` spells its types and some keys in `snake_case`, and the fallback must
/// not lose a command because of it. `None` means there is nothing to show — the caller emits an
/// `agentMessage` through its own path, and `started` items carry no result yet.
fn codex_item_of(item: &Value) -> Option<NormalItem> {
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
    // `TodoList`/`todo_list` is the exec transport's plan.
    let kind = normalise::codex_item(item_type).or_else(|| normalise::codex_exec_item(item_type));
    let Some(kind) = kind else {
        return Some(NormalItem::unmapped(
            "codex",
            item.clone(),
            "no item kind for this item type",
        ));
    };
    let field = |camel: &str, snake: &str| item.get(camel).or_else(|| item.get(snake));
    let decoded = match kind {
        ItemKind::Message => item
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(NormalItem::message),
        ItemKind::Reasoning => {
            let text = reasoning_text(item);
            (!text.is_empty()).then(|| NormalItem::reasoning(text))
        }
        ItemKind::Command => Some(
            NormalItem::command(
                item.get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
            .with(json!({
                "cwd": item.get("cwd"),
                "status": item.get("status"),
                "exit_code": field("exitCode", "exit_code"),
                "output": field("aggregatedOutput", "aggregated_output"),
            })),
        ),
        ItemKind::FileChange => {
            let changes = item.get("changes");
            let first = changes
                .and_then(Value::as_array)
                .and_then(|entries| entries.first());
            let operation = first
                .and_then(|change| change.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("write");
            let path = first
                .and_then(|change| change.get("path"))
                .and_then(Value::as_str);
            Some(NormalItem::file_change(operation, path).with(json!({
                "changes": changes,
                "status": item.get("status"),
            })))
        }
        ItemKind::McpCall => Some(
            NormalItem::mcp_call(
                item.get("server").and_then(Value::as_str).unwrap_or("mcp"),
                item.get("tool").and_then(Value::as_str).unwrap_or("tool"),
                item.get("arguments").cloned().unwrap_or_else(|| json!({})),
            )
            .with(json!({
                "status": item.get("status"),
                "result": item.get("result"),
                "error": item.get("error"),
            })),
        ),
        ItemKind::WebSearch => Some(
            NormalItem::web_search(
                item.get("query")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
            .with(json!({ "status": item.get("status") })),
        ),
        ItemKind::Plan => Some(NormalItem::plan(plan_texts(item))),
        ItemKind::Error => item
            .get("message")
            .and_then(Value::as_str)
            .map(|message| NormalItem::error("HARNESS", message)),
        // Not carried by an app-server item, or handled by the caller.
        ItemKind::Approval | ItemKind::Usage => None,
    }?;
    Some(decoded)
}

/// A reasoning item reports either a single `text` or a list of `summary` parts.
fn reasoning_text(item: &Value) -> String {
    if let Some(text) = item.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    item.get("summary")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// A plan arrives as bare strings, `{step}` objects or `{text}` todos depending on transport.
fn plan_texts(item: &Value) -> Vec<String> {
    let entries = item
        .get("plan")
        .or_else(|| item.get("items"))
        .or_else(|| item.get("steps"))
        .and_then(Value::as_array);
    entries
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    entry.as_str().map(str::to_string).or_else(|| {
                        entry
                            .get("step")
                            .or_else(|| entry.get("text"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, PartialEq, Eq)]
enum ServerStep {
    Delta(String),
    Message(String),
    Item(NormalItem),
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
        // Only `completed` is decoded: `started` for a tool carries no result yet, and the
        // transcript would then hold two rows for one call. The agentMessage text has already
        // streamed through `item/agentMessage/delta`.
        "item/completed" => match params["item"]["type"].as_str() {
            Some("agentMessage") => params["item"]["text"]
                .as_str()
                .filter(|text| !text.is_empty())
                .map(|text| ServerStep::Message(text.to_string()))
                .unwrap_or(ServerStep::Ignore),
            Some(_) => codex_item_of(&params["item"])
                .map(ServerStep::Item)
                .unwrap_or(ServerStep::Ignore),
            None => ServerStep::Ignore,
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
    Item(NormalItem),
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
                .filter(|text| !text.is_empty())
                .map(|text| ExecStep::Message(text.to_string()))
                .unwrap_or(ExecStep::Ignore),
            Some(_) => codex_item_of(&value["item"])
                .map(ExecStep::Item)
                .unwrap_or(ExecStep::Ignore),
            None => ExecStep::Ignore,
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
    fn an_exec_error_item_is_an_error_item_not_a_failed_turn() {
        let warning = json!({
            "type": "item.completed",
            "item": { "type": "error", "message": "ignoring one unrecognized setting" }
        });
        let ExecStep::Item(item) = classify_exec(&warning) else {
            panic!("expected an item");
        };
        assert_eq!(item.kind, ItemKind::Error);
        assert_eq!(item.body["message"], "ignoring one unrecognized setting");
        assert!(!item.is_unmapped());
    }

    #[test]
    fn an_exec_command_is_a_command_item_with_its_exit_code() {
        let completed = json!({
            "type": "item.completed",
            "item": {
                "type": "command_execution",
                "command": "cargo test --quiet",
                "status": "completed",
                "exit_code": 0,
                "aggregated_output": "ok. 102 passed"
            }
        });
        let ExecStep::Item(item) = classify_exec(&completed) else {
            panic!("expected an item");
        };
        assert_eq!(item.kind, ItemKind::Command);
        assert_eq!(item.body["command"], "cargo test --quiet");
        assert_eq!(item.body["exit_code"], 0);
        assert_eq!(item.body["output"], "ok. 102 passed");
    }

    #[test]
    fn every_app_server_item_type_maps_to_its_kind_or_stays_unmapped() {
        let cases = [
            (
                json!({ "type": "reasoning", "text": "weighing options" }),
                ItemKind::Reasoning,
            ),
            (
                json!({ "type": "commandExecution", "command": "cargo test", "exitCode": 0 }),
                ItemKind::Command,
            ),
            (
                json!({
                    "type": "fileChange",
                    "status": "completed",
                    "changes": [{ "path": "src/lib.rs", "kind": "update" }]
                }),
                ItemKind::FileChange,
            ),
            (
                json!({
                    "type": "mcpToolCall",
                    "server": "files",
                    "tool": "read",
                    "arguments": { "path": "README.md" }
                }),
                ItemKind::McpCall,
            ),
            (
                json!({ "type": "webSearch", "query": "rustfmt options" }),
                ItemKind::WebSearch,
            ),
            (
                json!({ "type": "todoList", "items": [{ "text": "read" }, { "text": "fix" }] }),
                ItemKind::Plan,
            ),
            (
                json!({ "type": "error", "message": "drift" }),
                ItemKind::Error,
            ),
        ];
        for (item, expected) in cases {
            let Some(decoded) = codex_item_of(&item) else {
                panic!("no item for {item}");
            };
            assert_eq!(decoded.kind, expected, "{item}");
            assert!(!decoded.is_unmapped(), "{item}");
        }

        let unknown = codex_item_of(&json!({ "type": "hologram", "value": 1 })).unwrap();
        assert!(unknown.is_unmapped());
        assert_eq!(unknown.body["raw"]["value"], 1);
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
                .any(|pair| pair.contains("env_key=\"OPENAI_API_KEY\""))
        );

        // The skills ceiling is not provider-specific and must survive a direct connection.
        assert!(
            overrides
                .iter()
                .any(|pair| pair == "skills.max_context_tokens=1000"),
            "the default skills ceiling must be sent: {overrides:?}"
        );

        let mut direct = CodexConfig::router(".");
        direct.base_url = String::new();
        let direct_overrides = CodexAdapter::new(direct).overrides();
        assert!(
            direct_overrides
                .iter()
                .all(|pair| !pair.starts_with("model_provider")),
            "a direct connection invents no provider: {direct_overrides:?}"
        );
        assert_eq!(direct_overrides.len(), 1);
    }

    #[test]
    fn disabling_skills_is_a_feature_flag_and_zero_budget_is_honest() {
        let mut config = CodexConfig::router(".");
        config.skills_budget = 0;
        let overrides = CodexAdapter::new(config).overrides();
        assert!(
            overrides
                .iter()
                .any(|pair| pair == "features.skip_host_skill_discovery=true"),
            "{overrides:?}"
        );
        assert!(
            overrides
                .iter()
                .all(|pair| !pair.starts_with("skills.max_context_tokens")),
            "a zero ceiling is expressed by disabling discovery, not by asking for 0 tokens"
        );
    }

    #[test]
    fn the_estimate_uses_the_last_turn_when_there_is_one() {
        let adapter = CodexAdapter::new(CodexConfig::router("."));
        let cold = adapter.estimate_input("say pong", None);
        assert_eq!(cold.prompt_tokens, 2);
        assert_eq!(cold.skills_tokens, DEFAULT_SKILLS_BUDGET);
        assert_eq!(cold.fixed_tokens, FIXED_CONTEXT_TOKENS);
        assert_eq!(cold.input_tokens, 2 + FIXED_CONTEXT_TOKENS + 1_000);
        assert_eq!(cold.basis, "prompt + fixed + skills");

        let warm = adapter.estimate_input("say pong", Some(155_895));
        assert_eq!(warm.input_tokens, 155_895 + 2);
        assert_eq!(warm.last_turn_input, Some(155_895));
        assert_eq!(warm.basis, "last turn + prompt");
        assert!(
            warm.input_tokens > cold.input_tokens,
            "a measured 155k turn must not be estimated as a 3k one"
        );
    }

    #[test]
    fn loopback_is_not_authentication_without_an_injected_key() {
        let mut config = CodexConfig::router(".");
        config.vendor_login = None;
        assert_eq!(auth_mode(&config), AuthMode::None);

        config
            .env
            .insert("OPENAI_API_KEY".to_string(), Secret::new("not-printed"));
        assert_eq!(auth_mode(&config), AuthMode::ApiKey);
    }

    #[test]
    fn child_errors_and_events_cannot_echo_an_injected_key() {
        let secrets = vec![Secret::new("sk-test-must-stay-secret")];
        let error = AdapterError::from(
            Failure::AfterTurn("child echoed sk-test-must-stay-secret".to_string())
                .redacted(&secrets),
        );
        assert_eq!(error.to_string(), "codex: child echoed <redacted>");

        let event = redact_event(
            CodexEvent::Notice {
                message: "notice sk-test-must-stay-secret".to_string(),
            },
            &secrets,
        );
        assert_eq!(
            event,
            CodexEvent::Notice {
                message: "notice <redacted>".to_string()
            }
        );
    }
}

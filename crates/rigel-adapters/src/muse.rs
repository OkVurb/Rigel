//! The Muse Code adapter: one prompt through `muse exec --json`.
//!
//! Muse Code's headless surface is a JSONL journal on stdout. Every line is an envelope carrying
//! `schema_version`, `payload_type`, `payload_schema_version` and `payload`; the shapes here follow
//! Meta's published envelope (`muse-code/extending`) and two independent parsers of it. The answer
//! is `run.terminal.completed` -> `payload.text`; `run.output.delta` carries the same text in
//! fragments, so terminal text wins whenever it arrives. `run.terminal.failed` carries
//! `payload.reason`, and `run.model.configured` names the model Muse actually chose.
//!
//! Rigel drives the one-shot transport only. `muse serve` is a stdio session protocol and is not
//! used yet. The published stream carries no token counts — Muse writes those to its session log
//! and SASE reads them from disk — so a turn records its answer but no `Usage` row. The CLI says
//! that plainly rather than estimating a bill the provider never reported.
//!
//! **Approvals and sandbox.** A hidden child cannot answer a prompt, so the turn passes
//! `--disable-approval`; the sandbox stays on. `--yolo` (approval *and* sandbox off) is never sent.
//! `--trust-workspace` is not sent either: it would let the project's hooks run outside the sandbox
//! without asking. A workspace Muse refuses to trust therefore fails loudly instead of running
//! untrusted project code.
//!
//! **Auth.** D21: the child receives only `META_API_KEY` from the keychain, and a turn refuses
//! without it. Muse Code's device-code login is reported as `Subscription` and never reused. The
//! stream has no auth event, so Rigel cannot prove which credential Muse preferred; it can only
//! guarantee which one it injected.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::{Instant, timeout_at};

use rigel_core::ItemKind;
use rigel_core::credentials::{CredentialStore, Secret, get_from};

use crate::normalise::{self, NormalItem};
use crate::{
    Adapter, AdapterError, AuthMode, Harness, HarnessInfo, StreamRedactor, credential_spec, redact,
    redact_json,
};

/// Rigel's default Muse model. The native `muse` provider uses the same id, so a turn recorded
/// through either path compares like for like. `--model muse-wonder-1.0` selects the frontier one.
pub const DEFAULT_MODEL: &str = "muse-spark-1.3";

#[derive(Clone)]
pub struct MuseConfig {
    /// The `muse` binary. Resolved from `PATH` when left as the default.
    pub exe: PathBuf,
    pub model: String,
    pub cwd: PathBuf,
    /// One turn, start to finish. Coding turns can outlast the default.
    pub timeout: Duration,
    /// Per-agent process environment. Values stay wrapped so debug output is always redacted.
    pub env: BTreeMap<String, Secret>,
    /// Existence means `muse auth` may have stored a subscription. Rigel never reads it.
    pub vendor_login: Option<PathBuf>,
}

impl fmt::Debug for MuseConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MuseConfig")
            .field("exe", &self.exe)
            .field("model", &self.model)
            .field("cwd", &self.cwd)
            .field("timeout", &self.timeout)
            .field("env", &self.env)
            .field("vendor_login", &self.vendor_login)
            .finish()
    }
}

impl MuseConfig {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            exe: PathBuf::from(if cfg!(windows) { "muse.exe" } else { "muse" }),
            model: DEFAULT_MODEL.to_string(),
            cwd: cwd.into(),
            timeout: Duration::from_secs(600),
            env: BTreeMap::new(),
            vendor_login: default_vendor_login(),
        }
    }

    /// Resolve the keychain's Meta key into this one process environment. A missing key leaves the
    /// map empty so the probe can report `None` or `Subscription`; starting a turn refuses before
    /// spawning.
    pub fn inject_credential(&mut self, store: &dyn CredentialStore) -> bool {
        let Some(spec) = credential_spec(Harness::Muse, None) else {
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
        credential_spec(Harness::Muse, None)
            .expect("Muse always has a credential specification")
            .provider
    }

    /// `muse exec` arguments. Kept separate from the process so a test can pin every flag; the
    /// sandbox and workspace-trust postures are policy, not implementation detail.
    fn arguments(&self, session_id: &str, prompt_file: &Path) -> Vec<String> {
        vec![
            "exec".to_string(),
            "--json".to_string(),
            "--workspace".to_string(),
            self.cwd.display().to_string(),
            "--model".to_string(),
            self.model.clone(),
            // Skip prompts, keep the sandbox. `--yolo` would drop both and is never sent.
            "--disable-approval".to_string(),
            // A headless run cannot answer a clarifying question; let Muse pick the default.
            "--user-input-auto-resolve".to_string(),
            // Do not read Claude/Codex personal skills into a Rigel turn.
            "--no-foreign-personal-context".to_string(),
            "--session-id".to_string(),
            session_id.to_string(),
            "--prompt-file".to_string(),
            prompt_file.display().to_string(),
        ]
    }

    fn command(&self, session_id: &str, prompt_file: &Path) -> Command {
        let exe = crate::registry::resolve_exe(Harness::Muse, &self.exe);
        let mut command = crate::registry::command_for(&exe);
        command
            .current_dir(&self.cwd)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            // Inherited, not piped: nothing reads stderr, and an unread pipe can fill and deadlock
            // a chatty child. Muse's diagnostics belong in the process's own stderr.
            .stderr(Stdio::inherit());
        command
            // Do not let an ambient vendor key bypass the per-agent resolver.
            .env_remove("META_API_KEY")
            // The launcher checks for a new binary hourly and swaps it in; a turn must not have
            // its executable replaced mid-run.
            .env("MUSE_NO_AUTO_UPDATE", "1")
            .envs(self.env.iter().map(|(name, value)| (name, value.expose())));
        command.args(self.arguments(session_id, prompt_file));
        command
    }
}

/// Normalised progress from one Muse turn.
#[derive(Debug, Clone, PartialEq)]
pub enum MuseEvent {
    Delta {
        text: String,
    },
    /// A tool result, file change or other item already sorted into one of the ten kinds.
    Item(NormalItem),
    Notice {
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct MuseTurn {
    pub text: String,
    pub model: String,
    pub session_id: String,
    pub duration: Duration,
}

#[derive(Clone)]
pub struct MuseAdapter {
    config: MuseConfig,
}

impl MuseAdapter {
    pub fn new(config: MuseConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &MuseConfig {
        &self.config
    }

    /// Run one prompt to completion, reporting deltas and items as they arrive.
    pub async fn run_turn<F>(&self, prompt: &str, mut on_event: F) -> Result<MuseTurn, AdapterError>
    where
        F: FnMut(MuseEvent),
    {
        if auth_mode(&self.config) != AuthMode::ApiKey {
            return Err(AdapterError::RequiresApiKey {
                harness: Harness::Muse,
            });
        }
        let secrets = self.config.env.values().cloned().collect::<Vec<_>>();
        let mut stream = StreamRedactor::new(&secrets);
        let started = Instant::now();
        let outcome = {
            let mut emit = |event| match event {
                MuseEvent::Delta { text } => {
                    let safe = stream.push(&text);
                    if !safe.is_empty() {
                        on_event(MuseEvent::Delta { text: safe });
                    }
                }
                MuseEvent::Item(item) => on_event(MuseEvent::Item(NormalItem {
                    kind: item.kind,
                    body: redact_json(&item.body, &secrets),
                })),
                MuseEvent::Notice { message } => on_event(MuseEvent::Notice {
                    message: redact(&message, &secrets),
                }),
            };
            self.exec_turn(prompt, &mut emit).await
        };
        let tail = stream.finish();
        if !tail.is_empty() {
            on_event(MuseEvent::Delta { text: tail });
        }

        outcome
            .map(|mut turn| {
                turn.duration = started.elapsed();
                turn.text = redact(&turn.text, &secrets);
                turn
            })
            .map_err(|failure| AdapterError::from(failure.redacted(&secrets)))
    }

    async fn exec_turn<F>(&self, prompt: &str, on_event: &mut F) -> Result<MuseTurn, Failure>
    where
        F: FnMut(MuseEvent),
    {
        let session_id = uuid::Uuid::new_v4().to_string();
        let prompt_file = write_prompt_file(prompt)?;
        let _guard = PromptFile(prompt_file.clone());

        let mut child = self
            .config
            .command(&session_id, &prompt_file)
            .spawn()
            .map_err(|error| Failure::BeforeTurn(format!("could not start muse: {error}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Failure::BeforeTurn("muse has no stdout".to_string()))?;
        let mut lines = BufReader::new(stdout).lines();
        let deadline = Instant::now() + self.config.timeout;

        let mut terminal: Option<String> = None;
        let mut failed: Option<String> = None;
        let mut streamed = String::new();
        let mut model: Option<String> = None;

        loop {
            let line = match timeout_at(deadline, lines.next_line()).await {
                Ok(Ok(line)) => line,
                Ok(Err(error)) => {
                    let _ = child.kill().await;
                    return Err(Failure::AfterTurn(format!(
                        "could not read muse's journal: {error}"
                    )));
                }
                Err(_) => {
                    let _ = child.kill().await;
                    return Err(Failure::AfterTurn(format!(
                        "muse did not finish within {}s",
                        self.config.timeout.as_secs()
                    )));
                }
            };
            let Some(line) = line else {
                break;
            };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                // Diagnostic lines are allowed on stderr; a non-JSON line on stdout is not a
                // reason to abandon a turn that may still finish.
                continue;
            };
            match classify(&value) {
                MuseStep::Delta(text) => {
                    streamed.push_str(&text);
                    on_event(MuseEvent::Delta { text });
                }
                // The root run completes last, so the last terminal text is the answer; child
                // runs finish before it and cannot overwrite it.
                MuseStep::Terminal { text } => terminal = Some(text),
                MuseStep::Failed(reason) => failed = Some(reason),
                MuseStep::Model(name) => model = Some(name),
                MuseStep::Item(item) => on_event(MuseEvent::Item(item)),
                MuseStep::Ignore => {}
            }
        }

        let status = child
            .wait()
            .await
            .map_err(|error| Failure::AfterTurn(format!("muse did not exit cleanly: {error}")))?;

        if let Some(reason) = failed {
            return Err(Failure::AfterTurn(reason));
        }
        if !status.success() {
            return Err(Failure::AfterTurn(format!(
                "muse exec exited with {status}"
            )));
        }

        let text = terminal.filter(|text| !text.is_empty()).unwrap_or(streamed);
        if text.is_empty() {
            return Err(Failure::BeforeTurn(format!(
                "muse exec produced no message (status {status})"
            )));
        }

        Ok(MuseTurn {
            text,
            model: model.unwrap_or_else(|| self.config.model.clone()),
            session_id,
            duration: Duration::ZERO,
        })
    }
}

#[async_trait::async_trait]
impl Adapter for MuseAdapter {
    fn harness(&self) -> Harness {
        Harness::Muse
    }

    async fn probe(&self) -> Result<HarnessInfo, AdapterError> {
        Ok(crate::registry::probe_muse(&self.config).await)
    }
}

/// How Muse proves who it is. Only an injected, non-empty Meta key qualifies for a turn; a login
/// file is classified for the probe, never used to spawn.
pub(crate) fn auth_mode(config: &MuseConfig) -> AuthMode {
    let has_key = config
        .env
        .get("META_API_KEY")
        .is_some_and(|value| !value.expose().trim().is_empty());
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

/// Muse's login lives in `$XDG_CONFIG_HOME/muse/auth.json`, falling back to `~/.config`. Windows
/// ignores `MUSE_AUTH_PATH`, so this is the one location that works on every platform.
fn default_vendor_login() -> Option<PathBuf> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|home| PathBuf::from(home).join(".config"))
        })?;
    Some(config_home.join("muse").join("auth.json"))
}

/// A prompt file that is removed however the turn ends, including a spawn error or a kill.
struct PromptFile(PathBuf);

impl Drop for PromptFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn write_prompt_file(prompt: &str) -> Result<PathBuf, Failure> {
    let path = std::env::temp_dir().join(format!("rigel-muse-{}.txt", uuid::Uuid::new_v4()));
    std::fs::write(&path, prompt).map_err(|error| {
        Failure::BeforeTurn(format!(
            "could not write the prompt to {}: {error}",
            path.display()
        ))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(path)
}

#[derive(Debug, PartialEq, Eq)]
enum MuseStep {
    Delta(String),
    Terminal { text: String },
    Failed(String),
    Model(String),
    Item(NormalItem),
    Ignore,
}

fn classify(value: &Value) -> MuseStep {
    let payload_type = value
        .get("payload_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload = value.get("payload").unwrap_or(&Value::Null);
    match payload_type {
        "run.output.delta" => payload
            .get("text")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| MuseStep::Delta(text.to_string()))
            .unwrap_or(MuseStep::Ignore),
        "run.terminal.completed" => {
            let outcome = terminal_outcome(payload).unwrap_or("completed");
            if !matches!(outcome, "completed" | "success" | "succeeded" | "ok") {
                let reason = payload
                    .get("reason")
                    .and_then(Value::as_str)
                    .filter(|reason| !reason.trim().is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("muse run ended: {outcome}"));
                return MuseStep::Failed(reason);
            }
            MuseStep::Terminal {
                text: payload
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            }
        }
        "run.terminal.failed" | "run.terminal.cancelled" | "run.terminal.canceled" => {
            let reason = payload
                .get("reason")
                .and_then(Value::as_str)
                .filter(|reason| !reason.trim().is_empty())
                .unwrap_or("muse run failed");
            MuseStep::Failed(reason.to_string())
        }
        // The field is `model_id` in the published envelope; `model` is accepted as a fallback so
        // a beta rename does not lose the only record of what actually answered.
        "run.model.configured" => payload
            .get("model_id")
            .or_else(|| payload.get("model"))
            .and_then(Value::as_str)
            .filter(|model| !model.trim().is_empty())
            .map(|model| MuseStep::Model(model.to_string()))
            .unwrap_or(MuseStep::Ignore),
        "tool.result" => MuseStep::Item(tool_item(payload)),
        _ => MuseStep::Ignore,
    }
}

fn terminal_outcome(payload: &Value) -> Option<&str> {
    payload.get("terminal").and_then(|value| {
        value
            .as_str()
            .or_else(|| value.get("status").and_then(Value::as_str))
    })
}

/// A `tool.result` carries no arguments (Muse does not publish them), only the call's outcome and
/// its result. Where the result names a command, the item carries it; otherwise the item is the
/// tool name plus the result preview, never an invented argument list.
fn tool_item(payload: &Value) -> NormalItem {
    let facts = &payload["correlation_facts"];
    let tool = facts["tool_name"].as_str().unwrap_or_default();

    if let Some(path) = payload["edit_facts"]["path"]
        .as_str()
        .filter(|path| !path.is_empty())
    {
        return NormalItem::file_change("write", Some(path));
    }

    let result = &payload["result"];
    let result_text = match result {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    let parsed = result
        .as_str()
        .and_then(|text| serde_json::from_str::<Value>(text).ok());
    let field = |name: &str| {
        parsed
            .as_ref()
            .and_then(|value| value.get(name))
            .and_then(Value::as_str)
    };

    match tool {
        "bash" | "shell" => {
            let command = field("command")
                .or_else(|| field("description"))
                .unwrap_or(tool);
            let output = field("output")
                .or_else(|| field("stdout"))
                .unwrap_or(&result_text);
            let mut body = json!({ "command": command, "output": output });
            if let Some(code) = parsed
                .as_ref()
                .and_then(|value| value.get("exit_code"))
                .and_then(Value::as_i64)
            {
                body["exit_code"] = json!(code);
            }
            NormalItem::new(ItemKind::Command, body)
        }
        name if name.starts_with("mcp__") => {
            let (server, tool) = normalise::mcp_parts(name);
            NormalItem::mcp_call(server, tool, json!({ "result": result_text }))
        }
        "web_search" | "web_fetch" => {
            NormalItem::web_search(field("query").unwrap_or(&result_text))
        }
        _ => NormalItem::unmapped(
            "muse",
            payload.clone(),
            "tool result has no mapped item kind",
        ),
    }
}

#[derive(Debug)]
enum Failure {
    BeforeTurn(String),
    AfterTurn(String),
}

impl Failure {
    fn redacted(self, secrets: &[Secret]) -> Self {
        match self {
            Self::BeforeTurn(reason) => Self::BeforeTurn(redact(&reason, secrets)),
            Self::AfterTurn(reason) => Self::AfterTurn(redact(&reason, secrets)),
        }
    }
}

impl From<Failure> for AdapterError {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::BeforeTurn(reason) | Failure::AfterTurn(reason) => AdapterError::Other(reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(payload_type: &str, payload: Value) -> Value {
        json!({
            "schema_version": 1,
            "payload_type": payload_type,
            "payload_schema_version": 1,
            "payload": payload,
        })
    }

    #[test]
    fn a_terminal_event_is_the_answer_and_a_delta_is_a_fragment() {
        assert_eq!(
            classify(&envelope("run.output.delta", json!({ "text": "po" }))),
            MuseStep::Delta("po".to_string())
        );
        assert_eq!(
            classify(&envelope(
                "run.terminal.completed",
                json!({ "text": "pong", "terminal": "completed" })
            )),
            MuseStep::Terminal {
                text: "pong".to_string()
            }
        );
    }

    #[test]
    fn a_failed_terminal_carries_its_reason_instead_of_an_answer() {
        assert_eq!(
            classify(&envelope(
                "run.terminal.failed",
                json!({ "reason": "model service unavailable" })
            )),
            MuseStep::Failed("model service unavailable".to_string())
        );
        assert_eq!(
            classify(&envelope(
                "run.terminal.completed",
                json!({ "text": "partial", "terminal": "cancelled", "reason": "user cancelled" })
            )),
            MuseStep::Failed("user cancelled".to_string())
        );
    }

    #[test]
    fn the_configured_model_is_kept_in_the_beta_field_names() {
        assert_eq!(
            classify(&envelope(
                "run.model.configured",
                json!({ "model_id": "muse-spark-1.3" })
            )),
            MuseStep::Model("muse-spark-1.3".to_string())
        );
        assert_eq!(
            classify(&envelope(
                "run.model.configured",
                json!({ "model": "muse-wonder-1.0" })
            )),
            MuseStep::Model("muse-wonder-1.0".to_string())
        );
    }

    #[test]
    fn a_tool_result_becomes_a_command_or_a_file_change() {
        let command = classify(&envelope(
            "tool.result",
            json!({
                "correlation_facts": { "tool_name": "bash", "outcome": "success" },
                "result": "{\"command\":\"cargo test\",\"exit_code\":0,\"output\":\"ok\"}"
            }),
        ));
        let MuseStep::Item(command) = command else {
            panic!("expected a command item");
        };
        assert_eq!(command.kind, ItemKind::Command);
        assert_eq!(command.body["command"], "cargo test");
        assert_eq!(command.body["exit_code"], 0);

        let edit = classify(&envelope(
            "tool.result",
            json!({
                "correlation_facts": { "tool_name": "edit", "outcome": "success" },
                "edit_facts": { "path": "src/lib.rs" }
            }),
        ));
        let MuseStep::Item(edit) = edit else {
            panic!("expected a file-change item");
        };
        assert_eq!(edit.kind, ItemKind::FileChange);
        assert_eq!(edit.body["path"], "src/lib.rs");
    }

    #[test]
    fn the_arguments_keep_the_sandbox_and_never_use_yolo_or_trust() {
        let dir = tempfile::tempdir().unwrap();
        let config = MuseConfig::new(dir.path());
        let prompt = dir.path().join("prompt.txt");
        let args = config.arguments("session-1", &prompt);
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--disable-approval", "--user-input-auto-resolve"])
        );
        assert!(!args.iter().any(|arg| arg == "--yolo"));
        assert!(!args.iter().any(|arg| arg == "--disable-sandbox"));
        assert!(!args.iter().any(|arg| arg == "--trust-workspace"));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--session-id", "session-1"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "--prompt-file" && pair[1] == prompt.display().to_string())
        );
    }
}

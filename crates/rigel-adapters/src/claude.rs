//! The Claude Code adapter (P4.6): `claude -p --output-format stream-json`.
//!
//! Claude Code prints NDJSON, one object per line, and the shapes this module understands were
//! read from a real 2.1.251 turn on 21 Oct 2026:
//!
//! ```text
//! {"type":"system","subtype":"init","session_id":"…","apiKeySource":"ANTHROPIC_API_KEY"}
//! {"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"po"}}}
//! {"type":"assistant","message":{"content":[{"type":"text","text":"pong"}]}}
//! {"type":"result","is_error":false,"result":"pong","total_cost_usd":0.00616,"usage":{…}}
//! ```
//!
//! Three rules are enforced here rather than left to the caller:
//!
//! - **A key, never a login (D21).** `--bare` keeps the vendor config out of the turn, and the
//!   `system/init` line's `apiKeySource` has to name an API key. When it does not, the child is
//!   killed at that first event instead of being waited on for an answer Rigel will discard.
//! - **The key never reaches argv.** It is passed in the child's environment, and every piece of
//!   text that leaves this module is passed through [`crate::redact`].
//! - **A silent non-zero exit is a failure, not an empty answer.** An agent that printed nothing
//!   has not answered, whatever its exit status claims.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rigel_core::credentials::{CredentialStore, Secret, get_from};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{Instant as TokioInstant, timeout_at};

use crate::{
    Adapter, AdapterError, AuthMode, Harness, HarnessInfo, credential_spec, redact, registry,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    DontAsk,
    Plan,
    BypassPermissions,
}

impl PermissionMode {
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "acceptEdits",
            Self::DontAsk => "dontAsk",
            Self::Plan => "plan",
            Self::BypassPermissions => "bypassPermissions",
        }
    }
}

#[derive(Clone)]
pub struct ClaudeConfig {
    /// The `claude` binary. Resolved from `PATH` when left as the default.
    pub exe: PathBuf,
    pub model: Option<String>,
    pub cwd: PathBuf,
    pub permission_mode: PermissionMode,
    /// One turn, start to finish.
    pub timeout: Duration,
    /// Existence only: a vendor login means `Subscription`, which Rigel will not run (D21).
    pub vendor_login: Option<PathBuf>,
    /// The child's environment. Values stay wrapped so `Debug` is always redacted.
    pub env: BTreeMap<String, Secret>,
}

impl ClaudeConfig {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            exe: PathBuf::from(registry::exe_name(Harness::Claude)),
            model: None,
            cwd: cwd.into(),
            permission_mode: PermissionMode::DontAsk,
            timeout: Duration::from_secs(300),
            vendor_login: registry::vendor_login(Harness::Claude),
            env: BTreeMap::new(),
        }
    }
}

impl ClaudeConfig {
    /// Resolve this harness's key from a store into the child's environment. The store is
    /// environment-first for real runs; a `MemoryCredentialStore` lets a test prove the path
    /// without touching the OS keychain. Returns false when no key exists — the caller decides
    /// whether that is fatal, and [`ClaudeAdapter::run_turn`] refuses to spawn.
    pub fn inject_credential(&mut self, store: &dyn CredentialStore) -> bool {
        let Some(spec) = credential_spec(Harness::Claude, None) else {
            return false;
        };
        self.env.remove(spec.env_var);
        match get_from(store, &spec.provider) {
            Ok(secret) => {
                self.env.insert(spec.env_var.to_string(), secret);
                true
            }
            Err(_) => false,
        }
    }
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
            .field("vendor_login", &self.vendor_login)
            .field("env", &self.env)
            .finish()
    }
}

/// Progress from one Claude turn. Tool calls and approvals belong to P4.7/P4.10; until then an
/// unknown line is a notice rather than a silent drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeEvent {
    Delta { text: String },
    Message { text: String },
    Notice { message: String },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClaudeUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// `total_cost_usd` from the result line, in micro-dollars.
    pub cost_micros: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ClaudeTurn {
    pub text: String,
    pub session_id: Option<String>,
    pub usage: Option<ClaudeUsage>,
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

    /// How this turn proves who it is. Only a key in the injected environment counts: the vendor
    /// login file is reported as `Subscription`, never silently used.
    pub fn auth_mode(&self) -> AuthMode {
        let Some(spec) = credential_spec(Harness::Claude, None) else {
            return AuthMode::None;
        };
        let has_key = self
            .config
            .env
            .get(spec.env_var)
            .is_some_and(|value| !value.expose().trim().is_empty());
        if has_key {
            AuthMode::ApiKey
        } else if self
            .config
            .vendor_login
            .as_ref()
            .is_some_and(|path| path.is_file())
        {
            AuthMode::Subscription
        } else {
            AuthMode::None
        }
    }

    fn secrets(&self) -> Vec<Secret> {
        self.config.env.values().cloned().collect()
    }

    /// One prompt to completion. Nothing is spawned when the turn has no API key.
    pub async fn run_turn<F>(
        &self,
        prompt: &str,
        mut on_event: F,
    ) -> Result<ClaudeTurn, AdapterError>
    where
        F: FnMut(ClaudeEvent),
    {
        if self.auth_mode() != AuthMode::ApiKey {
            return Err(AdapterError::RequiresApiKey {
                harness: Harness::Claude,
            });
        }

        let secrets = self.secrets();
        let exe = registry::resolve_exe(Harness::Claude, &self.config.exe);
        let mut command = registry::command_for(&exe);
        command
            .current_dir(&self.config.cwd)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            // Inherited: an unread stderr pipe can deadlock a chatty child.
            .stderr(Stdio::inherit());
        command.args([
            "--bare",
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--permission-mode",
            self.config.permission_mode.as_arg(),
        ]);
        if let Some(model) = &self.config.model {
            command.arg("--model").arg(model);
        }
        command.arg(prompt);
        // Nothing from the ambient environment decides who this turn is; only the injected map.
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

        let started = Instant::now();
        let mut child = command
            .spawn()
            .map_err(|error| AdapterError::Other(format!("claude: could not start: {error}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AdapterError::Other("claude: no stdout".to_string()))?;
        let mut lines = BufReader::new(stdout).lines();
        let deadline = TokioInstant::now() + self.config.timeout;

        let mut accrued = String::new();
        let mut answer: Option<String> = None;
        let mut session_id: Option<String> = None;
        let mut usage: Option<ClaudeUsage> = None;
        let mut failure: Option<String> = None;

        loop {
            let line = match timeout_at(deadline, lines.next_line()).await {
                Ok(Ok(Some(line))) => line,
                Ok(Ok(None)) => break,
                Ok(Err(error)) => {
                    return Err(AdapterError::Other(format!(
                        "claude: could not read its output: {error}"
                    )));
                }
                Err(_) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return Err(AdapterError::Other(
                        "claude: the turn timed out and was cancelled".to_string(),
                    ));
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };

            match value.get("type").and_then(Value::as_str) {
                Some("system") => {
                    if let Some(id) = value.get("session_id").and_then(Value::as_str) {
                        session_id = Some(id.to_string());
                    }
                    let source = value
                        .get("apiKeySource")
                        .and_then(Value::as_str)
                        .unwrap_or("none");
                    if !is_api_key_source(source) {
                        // The identity is a subscription: stop the process now rather than let a
                        // turn run that Rigel would have to throw away.
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        return Err(AdapterError::RequiresApiKey {
                            harness: Harness::Claude,
                        });
                    }
                }
                Some("stream_event") => {
                    if let Some(delta) = stream_delta(&value) {
                        let delta = redact(&delta, &secrets);
                        accrued.push_str(&delta);
                        on_event(ClaudeEvent::Delta { text: delta });
                    }
                }
                Some("assistant") => {
                    if let Some(text) = assistant_text(&value) {
                        let text = redact(&text, &secrets);
                        on_event(ClaudeEvent::Message { text });
                    }
                }
                Some("result") => {
                    if let Some(id) = value.get("session_id").and_then(Value::as_str) {
                        session_id = Some(id.to_string());
                    }
                    usage = usage_of(&value);
                    let is_error = value
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let reported = value
                        .get("result")
                        .and_then(Value::as_str)
                        .map(|text| redact(text, &secrets));
                    if is_error {
                        failure = Some(
                            reported.unwrap_or_else(|| "the harness reported an error".to_string()),
                        );
                    } else if let Some(text) = reported {
                        answer = Some(text);
                    }
                }
                Some("error") => {
                    failure = Some(
                        value
                            .get("message")
                            .and_then(Value::as_str)
                            .map(|message| redact(message, &secrets))
                            .unwrap_or_else(|| "the harness reported an error".to_string()),
                    );
                }
                Some(other) => on_event(ClaudeEvent::Notice {
                    message: format!("claude: unmapped line `{other}`"),
                }),
                None => {}
            }
        }

        let status = child.wait().await.map_err(|error| {
            AdapterError::Other(format!("claude: did not exit cleanly: {error}"))
        })?;

        if let Some(message) = failure {
            return Err(AdapterError::Other(format!("claude: {message}")));
        }

        let text = answer.unwrap_or(accrued);
        if text.trim().is_empty() {
            return Err(AdapterError::Other(format!(
                "claude produced no message (status {status})"
            )));
        }
        if !status.success() {
            return Err(AdapterError::Other(format!(
                "claude exited with {status}: {text}"
            )));
        }

        Ok(ClaudeTurn {
            text,
            session_id,
            usage,
            duration: started.elapsed(),
        })
    }
}

#[async_trait]
impl Adapter for ClaudeAdapter {
    fn harness(&self) -> Harness {
        Harness::Claude
    }

    async fn probe(&self) -> Result<HarnessInfo, AdapterError> {
        let exe = registry::resolve_exe(Harness::Claude, &self.config.exe);
        let installed = exe.is_file();
        let version = if installed {
            registry::version_of(&exe).await
        } else {
            None
        };
        Ok(HarnessInfo {
            harness: Harness::Claude,
            installed,
            version: version.clone(),
            path: installed.then_some(exe),
            auth_mode: self.auth_mode(),
            capabilities: registry::capabilities(Harness::Claude, version.as_deref()),
            provider: credential_spec(Harness::Claude, None).map(|spec| spec.provider.to_string()),
            install_hint: (!installed).then(|| registry::install_hint(Harness::Claude)),
        })
    }
}

/// `ANTHROPIC_API_KEY` is a key. `none`, `oauth` and a bare account name are not, and Rigel does
/// not run a turn on them (D21).
fn is_api_key_source(source: &str) -> bool {
    source.to_ascii_uppercase().contains("API_KEY")
}

fn stream_delta(value: &Value) -> Option<String> {
    let event = value.get("event")?;
    if event.get("type").and_then(Value::as_str) != Some("content_block_delta") {
        return None;
    }
    let delta = event.get("delta")?;
    if delta.get("type").and_then(Value::as_str) != Some("text_delta") {
        return None;
    }
    delta
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn assistant_text(value: &Value) -> Option<String> {
    let blocks = value.get("message")?.get("content")?.as_array()?;
    let texts: Vec<&str> = blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect();
    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

fn usage_of(value: &Value) -> Option<ClaudeUsage> {
    let usage = value.get("usage")?;
    let number = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    Some(ClaudeUsage {
        input_tokens: number("input_tokens"),
        output_tokens: number("output_tokens"),
        cost_micros: value
            .get("total_cost_usd")
            .and_then(Value::as_f64)
            .map(|cost| (cost * 1_000_000.0).round() as i64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_api_key_source_counts_as_a_key() {
        assert!(is_api_key_source("ANTHROPIC_API_KEY"));
        assert!(!is_api_key_source("none"));
        assert!(!is_api_key_source("oauth"));
        assert!(!is_api_key_source("claude.ai"));
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
    fn a_key_can_arrive_from_a_store_without_the_environment() {
        let store =
            rigel_core::credentials::MemoryCredentialStore::with("anthropic", "sk-ant-memory");
        let mut config = ClaudeConfig::new(".");
        config.vendor_login = None;
        assert!(config.inject_credential(&store));
        assert_eq!(ClaudeAdapter::new(config).auth_mode(), AuthMode::ApiKey);

        let mut empty = ClaudeConfig::new(".");
        empty.vendor_login = None;
        assert!(!empty.inject_credential(&rigel_core::credentials::MemoryCredentialStore::new()));
        assert_eq!(ClaudeAdapter::new(empty).auth_mode(), AuthMode::None);
    }

    #[test]
    fn a_result_line_carries_the_cost_in_micro_dollars() {
        let value = serde_json::json!({
            "usage": { "input_tokens": 1207, "output_tokens": 5 },
            "total_cost_usd": 0.00616
        });
        let usage = usage_of(&value).unwrap();
        assert_eq!(usage.input_tokens, 1207);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.cost_micros, Some(6160));
    }
}

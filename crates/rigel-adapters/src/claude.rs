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
//! Approvals go through `--permission-prompt-tool mcp__rigel__rigel_approve`, with the Rigel MCP
//! server supplied by `--mcp-config` (P4.10). Claude then asks Rigel instead of failing closed;
//! Rigel's tool answers `allow` or `deny` from the shared scope store, and a question nobody
//! answers becomes `deny`. Without that wiring a tool that needs permission still fails closed.
//!
//! Events observed against Claude Code 2.1.251 on 21 Oct 2026:
//! `system/init` (session id, model, `apiKeySource`), `stream_event` wrapping Anthropic SSE
//! (`content_block_delta` with `text_delta`), a final `assistant` message, and `result` with
//! `usage` and `total_cost_usd`.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use rigel_core::ItemKind;
use rigel_core::credentials::{CredentialStore, Secret, get_from};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{ChildStdout, Command};
use tokio::time::{Instant, timeout_at};

use crate::normalise::{self, NormalItem};
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
    /// A generated MCP config naming Rigel's own stdio server. Required for approvals.
    pub mcp_config: Option<PathBuf>,
    /// The permission tool to name, e.g. `mcp__rigel__rigel_approve`. Only used with
    /// `mcp_config`; both or neither.
    pub permission_prompt_tool: Option<String>,
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
            .field("mcp_config", &self.mcp_config)
            .field("permission_prompt_tool", &self.permission_prompt_tool)
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
            mcp_config: None,
            permission_prompt_tool: None,
        }
    }

    /// Name the permission tool Rigel answers, and the MCP config that defines it. Passing
    /// only one of the two is a wiring mistake, so the pair is set together.
    pub fn with_permission_tool(
        mut self,
        mcp_config: impl Into<PathBuf>,
        tool: impl Into<String>,
    ) -> Self {
        self.mcp_config = Some(mcp_config.into());
        self.permission_prompt_tool = Some(tool.into());
        self
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
    /// A non-text block — a reasoning trace, a tool call, a tool result — already sorted into one
    /// of the ten item kinds (§5.2, P4.8).
    Item(NormalItem),
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
        // Both arguments are needed for the permission round trip: the config defines the
        // server, the tool name says which of its tools answers a permission question.
        if let (Some(config), Some(tool)) =
            (&self.config.mcp_config, &self.config.permission_prompt_tool)
        {
            command.arg("--mcp-config").arg(config);
            command.arg("--permission-prompt-tool").arg(tool);
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
        // Tool calls whose results arrive later, so a result inherits the kind of the call that
        // produced it instead of being guessed from its own shape.
        let mut tool_calls: HashMap<String, ItemKind> = HashMap::new();

        loop {
            let line = next_line(&mut lines, deadline, emitted).await?;
            let Some(line) = line else {
                break;
            };
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };

            for step in classify(&value) {
                match step {
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
                    Step::ToolCall { id, item } => {
                        if let Some(id) = id.as_ref().filter(|_| !item.is_unmapped()) {
                            tool_calls.insert(id.clone(), item.kind);
                        }
                        on_event(ClaudeEvent::Item(redacted(item, &secrets)));
                    }
                    Step::Item(item) => on_event(ClaudeEvent::Item(redacted(item, &secrets))),
                    Step::ToolResult {
                        tool_use_id,
                        text: result,
                        is_error,
                        raw,
                    } => {
                        let item = match tool_calls.get(&tool_use_id) {
                            Some(kind) => NormalItem::new(
                                *kind,
                                json!({
                                    "tool_use_id": tool_use_id,
                                    "ok": !is_error,
                                    "result": result,
                                }),
                            ),
                            None => NormalItem::unmapped(
                                "claude",
                                raw,
                                "a tool result whose call produced no item kind",
                            ),
                        };
                        on_event(ClaudeEvent::Item(redacted(item, &secrets)));
                    }
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
                }
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
    /// A tool call, carrying its id so the matching `tool_result` can inherit its kind.
    ToolCall {
        id: Option<String>,
        item: NormalItem,
    },
    /// A block that is already one item and needs no pairing (a reasoning trace).
    Item(NormalItem),
    ToolResult {
        tool_use_id: String,
        text: String,
        is_error: bool,
        raw: Value,
    },
    Result {
        text: Option<String>,
        is_error: bool,
        subtype: String,
        usage: Option<ClaudeUsage>,
        session_id: Option<String>,
    },
}

fn classify(value: &Value) -> Vec<Step> {
    match value.get("type").and_then(Value::as_str) {
        Some("system") => match value.get("subtype").and_then(Value::as_str) {
            // Even a malformed init still carries the auth source, so it is classified rather
            // than dropped: a missing field must not become a licence to use the vendor login.
            Some("init") => vec![Step::Init {
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
            }],
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
                vec![Step::Notice(message)]
            }
            _ => Vec::new(),
        },
        Some("stream_event") => {
            let event = &value["event"];
            if event.get("type").and_then(Value::as_str) != Some("content_block_delta") {
                return Vec::new();
            }
            let delta = &event["delta"];
            match delta.get("type").and_then(Value::as_str) {
                Some("text_delta") => delta
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| vec![Step::Delta(text.to_string())])
                    .unwrap_or_default(),
                // A reasoning trace streams the same way the answer does; it is a `reasoning`
                // item once it settles, so the delta is not folded into the answer text.
                Some("thinking_delta") => delta
                    .get("thinking")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map(|text| vec![Step::Item(NormalItem::reasoning(text))])
                    .unwrap_or_default(),
                _ => Vec::new(),
            }
        }
        Some("assistant") => assistant_steps(&value["message"]["content"]),
        // A `user` line in a stream-json transcript is a tool result travelling back to the model.
        Some("user") => tool_result_steps(&value["message"]["content"]),
        Some("result") => vec![Step::Result {
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
        }],
        _ => Vec::new(),
    }
}

/// One `assistant` message can carry text, thinking and any number of tool calls at once, so it
/// decodes to a list of steps rather than a single one.
fn assistant_steps(content: &Value) -> Vec<Step> {
    let blocks = content.as_array().cloned().unwrap_or_default();
    let mut steps = Vec::new();
    let text = blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        steps.push(Step::Message(text));
    }
    for block in &blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("thinking") => {
                if let Some(thinking) = block
                    .get("thinking")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    steps.push(Step::Item(NormalItem::reasoning(thinking)));
                }
            }
            Some("tool_use") => {
                let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                steps.push(Step::ToolCall {
                    id: block.get("id").and_then(Value::as_str).map(str::to_string),
                    item: tool_item(
                        name,
                        block.get("id").and_then(Value::as_str),
                        &block["input"],
                    ),
                });
            }
            _ => {}
        }
    }
    steps
}

fn tool_result_steps(content: &Value) -> Vec<Step> {
    let mut steps = Vec::new();
    for block in content.as_array().cloned().unwrap_or_default() {
        if block.get("type").and_then(Value::as_str) != Some("tool_result") {
            continue;
        }
        steps.push(Step::ToolResult {
            tool_use_id: block
                .get("tool_use_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            text: match block.get("content") {
                Some(Value::String(text)) => text.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            },
            is_error: block
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            raw: block.clone(),
        });
    }
    steps
}

/// One `tool_use` block as one of the ten kinds. An unknown builtin is kept as an unmapped error
/// with its input intact rather than forced into a neighbouring kind (P4.8).
fn tool_item(name: &str, id: Option<&str>, input: &Value) -> NormalItem {
    let string = |key: &str| input.get(key).and_then(Value::as_str);
    let Some((kind, operation)) = normalise::claude_tool(name) else {
        return NormalItem::unmapped(
            "claude",
            json!({ "tool": name, "input": input }),
            "no item kind for this tool",
        );
    };
    let mut item = match kind {
        ItemKind::Command => NormalItem::command(string("command").unwrap_or(name)),
        ItemKind::FileChange => NormalItem::file_change(
            operation,
            string("file_path")
                .or_else(|| string("notebook_path"))
                .or_else(|| string("path")),
        ),
        ItemKind::WebSearch => NormalItem::web_search(
            string("query")
                .or_else(|| string("url"))
                .unwrap_or_default(),
        ),
        ItemKind::McpCall => {
            let (server, tool) = normalise::mcp_parts(name);
            NormalItem::mcp_call(server, tool, input.clone())
        }
        ItemKind::Plan => NormalItem::plan(plan_entries(input)),
        _ => NormalItem::new(kind, input.clone()),
    };
    if let Some(id) = id {
        item = item.with(json!({ "tool_use_id": id }));
    }
    item.with(json!({ "tool": name, "operation": operation, "input": input }))
}

/// `TodoWrite` carries structured todos; `Task` carries a description. Both are the agent saying
/// what it intends to do, which is what a `plan` item is for.
fn plan_entries(input: &Value) -> Vec<String> {
    let todos = input
        .get("todos")
        .and_then(Value::as_array)
        .map(|todos| {
            todos
                .iter()
                .filter_map(|todo| todo.get("content").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !todos.is_empty() {
        return todos;
    }
    ["description", "prompt"]
        .iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str))
        .map(|text| vec![text.to_string()])
        .unwrap_or_default()
}

/// Item bodies carry raw tool input, so the credential rule applies before one leaves the adapter.
fn redacted(item: NormalItem, secrets: &[Secret]) -> NormalItem {
    NormalItem::new(item.kind, crate::redact_json(&item.body, secrets))
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
            vec![Step::Init {
                session_id: "s-1".to_string(),
                model: Some("free-stack".to_string()),
                api_key_source: Some("ANTHROPIC_API_KEY".to_string()),
            }]
        );

        let delta = json!({
            "type": "stream_event",
            "event": {
                "type": "content_block_delta",
                "index": 0,
                "delta": { "type": "text_delta", "text": "pong" }
            }
        });
        assert_eq!(classify(&delta), vec![Step::Delta("pong".to_string())]);

        let assistant = json!({
            "type": "assistant",
            "message": { "content": [{ "type": "text", "text": "pong" }] }
        });
        assert_eq!(
            classify(&assistant),
            vec![Step::Message("pong".to_string())]
        );

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
            vec![Step::Result {
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
            }]
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
        let steps = classify(&result);
        let [
            Step::Result {
                is_error, subtype, ..
            },
        ] = &steps[..]
        else {
            panic!("expected a result step");
        };
        assert!(*is_error);
        assert_eq!(subtype, "error_during_execution");
    }

    #[test]
    fn an_assistant_message_decodes_its_text_thinking_and_tool_calls() {
        let assistant = json!({
            "type": "assistant",
            "message": { "content": [
                { "type": "text", "text": "Reading src/lib.rs." },
                { "type": "thinking", "thinking": "the file is small" },
                { "type": "tool_use", "id": "toolu_01", "name": "Read", "input": { "file_path": "src/lib.rs" } },
                { "type": "tool_use", "id": "toolu_02", "name": "Bash", "input": { "command": "cargo test" } },
                { "type": "tool_use", "id": "toolu_03", "name": "mcp__files__read", "input": { "path": "README.md" } }
            ]}
        });
        let steps = classify(&assistant);
        assert_eq!(steps[0], Step::Message("Reading src/lib.rs.".into()));
        let [
            Step::Item(reasoning),
            Step::ToolCall { item: read, .. },
            Step::ToolCall { item: bash, .. },
            Step::ToolCall { item: mcp, .. },
        ] = &steps[1..]
        else {
            panic!("expected reasoning and three tool calls, got {steps:?}");
        };
        assert_eq!(reasoning.kind, ItemKind::Reasoning);
        assert_eq!(reasoning.body["text"], "the file is small");
        assert_eq!(read.kind, ItemKind::FileChange);
        assert_eq!(read.body["operation"], "read");
        assert_eq!(read.body["path"], "src/lib.rs");
        assert_eq!(read.body["tool_use_id"], "toolu_01");
        assert_eq!(bash.kind, ItemKind::Command);
        assert_eq!(bash.body["command"], "cargo test");
        assert_eq!(mcp.kind, ItemKind::McpCall);
        assert_eq!(mcp.body["server"], "files");
        assert_eq!(mcp.body["tool"], "read");
    }

    #[test]
    fn an_unknown_tool_is_unmapped_rather_than_guessed() {
        let assistant = json!({
            "type": "assistant",
            "message": { "content": [
                { "type": "tool_use", "id": "toolu_09", "name": "TimeTravel", "input": { "year": 1999 } }
            ]}
        });
        let [Step::ToolCall { item, .. }] = &classify(&assistant)[..] else {
            panic!("expected one tool call");
        };
        assert!(item.is_unmapped());
        assert_eq!(item.kind, ItemKind::Error);
        assert_eq!(item.body["raw"]["input"]["year"], 1999);
    }

    #[test]
    fn a_tool_result_carries_the_id_its_call_will_be_matched_by() {
        let user = json!({
            "type": "user",
            "message": { "content": [
                { "type": "tool_result", "tool_use_id": "toolu_01", "content": "fn main() {}" },
                { "type": "tool_result", "tool_use_id": "toolu_02", "content": "boom", "is_error": true }
            ]}
        });
        let steps = classify(&user);
        assert_eq!(steps.len(), 2);
        let [
            Step::ToolResult {
                tool_use_id,
                text,
                is_error,
                ..
            },
            Step::ToolResult {
                tool_use_id: failed,
                is_error: failed_flag,
                ..
            },
        ] = &steps[..]
        else {
            panic!("expected two tool results");
        };
        assert_eq!(tool_use_id, "toolu_01");
        assert_eq!(text, "fn main() {}");
        assert!(!is_error);
        assert_eq!(failed, "toolu_02");
        assert!(failed_flag);
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

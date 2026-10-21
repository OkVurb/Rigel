//! The Agent Client Protocol client shared by Cursor and Cline (P4.3).
//!
//! ACP is JSON-RPC 2.0 over the child's stdio: the client calls `initialize`, `session/new` and
//! `session/prompt`, the agent answers `session/update` notifications, and either side may make a
//! request. The one client method that matters for Rigel is the agent's
//! `session/request_permission`, which is routed to an [`Approver`] instead of being answered
//! optimistically (N2).
//!
//! Shapes below follow the published v1 schema (`agentclientprotocol.com/protocol/v1`):
//!
//! ```text
//! → initialize {protocolVersion: 1, clientCapabilities: {}, clientInfo: {...}}
//! → session/new {cwd: "<absolute>", mcpServers: []}             → {sessionId}
//! → session/prompt {sessionId, prompt: [{type: "text", text}]}  → {stopReason}
//! ← session/update {sessionId, update: {sessionUpdate: "..."}}
//! ← session/request_permission {sessionId, toolCall, options}   → {outcome}
//! → session/cancel {sessionId}                                  (notification)
//! ```
//!
//! Rigel advertises no filesystem or terminal capability: the harness runs its own tools, so there
//! is nothing for the client to proxy, and a smaller capability surface is a smaller exploit
//! surface. Unrecognised session updates surface as [`AcpEvent::Unmapped`] rather than being
//! dropped — the hook P4.8 builds its ten-item normalisation on.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rigel_core::credentials::{CredentialStore, Secret, get_from};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::mpsc;
use tokio::time::{Instant as TokioInstant, timeout_at};

use crate::{Adapter, AdapterError, AuthMode, Harness, HarnessInfo, credential_spec};

/// The only protocol major version Rigel speaks. A different version is a hard failure, not a
/// best-effort parse: guessing would hide real breakage.
pub const PROTOCOL_VERSION: u64 = 1;

#[derive(Clone)]
pub struct AcpConfig {
    pub harness: Harness,
    pub exe: PathBuf,
    /// The agent's ACP invocation: `agent acp` for Cursor, `cline --acp` for Cline.
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    /// Per-agent process environment. Values stay wrapped so `Debug` is always redacted.
    pub env: BTreeMap<String, Secret>,
    /// Existence only: a vendor login means `Subscription`, which Rigel will not spawn (D21).
    pub vendor_login: Option<PathBuf>,
}

impl fmt::Debug for AcpConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcpConfig")
            .field("harness", &self.harness)
            .field("exe", &self.exe)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("timeout", &self.timeout)
            .field("env", &self.env)
            .field("vendor_login", &self.vendor_login)
            .finish()
    }
}

impl AcpConfig {
    pub fn new(harness: Harness, cwd: impl Into<PathBuf>) -> Self {
        let (exe, args) = match harness {
            Harness::Cline => (
                PathBuf::from(if cfg!(windows) { "cline.cmd" } else { "cline" }),
                vec!["--acp".to_string()],
            ),
            // `agent acp`. The probe also accepts the older `cursor-agent` name.
            _ => (
                PathBuf::from(if cfg!(windows) { "agent.exe" } else { "agent" }),
                vec!["acp".to_string()],
            ),
        };
        Self {
            harness,
            exe,
            args,
            cwd: cwd.into(),
            timeout: Duration::from_secs(300),
            env: BTreeMap::new(),
            vendor_login: crate::registry::vendor_login(harness),
        }
    }

    pub fn cursor(cwd: impl Into<PathBuf>) -> Self {
        Self::new(Harness::Cursor, cwd)
    }

    pub fn cline(cwd: impl Into<PathBuf>) -> Self {
        Self::new(Harness::Cline, cwd)
    }

    /// Resolve this harness's key into the child's environment. Cline has no provider key before
    /// P4.4 writes its own config, so it never gains one from here.
    pub fn inject_credential(&mut self, store: &dyn CredentialStore) -> bool {
        let Some(spec) = credential_spec(self.harness, None) else {
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

    fn secrets(&self) -> Vec<Secret> {
        self.env.values().cloned().collect()
    }
}

/// One option the agent offered for an approval, straight from its `options` array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalOption {
    pub option_id: String,
    pub name: String,
    pub kind: String,
}

/// What the agent wants to do, as much as the protocol says. `raw_input` is the tool's argument
/// object; P4.10 renders it verbatim beside the scope buttons.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalRequest {
    pub tool_call_id: String,
    pub title: String,
    pub kind: Option<String>,
    pub raw_input: Option<Value>,
    pub options: Vec<ApprovalOption>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    /// The turn was interrupted, so ACP requires the `cancelled` outcome.
    Cancel,
}

impl ApprovalDecision {
    fn wants(self) -> &'static str {
        match self {
            Self::AllowOnce => "allow_once",
            Self::AllowAlways => "allow_always",
            Self::RejectOnce => "reject_once",
            Self::RejectAlways => "reject_always",
            Self::Cancel => "",
        }
    }
}

/// Who answers `session/request_permission`. The UI implements this in P4.10; the default refuses
/// every request, so an unattended run fails closed.
#[async_trait]
pub trait Approver: Send + Sync {
    async fn approve(&self, request: &ApprovalRequest) -> ApprovalDecision;
}

/// Refuses everything. The safe default for a client with no approval surface yet.
pub struct DenyAll;

#[async_trait]
impl Approver for DenyAll {
    async fn approve(&self, _request: &ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::RejectOnce
    }
}

/// Progress normalised from `session/update` and from the permission round-trip.
#[derive(Debug, Clone, PartialEq)]
pub enum AcpEvent {
    MessageDelta {
        text: String,
    },
    ThoughtDelta {
        text: String,
    },
    ToolCall {
        tool_call_id: String,
        title: String,
        kind: Option<String>,
        status: Option<String>,
        raw_input: Option<Value>,
    },
    ToolCallUpdate {
        tool_call_id: String,
        status: Option<String>,
        text: Option<String>,
    },
    Plan {
        entries: Vec<String>,
    },
    Usage {
        used: u64,
        size: u64,
        cost_micros: Option<i64>,
    },
    Notice {
        message: String,
    },
    PermissionRequested {
        request: ApprovalRequest,
    },
    /// The decision the approver returned, echoed so a transcript can record consent.
    PermissionResolved {
        tool_call_id: String,
        decision: ApprovalDecision,
    },
    /// A valid update Rigel does not model yet. Kept, never silently dropped (P4.8).
    Unmapped {
        kind: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AcpUsage {
    pub used: u64,
    pub size: u64,
    pub cost_micros: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AcpTurn {
    pub text: String,
    pub stop_reason: String,
    pub usage: Option<AcpUsage>,
    pub duration: Duration,
}

enum Incoming {
    Response {
        id: u64,
        result: Result<Value, RpcError>,
    },
    Update {
        update: Value,
        session_id: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct RpcError {
    message: String,
}

impl RpcError {
    fn from_value(value: &Value) -> Self {
        Self {
            message: value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("agent error")
                .to_string(),
        }
    }
}

pub struct AcpClient {
    harness: Harness,
    cwd: PathBuf,
    child: Child,
    outbound: mpsc::UnboundedSender<String>,
    incoming: mpsc::UnboundedReceiver<Incoming>,
    next_id: AtomicU64,
    secrets: Vec<Secret>,
    timeout: Duration,
    session_id: Option<String>,
    finished: bool,
}

impl fmt::Debug for AcpClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcpClient")
            .field("harness", &self.harness)
            .field("pid", &self.child.id())
            .field("session_id", &self.session_id)
            .field("secrets", &self.secrets)
            .finish()
    }
}

impl AcpClient {
    /// Spawn the agent and complete `initialize`. Nothing is sent before the handshake succeeds.
    pub async fn spawn(
        config: AcpConfig,
        approver: Arc<dyn Approver>,
    ) -> Result<Self, AdapterError> {
        if !config.cwd.is_absolute() {
            return Err(AdapterError::Other(format!(
                "ACP requires an absolute cwd, got {}",
                config.cwd.display()
            )));
        }
        if acp_auth_mode(&config) != AuthMode::ApiKey {
            return Err(AdapterError::RequiresApiKey {
                harness: config.harness,
            });
        }
        let secrets = config.secrets();
        let exe = crate::registry::resolve_exe(config.harness, &config.exe);
        let mut command = crate::registry::command_for(&exe);
        command
            .args(&config.args)
            .current_dir(&config.cwd)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited: an unread stderr pipe can deadlock a chatty agent.
            .stderr(Stdio::inherit());
        command
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("OPENAI_API_KEY")
            .env_remove("CURSOR_API_KEY")
            .envs(
                config
                    .env
                    .iter()
                    .map(|(name, value)| (name, value.expose())),
            );

        let mut child = command.spawn().map_err(|error| {
            AdapterError::Other(format!(
                "{}: could not start: {error}",
                crate::registry::display_name(config.harness)
            ))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AdapterError::Other("acp: no stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AdapterError::Other("acp: no stdout".to_string()))?;

        let (outbound, outbound_rx) = mpsc::unbounded_channel();
        let (incoming_tx, incoming) = mpsc::unbounded_channel();
        tokio::spawn(writer(stdin, outbound_rx));
        tokio::spawn(reader(
            stdout,
            incoming_tx,
            outbound.clone(),
            approver,
            secrets.clone(),
        ));

        let mut client = Self {
            harness: config.harness,
            cwd: config.cwd.clone(),
            child,
            outbound,
            incoming,
            next_id: AtomicU64::new(1),
            secrets,
            timeout: config.timeout,
            session_id: None,
            finished: false,
        };

        let result = client
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "clientCapabilities": {},
                    "clientInfo": {
                        "name": "rigel",
                        "title": "Rigel",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )
            .await?;

        let negotiated = result
            .get("protocolVersion")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if negotiated != PROTOCOL_VERSION {
            client.kill().await;
            return Err(AdapterError::Other(format!(
                "acp: agent speaks protocol {negotiated}, Rigel speaks {PROTOCOL_VERSION}"
            )));
        }
        Ok(client)
    }

    pub fn harness(&self) -> Harness {
        self.harness
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// `session/new`. The cwd is absolute by construction; a relative one was refused at spawn.
    pub async fn new_session(&mut self) -> Result<String, AdapterError> {
        let cwd = self.cwd.display().to_string();
        let result = self
            .request("session/new", json!({ "cwd": cwd, "mcpServers": [] }))
            .await?;
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AdapterError::Other("acp: session/new returned no sessionId".to_string())
            })?
            .to_string();
        self.session_id = Some(session_id.clone());
        Ok(session_id)
    }

    /// One prompt turn. Returns when the agent answers with a stop reason.
    pub async fn prompt<F>(&mut self, text: &str, mut on_event: F) -> Result<AcpTurn, AdapterError>
    where
        F: FnMut(AcpEvent),
    {
        let session_id = self.session_id.clone().ok_or_else(|| {
            AdapterError::Other("acp: no session; call new_session first".to_string())
        })?;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/prompt",
            "params": {
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }]
            }
        }))?;

        let started = Instant::now();
        let deadline = TokioInstant::now() + self.timeout;
        let mut text_out = String::new();
        let mut usage = None;
        let stop_reason;

        loop {
            let message = match timeout_at(deadline, self.incoming.recv()).await {
                Ok(Some(message)) => message,
                Ok(None) => {
                    let status = self.child.try_wait().ok().flatten();
                    return Err(AdapterError::Other(format!(
                        "acp: {} closed the connection{}",
                        crate::registry::display_name(self.harness),
                        status
                            .map(|status| format!(" ({status})"))
                            .unwrap_or_default()
                    )));
                }
                Err(_) => {
                    self.cancel().await;
                    // Give the agent a bounded moment to acknowledge: its prompt response is the
                    // only proof the cancel arrived before this client is dropped.
                    let grace = TokioInstant::now() + Duration::from_secs(2);
                    while let Ok(Some(Incoming::Response { id: got, .. })) =
                        timeout_at(grace, self.incoming.recv()).await
                    {
                        if got == id {
                            break;
                        }
                    }
                    return Err(AdapterError::Other(
                        "acp: the turn timed out and was cancelled".to_string(),
                    ));
                }
            };

            match message {
                Incoming::Response { id: got, result } if got == id => match result {
                    Ok(value) => {
                        stop_reason = Some(
                            value
                                .get("stopReason")
                                .and_then(Value::as_str)
                                .unwrap_or("end_turn")
                                .to_string(),
                        );
                        break;
                    }
                    Err(error) => {
                        return Err(AdapterError::Other(format!(
                            "acp: {} failed: {}",
                            crate::registry::display_name(self.harness),
                            self.redact(&error.message)
                        )));
                    }
                },
                Incoming::Response { id: other, .. } => {
                    // A late response to an earlier, already-timed-out request. It carries nothing
                    // this turn needs, and treating it as a protocol error would fail the turn that
                    // is still running.
                    let _ = other;
                }
                Incoming::Update {
                    update,
                    session_id: from,
                } => {
                    if from.as_deref().is_some_and(|from| from != session_id) {
                        continue;
                    }
                    if let Some(event) = self.normalise(update, &mut text_out) {
                        if let AcpEvent::Usage {
                            used,
                            size,
                            cost_micros,
                        } = &event
                        {
                            usage = Some(AcpUsage {
                                used: *used,
                                size: *size,
                                cost_micros: *cost_micros,
                            });
                        }
                        on_event(event);
                    }
                }
            }
        }

        Ok(AcpTurn {
            text: self.redact(&text_out),
            stop_reason: stop_reason.unwrap_or_else(|| "end_turn".to_string()),
            usage,
            duration: started.elapsed(),
        })
    }

    /// `session/cancel`, best effort. The agent still owes a prompt response with the cancelled
    /// stop reason; the caller has already stopped reading at that point.
    pub async fn cancel(&mut self) {
        if let Some(session_id) = &self.session_id {
            let _ = self.send(json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": session_id }
            }));
        }
    }

    pub async fn kill(&mut self) {
        self.finished = true;
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }

    fn send(&self, message: Value) -> Result<(), AdapterError> {
        let line = self.redact(&message.to_string());
        self.outbound
            .send(line)
            .map_err(|_| AdapterError::Other("acp: the agent is gone".to_string()))
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, AdapterError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;

        let deadline = TokioInstant::now() + self.timeout;
        loop {
            match timeout_at(deadline, self.incoming.recv()).await {
                Ok(Some(Incoming::Response { id: got, result })) if got == id => {
                    return result.map_err(|error| {
                        AdapterError::Other(format!(
                            "acp: {method} failed: {}",
                            self.redact(&error.message)
                        ))
                    });
                }
                Ok(Some(_)) => continue,
                Ok(None) => {
                    return Err(AdapterError::Other(format!(
                        "acp: the agent exited before answering {method}"
                    )));
                }
                Err(_) => {
                    return Err(AdapterError::Other(format!("acp: {method} timed out")));
                }
            }
        }
    }

    fn redact(&self, value: &str) -> String {
        crate::redact(value, &self.secrets)
    }

    fn normalise(&self, update: Value, text: &mut String) -> Option<AcpEvent> {
        let kind = update
            .get("sessionUpdate")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let text_of = |value: &Value| value.as_str().map(|text| self.redact(text));
        match kind.as_str() {
            "agent_message_chunk" => {
                let chunk = text_of(&update["content"]["text"])?;
                text.push_str(&chunk);
                Some(AcpEvent::MessageDelta { text: chunk })
            }
            "agent_thought_chunk" => Some(AcpEvent::ThoughtDelta {
                text: text_of(&update["content"]["text"]).unwrap_or_default(),
            }),
            "tool_call" => Some(AcpEvent::ToolCall {
                tool_call_id: update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                title: update
                    .get("title")
                    .and_then(Value::as_str)
                    .map(|title| self.redact(title))
                    .unwrap_or_default(),
                kind: update
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                status: update
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                raw_input: update.get("rawInput").cloned(),
            }),
            "tool_call_update" => Some(AcpEvent::ToolCallUpdate {
                tool_call_id: update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                status: update
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                text: content_text(&update["content"]).map(|text| self.redact(&text)),
            }),
            "plan" => Some(AcpEvent::Plan {
                entries: update["entries"]
                    .as_array()
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|entry| entry.get("content").and_then(Value::as_str))
                            .map(|content| self.redact(content))
                            .collect()
                    })
                    .unwrap_or_default(),
            }),
            "usage_update" => Some(AcpEvent::Usage {
                used: update.get("used").and_then(Value::as_u64).unwrap_or(0),
                size: update.get("size").and_then(Value::as_u64).unwrap_or(0),
                cost_micros: update["cost"]["amount"]
                    .as_f64()
                    .map(|amount| (amount * 1_000_000.0).round() as i64)
                    .filter(|_| {
                        update["cost"]["currency"]
                            .as_str()
                            .is_some_and(|currency| currency.eq_ignore_ascii_case("USD"))
                    }),
            }),
            "notice" => Some(AcpEvent::Notice {
                message: text_of(&update["title"]).unwrap_or_else(|| "notice".to_string()),
            }),
            "available_commands_update"
            | "current_mode_update"
            | "config_option_update"
            | "session_info_update" => Some(AcpEvent::Notice {
                message: format!("session state changed: {kind}"),
            }),
            other => Some(AcpEvent::Unmapped {
                kind: other.to_string(),
            }),
        }
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        // `kill_on_drop` handles the process; this only makes the intent explicit.
        if !self.finished {
            let _ = self.child.start_kill();
        }
    }
}

/// How an ACP harness authenticates. Cline has no provider key before P4.4, so it reports `None`
/// even when its CLI is logged in — which is why Rigel will not spawn it yet.
pub(crate) fn acp_auth_mode(config: &AcpConfig) -> AuthMode {
    let Some(spec) = credential_spec(config.harness, None) else {
        return AuthMode::None;
    };
    let has_key = config
        .env
        .get(spec.env_var)
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

/// Probe one ACP harness without entering ACP mode: `--version`, then auth classification.
pub async fn probe_acp(config: &AcpConfig) -> HarnessInfo {
    let exe = crate::registry::resolve_exe(config.harness, &config.exe);
    let installed = exe.is_file();
    let version = if installed {
        crate::registry::version_of(&exe).await
    } else {
        None
    };
    HarnessInfo {
        harness: config.harness,
        installed,
        version: version.clone(),
        path: installed.then_some(exe),
        auth_mode: acp_auth_mode(config),
        capabilities: crate::registry::capabilities(config.harness, version.as_deref()),
        provider: credential_spec(config.harness, None).map(|spec| spec.provider.to_string()),
        install_hint: (!installed).then(|| crate::registry::install_hint(config.harness)),
    }
}

#[async_trait]
impl Adapter for AcpConfig {
    fn harness(&self) -> Harness {
        self.harness
    }

    async fn probe(&self) -> Result<HarnessInfo, AdapterError> {
        Ok(probe_acp(self).await)
    }
}

fn content_text(content: &Value) -> Option<String> {
    let blocks = content.as_array()?;
    let texts: Vec<String> = blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("content"))
        .filter_map(|block| block["content"]["text"].as_str().map(str::to_string))
        .collect();
    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

async fn writer(mut stdin: ChildStdin, mut outbound: mpsc::UnboundedReceiver<String>) {
    while let Some(line) = outbound.recv().await {
        if stdin.write_all(line.as_bytes()).await.is_err() {
            return;
        }
        if stdin.write_all(b"\n").await.is_err() || stdin.flush().await.is_err() {
            return;
        }
    }
}

/// Reads one JSON-RPC object per line. Responses go to the prompt loop; client-method requests are
/// answered here (never dropped, because an unanswered request is a hung agent) and notifications
/// become `Incoming::Update`.
async fn reader(
    stdout: ChildStdout,
    incoming: mpsc::UnboundedSender<Incoming>,
    outbound: mpsc::UnboundedSender<String>,
    approver: Arc<dyn Approver>,
    secrets: Vec<Secret>,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = value.get("id").and_then(Value::as_u64);
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_string);

        match (id, method.as_deref()) {
            (Some(id), None) => {
                let result = match value.get("error") {
                    Some(error) if !error.is_null() => Err(RpcError::from_value(error)),
                    _ => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
                };
                if incoming.send(Incoming::Response { id, result }).is_err() {
                    return;
                }
            }
            (Some(id), Some("session/request_permission")) => {
                let params = value.get("params").cloned().unwrap_or(Value::Null);
                let request = approval_from(&params, &secrets);
                let approver = approver.clone();
                let outbound = outbound.clone();
                tokio::spawn(async move {
                    let decision = approver.approve(&request).await;
                    let outcome = outcome_for(&request.options, decision);
                    let response = json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "outcome": outcome }
                    });
                    let _ = outbound.send(response.to_string());
                });
            }
            (Some(id), Some(method)) => {
                // An unknown client method must still be answered, or the agent waits forever.
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": format!("method not found: {method}") }
                });
                let _ = outbound.send(response.to_string());
            }
            (None, Some("session/update")) => {
                let params = value.get("params").cloned().unwrap_or(Value::Null);
                let update = params.get("update").cloned().unwrap_or(Value::Null);
                let session_id = params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if incoming
                    .send(Incoming::Update { update, session_id })
                    .is_err()
                {
                    return;
                }
            }
            _ => {}
        }
    }
}

fn approval_from(params: &Value, secrets: &[Secret]) -> ApprovalRequest {
    let tool_call = &params["toolCall"];
    ApprovalRequest {
        tool_call_id: tool_call
            .get("toolCallId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        title: tool_call
            .get("title")
            .and_then(Value::as_str)
            .map(|title| crate::redact(title, secrets))
            .unwrap_or_else(|| "tool call".to_string()),
        kind: tool_call
            .get("kind")
            .and_then(Value::as_str)
            .map(str::to_string),
        raw_input: tool_call.get("rawInput").cloned(),
        options: params["options"]
            .as_array()
            .map(|options| {
                options
                    .iter()
                    .map(|option| ApprovalOption {
                        option_id: option
                            .get("optionId")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        name: option
                            .get("name")
                            .and_then(Value::as_str)
                            .map(|name| crate::redact(name, secrets))
                            .unwrap_or_default(),
                        kind: option
                            .get("kind")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Map a decision onto the options the agent actually offered. If the exact kind is missing the
/// request is cancelled: an approval is never widened to a broader scope than the user chose, and
/// narrowing an allow to a once-only yes is not the caller's decision to make.
fn outcome_for(options: &[ApprovalOption], decision: ApprovalDecision) -> Value {
    if decision == ApprovalDecision::Cancel {
        return json!({ "outcome": "cancelled" });
    }
    let wanted = decision.wants();
    let found = options
        .iter()
        .find(|option| option.kind == wanted)
        .map(|option| option.option_id.clone());
    match found {
        Some(option_id) => json!({ "outcome": "selected", "optionId": option_id }),
        None => json!({ "outcome": "cancelled" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decision_maps_onto_the_offered_kind() {
        let options = vec![
            ApprovalOption {
                option_id: "a".into(),
                name: "Allow once".into(),
                kind: "allow_once".into(),
            },
            ApprovalOption {
                option_id: "b".into(),
                name: "Always".into(),
                kind: "allow_always".into(),
            },
        ];
        assert_eq!(
            outcome_for(&options, ApprovalDecision::AllowOnce),
            json!({ "outcome": "selected", "optionId": "a" })
        );
        assert_eq!(
            outcome_for(&options, ApprovalDecision::AllowAlways),
            json!({ "outcome": "selected", "optionId": "b" })
        );
        // Rejecting needs an option the agent did not offer: cancel, never allow.
        assert_eq!(
            outcome_for(&options, ApprovalDecision::RejectOnce),
            json!({ "outcome": "cancelled" })
        );
        assert_eq!(
            outcome_for(&options, ApprovalDecision::Cancel),
            json!({ "outcome": "cancelled" })
        );
    }

    #[test]
    fn an_approval_request_keeps_the_raw_input_for_the_ui() {
        let params = json!({
            "sessionId": "s-1",
            "toolCall": {
                "toolCallId": "call-9",
                "title": "Remove build directory",
                "kind": "delete",
                "rawInput": { "command": "rm -rf build" }
            },
            "options": [
                { "optionId": "allow", "name": "Allow", "kind": "allow_once" },
                { "optionId": "deny", "name": "Deny", "kind": "reject_once" }
            ]
        });
        let request = approval_from(&params, &[]);
        assert_eq!(request.tool_call_id, "call-9");
        assert_eq!(request.title, "Remove build directory");
        assert_eq!(request.kind.as_deref(), Some("delete"));
        assert_eq!(request.options.len(), 2);
        assert_eq!(
            request.raw_input,
            Some(json!({ "command": "rm -rf build" }))
        );
    }

    #[test]
    fn an_approval_title_cannot_carry_the_key_into_the_ui() {
        let params = json!({
            "toolCall": { "toolCallId": "call-1", "title": "key ck-secret-value" }
        });
        let secrets = vec![Secret::new("ck-secret-value")];
        assert_eq!(approval_from(&params, &secrets).title, "key <redacted>");
    }

    #[test]
    fn cline_has_no_key_before_its_own_config_exists() {
        let mut config = AcpConfig::cline(".");
        config.vendor_login = None;
        assert_eq!(acp_auth_mode(&config), AuthMode::None);
        assert!(config.args.contains(&"--acp".to_string()));
    }

    #[test]
    fn cursor_asks_for_api_key_auth() {
        let mut config = AcpConfig::cursor(".");
        config.vendor_login = None;
        assert_eq!(acp_auth_mode(&config), AuthMode::None);
        config
            .env
            .insert("CURSOR_API_KEY".to_string(), Secret::new("ck_test"));
        assert_eq!(acp_auth_mode(&config), AuthMode::ApiKey);
        assert!(config.args.contains(&"acp".to_string()));
    }

    #[test]
    fn the_config_debug_is_redacted() {
        let mut config = AcpConfig::cursor(".");
        config.vendor_login = None;
        config.env.insert(
            "CURSOR_API_KEY".to_string(),
            Secret::new("ck-must-not-print"),
        );
        let debug = format!("{config:?}");
        assert!(debug.contains("CURSOR_API_KEY"));
        assert!(!debug.contains("ck-must-not-print"));
    }
}

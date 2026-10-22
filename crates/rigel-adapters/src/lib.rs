//! Harness adapters.
//!
//! Each external harness gets one module behind the `Adapter` trait. Phase 4 builds them in
//! order: Codex app-server/exec, Claude stream-json, then the ACP clients (Cursor, Cline).
//!
//! Two rules hold across every adapter here:
//!
//! - A harness is spawned only with **its own API key** (D21). A subscription identity is refused,
//!   before the spawn when it can be seen in time, and by killing the child as soon as the first
//!   event reveals one.
//! - A key never leaves the process in a log, an error or an event. Text crossing the adapter
//!   boundary passes through [`redact`], and a config's `Debug` prints `Secret(<redacted>)`.

use std::path::PathBuf;

use rigel_core::credentials::Secret;
use serde::{Deserialize, Serialize};

pub mod acp;
pub mod claude;
pub mod codex;
pub mod probe;
pub mod registry;

pub use acp::{
    AcpClient, AcpConfig, AcpEvent, AcpTurn, AcpUsage, ApprovalDecision, ApprovalOption,
    ApprovalRequest, Approver, DenyAll,
};
pub use claude::{
    ClaudeAdapter, ClaudeConfig, ClaudeEvent, ClaudeTurn, ClaudeUsage, PermissionMode,
};
pub use codex::{CodexAdapter, CodexConfig, CodexEvent, CodexUsage, Transport, TurnResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Harness {
    Native,
    Codex,
    Claude,
    Cursor,
    Cline,
}

impl Harness {
    /// The stable key `adapters.json` and the database use. The enum's `Debug` is not one: it
    /// capitalises, and a state file that changes when a variant is renamed is a migration.
    pub fn key(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Cursor => "cursor",
            Self::Cline => "cline",
        }
    }

    /// The four external harnesses, in the order the UI lists them.
    pub fn external() -> [Harness; 4] {
        [Self::Codex, Self::Claude, Self::Cursor, Self::Cline]
    }
}

/// How a harness proves who it is. Rigel only ever spawns `ApiKey` (D21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    ApiKey,
    Subscription,
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub acp: bool,
    pub json_stream: bool,
    pub programmatic_approval: bool,
    pub worktree: bool,
    pub nested_agents: bool,
}

/// One probe result. `provider` is the credential the harness authenticates as (`openai`,
/// `anthropic`, `cursor`), so the UI can say "Needs API key · OpenAI" without guessing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessInfo {
    pub harness: Harness,
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub auth_mode: AuthMode,
    pub capabilities: Capabilities,
    pub provider: Option<String>,
    /// Present only when `installed` is false: what to run to fix that.
    pub install_hint: Option<String>,
}

/// The provider key one harness authenticates with, and the variable it reaches the child
/// through. Codex and the router both take `OPENAI_API_KEY`; each ACP harness has its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialSpec {
    pub provider: String,
    pub env_var: &'static str,
}

/// `provider` narrows a harness that can be pointed at more than one credential. Codex is the only
/// one today: direct OpenAI by default, OmniRoute when the caller passes its own provider.
pub fn credential_spec(harness: Harness, provider: Option<&str>) -> Option<CredentialSpec> {
    match harness {
        Harness::Codex => Some(CredentialSpec {
            provider: provider
                .unwrap_or(rigel_core::credentials::OPENAI)
                .to_string(),
            env_var: "OPENAI_API_KEY",
        }),
        Harness::Claude => Some(CredentialSpec {
            provider: rigel_core::credentials::ANTHROPIC.to_string(),
            env_var: "ANTHROPIC_API_KEY",
        }),
        Harness::Cursor => Some(CredentialSpec {
            provider: rigel_core::credentials::CURSOR.to_string(),
            env_var: "CURSOR_API_KEY",
        }),
        // Cline's own key path (P4.4): the CLI reads CLINE_API_KEY instead of an interactive
        // sign-in, which is exactly the headless identity D21 requires.
        Harness::Cline => Some(CredentialSpec {
            provider: rigel_core::credentials::CLINE.to_string(),
            env_var: "CLINE_API_KEY",
        }),
        // Native runs inside Rigel.
        Harness::Native => None,
    }
}

/// Replace every known secret in a piece of text. Cheap enough to run on each event, and the only
/// way an adapter can promise that a harness echoing its key cannot put it in the transcript.
pub fn redact(value: &str, secrets: &[Secret]) -> String {
    let mut redacted = value.to_string();
    for secret in secrets {
        let value = secret.expose();
        if value.trim().is_empty() || value.len() < 4 {
            continue;
        }
        redacted = redacted.replace(value, "<redacted>");
    }
    redacted
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPolicy {
    OnRequest,
    Never,
    Auto,
}

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("{harness:?} is not installed")]
    NotInstalled { harness: Harness },

    #[error("{harness:?} is authenticated only by a subscription; Rigel requires an API key")]
    RequiresApiKey { harness: Harness },

    #[error("{0}")]
    Other(String),
}

/// One running harness integration. Nothing above this trait may assume a particular protocol.
#[async_trait::async_trait]
pub trait Adapter: Send + Sync {
    fn harness(&self) -> Harness;

    /// Version, path, auth mode and capabilities. Cached in `adapters.json`, re-probed
    /// when the binary's mtime changes.
    async fn probe(&self) -> Result<HarnessInfo, AdapterError>;
}

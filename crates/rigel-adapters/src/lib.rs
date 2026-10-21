//! Harness adapters.
//!
//! Phase 1 defines the contract only: what a harness is, how it authenticates, and the
//! probe the UI needs before it can offer a spawn button. Each protocol implementation
//! (ACP, Codex app-server, Claude stream-json) arrives in Phase 4.

use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Harness {
    Native,
    Codex,
    Claude,
    Cursor,
    Cline,
}

/// How a harness proves who it is. Rigel only ever spawns `ApiKey` (D21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    ApiKey,
    Subscription,
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    pub acp: bool,
    pub json_stream: bool,
    pub programmatic_approval: bool,
    pub worktree: bool,
    pub nested_agents: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HarnessInfo {
    pub harness: Harness,
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub auth_mode: AuthMode,
    pub capabilities: Capabilities,
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

/// One running harness integration. Implementations arrive in Phase 4; nothing above
/// this trait may assume a particular protocol.
#[async_trait::async_trait]
pub trait Adapter: Send + Sync {
    fn harness(&self) -> Harness;

    /// Version, path, auth mode and capabilities. Cached in `adapters.json`, re-probed
    /// when the binary's mtime changes.
    async fn probe(&self) -> Result<HarnessInfo, AdapterError>;
}

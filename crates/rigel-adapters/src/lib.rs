//! Harness adapters.
//!
//! Phase 1 defines the contract only: what a harness is, how it authenticates, and the
//! probe the UI needs before it can offer a spawn button. Each protocol implementation
//! (ACP, Codex app-server, Claude stream-json) arrives in Phase 4.

use std::path::PathBuf;

use serde::Serialize;

pub mod codex;

pub use codex::{CodexAdapter, CodexConfig, CodexEvent, CodexUsage, Transport, TurnResult};

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

/// The keychain entry and environment variable a harness receives at its process boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialSpec {
    pub provider: &'static str,
    pub env_var: &'static str,
}

/// Resolve the API key contract for one harness. Codex normally uses `rigel/openai` as
/// `OPENAI_API_KEY`. When its configured endpoint is the local router, it deliberately uses
/// `rigel/omniroute` under that same environment name: Codex only understands the OpenAI name,
/// while the credential is the router's own key. The router's current failure to enforce the key
/// is never treated as authentication (§10, D21).
pub fn credential_spec(harness: Harness, base_url: Option<&str>) -> Option<CredentialSpec> {
    match harness {
        Harness::Codex if base_url.is_some_and(is_local_omniroute_url) => Some(CredentialSpec {
            provider: rigel_core::credentials::OMNIROUTE,
            env_var: "OPENAI_API_KEY",
        }),
        Harness::Codex => Some(CredentialSpec {
            provider: rigel_core::credentials::OPENAI,
            env_var: "OPENAI_API_KEY",
        }),
        Harness::Claude => Some(CredentialSpec {
            provider: rigel_core::credentials::ANTHROPIC,
            env_var: "ANTHROPIC_API_KEY",
        }),
        Harness::Cursor => Some(CredentialSpec {
            provider: rigel_core::credentials::CURSOR,
            env_var: "CURSOR_API_KEY",
        }),
        Harness::Native | Harness::Cline => None,
    }
}

fn is_local_omniroute_url(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    let authority = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
        .unwrap_or(&value)
        .split('/')
        .next()
        .unwrap_or_default();
    let (host, port) = if authority.starts_with('[') {
        let Some((host, suffix)) = authority.split_once(']') else {
            return false;
        };
        (host, suffix.strip_prefix(':'))
    } else {
        authority
            .rsplit_once(':')
            .map_or((authority, None), |(host, port)| (host, Some(port)))
    };
    matches!(host, "127.0.0.1" | "localhost" | "[::1") && port == Some("20128")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_credentials_are_explicit() {
        assert_eq!(
            credential_spec(Harness::Codex, Some("https://api.openai.com/v1")),
            Some(CredentialSpec {
                provider: rigel_core::credentials::OPENAI,
                env_var: "OPENAI_API_KEY",
            })
        );
        assert_eq!(
            credential_spec(Harness::Codex, Some("http://127.0.0.1:20128/v1")),
            Some(CredentialSpec {
                provider: rigel_core::credentials::OMNIROUTE,
                env_var: "OPENAI_API_KEY",
            })
        );
        assert_eq!(
            credential_spec(Harness::Codex, Some("http://localhost:11434/v1"))
                .unwrap()
                .provider,
            rigel_core::credentials::OPENAI
        );
        assert_eq!(
            credential_spec(Harness::Claude, None).unwrap().env_var,
            "ANTHROPIC_API_KEY"
        );
        assert_eq!(
            credential_spec(Harness::Cursor, None).unwrap().env_var,
            "CURSOR_API_KEY"
        );
    }
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

    #[error(
        "{harness:?} requires its own API key; Rigel will not reuse a subscription or vendor login"
    )]
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

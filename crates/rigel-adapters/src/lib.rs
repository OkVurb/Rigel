//! Harness adapters.
//!
//! Each external harness gets one module behind the `Adapter` trait. Phase 4 builds them in
//! order: Codex app-server/exec, Claude stream-json, then the ACP clients (Cursor, Cline).

use std::path::PathBuf;

use rigel_core::credentials::Secret;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod acp;
pub mod claude;
pub mod codex;
pub mod normalise;
pub mod registry;
pub mod turn;

pub use acp::{
    AcpClient, AcpConfig, AcpEvent, AcpTurn, AcpUsage, ApprovalDecision, ApprovalOption,
    ApprovalRequest, Approver, DenyAll,
};
pub use claude::{
    ClaudeAdapter, ClaudeConfig, ClaudeEvent, ClaudeTurn, ClaudeUsage, PermissionMode,
};
pub use codex::{
    CodexAdapter, CodexConfig, CodexEvent, CodexUsage, ContextEstimate, Transport, TurnResult,
};
pub use normalise::NormalItem;
pub use registry::{AdapterCache, probe_all, probe_cached};
pub use turn::{Recorded, TurnEnd, resolve_session, write_mcp_config};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Harness {
    Native,
    Codex,
    Claude,
    Cursor,
    Cline,
}

impl Harness {
    /// The stable lowercase key used by the CLI, `adapters.json` and the agent harness column.
    pub fn key(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Cursor => "cursor",
            Self::Cline => "cline",
        }
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

/// Replace every injected credential value with `<redacted>`. Applied to events, error text and
/// anything else that leaves an adapter, so a harness that echoes its own environment cannot put a
/// key into a transcript or a log line.
pub fn redact(value: &str, secrets: &[Secret]) -> String {
    secrets.iter().fold(value.to_string(), |redacted, secret| {
        let exposed = secret.expose();
        if exposed.is_empty() {
            redacted
        } else {
            redacted.replace(exposed, "<redacted>")
        }
    })
}

/// [`redact`] over every string inside a JSON value. Item bodies carry raw tool input from the
/// harness, so the same rule that protects event text has to apply before one is emitted.
pub fn redact_json(value: &Value, secrets: &[Secret]) -> Value {
    match value {
        Value::String(text) => Value::String(redact(text, secrets)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_json(item, secrets))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), redact_json(value, secrets)))
                .collect(),
        ),
        other => other.clone(),
    }
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
        // Cline's ACP child authenticates with the Cline key, not a raw model-vendor key:
        // `cline --acp` reads it from `CLINE_API_KEY` and resolves the provider itself (P4.4).
        Harness::Cline => Some(CredentialSpec {
            provider: rigel_core::credentials::CLINE,
            env_var: "CLINE_API_KEY",
        }),
        Harness::Native => None,
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
            credential_spec(Harness::Codex, Some("http://[::1]:20128/v1"))
                .unwrap()
                .provider,
            rigel_core::credentials::OMNIROUTE
        );
        assert_eq!(
            credential_spec(Harness::Cursor, None).unwrap().env_var,
            "CURSOR_API_KEY"
        );
        let cline = credential_spec(Harness::Cline, None).expect("Cline has a key path");
        assert_eq!(cline.provider, rigel_core::credentials::CLINE);
        assert_eq!(cline.env_var, "CLINE_API_KEY");
        assert_eq!(credential_spec(Harness::Native, None), None);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub acp: bool,
    pub json_stream: bool,
    pub programmatic_approval: bool,
    pub worktree: bool,
    pub nested_agents: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessInfo {
    pub harness: Harness,
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub auth_mode: AuthMode,
    pub capabilities: Capabilities,
    /// Keychain provider named in the UI's `Needs API key · <provider>` line. `None` means the
    /// harness has no key of its own (the native agent reads provider keys per model).
    #[serde(default)]
    pub provider: Option<String>,
    /// The copy-paste install command, set only while `installed` is false.
    #[serde(default)]
    pub install_hint: Option<String>,
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

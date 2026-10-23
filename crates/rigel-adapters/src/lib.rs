//! Harness adapters.
//!
//! Each external harness gets one module behind the `Adapter` trait. Phase 4 builds them in
//! order: Codex app-server/exec, Claude stream-json, then the ACP clients (Cursor, Cline and
//! Gemini). Gemini is also a model provider for the native agent; the two are separate paths and
//! share only the `gemini` keychain entry.

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
    Gemini,
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
            Self::Gemini => "gemini",
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

/// A streaming redactor with a bounded hold-back window (D78).
///
/// Redacting each chunk independently leaks a credential split across two deltas: the first
/// chunk carries a prefix that matches nothing, and the completed string only exists after the
/// second. This holds back the longest suffix that could still become a secret (at most the
/// longest secret's length) and flushes it once the next chunk proves it is safe, or at end of
/// stream. Nothing that is part of a secret reaches a rendered event.
pub struct StreamRedactor<'a> {
    secrets: &'a [Secret],
    pending: String,
    longest: usize,
}

impl<'a> StreamRedactor<'a> {
    pub fn new(secrets: &'a [Secret]) -> Self {
        let longest = secrets
            .iter()
            .map(|secret| secret.expose().chars().count())
            .max()
            .unwrap_or(0);
        Self {
            secrets,
            pending: String::new(),
            longest,
        }
    }

    /// Append one chunk and return the part that is now safe to render, already redacted.
    pub fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        let cut = self.safe_prefix_len();
        if cut == 0 {
            return String::new();
        }
        let safe: String = self.pending.drain(..cut).collect();
        redact(&safe, self.secrets)
    }

    /// Flush the held tail at end of stream.
    pub fn finish(&mut self) -> String {
        let tail = std::mem::take(&mut self.pending);
        redact(&tail, self.secrets)
    }

    /// The byte index before the longest suffix that is a prefix of some secret.
    fn safe_prefix_len(&self) -> usize {
        let text = &self.pending;
        if self.longest == 0 {
            return text.len();
        }
        let max = self.longest.min(text.len());
        for hold in (1..=max).rev() {
            let cut = text.len() - hold;
            if !text.is_char_boundary(cut) {
                continue;
            }
            let suffix = &text[cut..];
            if self
                .secrets
                .iter()
                .any(|secret| secret.expose().starts_with(suffix))
            {
                return cut;
            }
        }
        text.len()
    }
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
        // Gemini CLI takes a Google AI Studio key as `GEMINI_API_KEY`; the native agent's Gemini
        // provider resolves the same keychain entry, so one stored key serves both paths.
        Harness::Gemini => Some(CredentialSpec {
            provider: rigel_core::credentials::GEMINI,
            env_var: "GEMINI_API_KEY",
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
        let gemini = credential_spec(Harness::Gemini, None).expect("Gemini has a key path");
        assert_eq!(gemini.provider, rigel_core::credentials::GEMINI);
        assert_eq!(gemini.env_var, "GEMINI_API_KEY");
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

#[cfg(test)]
mod stream_tests {
    use super::*;

    fn secret(value: &str) -> Secret {
        Secret::new(value)
    }

    #[test]
    fn a_secret_split_across_chunks_never_reaches_an_event() {
        let secrets = vec![secret("sk-very-secret-value")];
        let mut stream = StreamRedactor::new(&secrets);
        let mut rendered = String::new();
        for chunk in ["before sk-very-", "secret-", "value after"] {
            rendered.push_str(&stream.push(chunk));
        }
        rendered.push_str(&stream.finish());
        assert_eq!(rendered, "before <redacted> after");
        assert!(!rendered.contains("sk-"));
    }

    #[test]
    fn a_non_secret_prefix_is_released_without_waiting_for_the_end() {
        let secrets = vec![secret("sk-aaaaaaaa")];
        let mut stream = StreamRedactor::new(&secrets);
        assert_eq!(stream.push("hello "), "hello ");
        assert_eq!(stream.push("sk-"), "");
        assert_eq!(stream.push("world"), "sk-world");
        assert_eq!(stream.finish(), "");
    }

    #[test]
    fn a_whole_secret_ending_a_chunk_is_held_until_the_next_one() {
        let secrets = vec![secret("abcd")];
        let mut stream = StreamRedactor::new(&secrets);
        assert_eq!(stream.push("xxabcd"), "xx");
        assert_eq!(stream.finish(), "<redacted>");
    }

    #[test]
    fn multibyte_text_keeps_its_boundaries() {
        let secrets = vec![secret("sk-ééé")];
        let mut stream = StreamRedactor::new(&secrets);
        let mut rendered = stream.push("héllo wörld");
        rendered.push_str(&stream.finish());
        assert_eq!(rendered, "héllo wörld");
    }

    #[test]
    fn empty_secret_list_passes_everything_through() {
        let secrets: Vec<Secret> = Vec::new();
        let mut stream = StreamRedactor::new(&secrets);
        assert_eq!(stream.push("plain"), "plain");
        assert_eq!(stream.finish(), "");
    }
}

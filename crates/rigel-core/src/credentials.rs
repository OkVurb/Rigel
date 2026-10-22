//! Provider keys in the OS keychain (§10, D14).
//!
//! One entry per provider, named `rigel/<provider>`. The value is never written to a config
//! file, never logged, and never returned to the renderer: the UI can ask *whether* a key
//! exists, not what it is.
//!
//! `OPENROUTER_API_KEY` (and the other provider variables below) are honoured as a fallback
//! for headless use. That is a deliberate exception for the CLI and CI, not a storage
//! location: a key in the environment is the user's choice, and Rigel still never writes it
//! anywhere.

use std::collections::BTreeMap;
use std::sync::Mutex;

pub const OPENROUTER: &str = "openrouter";
/// The key OmniRoute itself checks on `/v1`; not a provider key.
pub const OMNIROUTE: &str = "omniroute";
pub const OPENAI: &str = "openai";
pub const ANTHROPIC: &str = "anthropic";
pub const CURSOR: &str = "cursor";
pub const CLINE: &str = "cline";
pub const GEMINI: &str = "gemini";

/// Every provider Rigel can resolve. One list so a new adapter cannot be added to the keychain
/// without also appearing in `rigel keys status` and in the environment scrub for commands.
pub const PROVIDERS: &[&str] = &[
    OMNIROUTE, OPENROUTER, OPENAI, ANTHROPIC, CURSOR, CLINE, GEMINI,
];

const SERVICE: &str = "rigel";

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("no key stored for {0}")]
    Missing(String),

    #[error("the OS keychain refused the request: {0}")]
    Keychain(String),
}

/// Store or replace a key. Called by `rigel keys set` with the value read from stdin.
pub fn set(provider: &str, value: &str) -> Result<(), KeyError> {
    let entry = keyring::Entry::new(SERVICE, provider)
        .map_err(|error| KeyError::Keychain(error.to_string()))?;
    entry
        .set_password(value)
        .map_err(|error| KeyError::Keychain(error.to_string()))
}

pub fn delete(provider: &str) -> Result<(), KeyError> {
    let entry = keyring::Entry::new(SERVICE, provider)
        .map_err(|error| KeyError::Keychain(error.to_string()))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(KeyError::Keychain(error.to_string())),
    }
}

/// Whether a usable key exists, for the settings screen. Never returns the key.
pub fn present(provider: &str) -> bool {
    get_from(&KeychainCredentialStore, provider).is_ok()
}

/// The key itself. Environment first so a test or CI run can override the keychain.
pub fn get(provider: &str) -> Result<String, KeyError> {
    get_from(&KeychainCredentialStore, provider).map(|secret| secret.expose().to_string())
}

/// Where a per-agent credential is read from. The adapters take one of these instead of calling
/// `get()` directly, so a test can inject a key without touching the OS keychain or the process
/// environment.
pub trait CredentialStore: Send + Sync {
    /// `None` means this store has nothing for the provider; the caller decides whether that is
    /// fatal.
    fn lookup(&self, provider: &str) -> Option<Secret>;
}

/// The real store: the environment first (CI, a shell export, `infisical run`), then the OS
/// keychain. A key in the environment wins so a developer can override a stale keychain entry for
/// one run without deleting it.
pub struct KeychainCredentialStore;

impl CredentialStore for KeychainCredentialStore {
    fn lookup(&self, provider: &str) -> Option<Secret> {
        if let Some(value) = env_key(provider) {
            return Some(Secret::new(value));
        }
        keyring::Entry::new(SERVICE, provider)
            .ok()?
            .get_password()
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(Secret::new)
    }
}

/// A store held in memory. Tests use it so no test ever reads a real keychain entry.
#[derive(Default)]
pub struct MemoryCredentialStore {
    keys: Mutex<BTreeMap<String, String>>,
}

impl MemoryCredentialStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(provider: &str, value: impl Into<String>) -> Self {
        let store = Self::new();
        store.insert(provider, value);
        store
    }

    pub fn insert(&self, provider: &str, value: impl Into<String>) {
        self.keys
            .lock()
            .expect("credential store lock")
            .insert(provider.to_string(), value.into());
    }

    pub fn remove(&self, provider: &str) {
        self.keys
            .lock()
            .expect("credential store lock")
            .remove(provider);
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn lookup(&self, provider: &str) -> Option<Secret> {
        self.keys
            .lock()
            .expect("credential store lock")
            .get(provider)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(Secret::new)
    }
}

/// Whether a key exists, with the environment checked first. Adapters use this instead of
/// reading the value just to decide whether a harness can start.
pub fn present_in(store: &dyn CredentialStore, provider: &str) -> bool {
    env_key(provider).is_some() || store.lookup(provider).is_some()
}

/// Resolve one provider through a store. An empty provider name is a miss, not a keychain read.
pub fn get_from(store: &dyn CredentialStore, provider: &str) -> Result<Secret, KeyError> {
    if provider.trim().is_empty() {
        return Err(KeyError::Missing(provider.to_string()));
    }
    store
        .lookup(provider)
        .ok_or_else(|| KeyError::Missing(provider.to_string()))
}

/// The environment name a provider falls back to. Public so an adapter can say which variable it
/// expects without hard-coding the mapping a second time.
pub fn provider_environment_variable(provider: &str) -> Option<&'static str> {
    match provider {
        OPENROUTER => Some("OPENROUTER_API_KEY"),
        OMNIROUTE => Some("OMNIROUTE_API_KEY"),
        OPENAI => Some("OPENAI_API_KEY"),
        ANTHROPIC => Some("ANTHROPIC_API_KEY"),
        CURSOR => Some("CURSOR_API_KEY"),
        CLINE => Some("CLINE_API_KEY"),
        GEMINI => Some("GEMINI_API_KEY"),
        _ => None,
    }
}

fn env_key(provider: &str) -> Option<String> {
    let name = provider_environment_variable(provider)?;
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The provider's credential never appears in a `Debug` dump.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_does_not_print_itself() {
        let secret = Secret::new("sk-live-abc");
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
    }

    #[test]
    fn a_memory_store_answers_only_for_what_it_holds() {
        let store = MemoryCredentialStore::with("openai", "sk-test");
        assert_eq!(get_from(&store, "openai").unwrap().expose(), "sk-test");
        assert!(matches!(
            get_from(&store, "anthropic"),
            Err(KeyError::Missing(provider)) if provider == "anthropic"
        ));
        store.remove("openai");
        assert!(get_from(&store, "openai").is_err());
    }

    #[test]
    fn a_blank_key_is_not_a_key() {
        let store = MemoryCredentialStore::with("openai", "   ");
        assert!(get_from(&store, "openai").is_err());
        assert!(get_from(&MemoryCredentialStore::new(), "").is_err());
    }

    #[test]
    fn every_provider_rigel_knows_has_an_environment_fallback() {
        // The mapping is what makes `infisical run` work without any code change; a provider
        // added without one would silently fall back to the keychain only.
        for provider in [
            OPENAI, ANTHROPIC, OPENROUTER, OMNIROUTE, CURSOR, CLINE, GEMINI,
        ] {
            let name = provider_environment_variable(provider)
                .unwrap_or_else(|| panic!("no environment name for {provider}"));
            assert!(name.ends_with("_API_KEY"), "{provider} -> {name}");
        }
        assert!(provider_environment_variable("not-a-provider").is_none());
    }
}

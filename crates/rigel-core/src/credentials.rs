//! Provider keys in the OS keychain (§10, D14).
//!
//! One entry per provider, named `rigel/<provider>`. The value is never written to a config
//! file, never logged, and never returned to the renderer: the UI can ask *whether* a key
//! exists, not what it is.
//!
//! Provider environment variables are honoured before the backing store for headless use.
//! That is a deliberate exception for the CLI and CI, not a storage location: a key in the
//! environment is the user's choice, and Rigel still never writes it anywhere.

use std::collections::BTreeMap;
use std::sync::Mutex;

pub const OPENROUTER: &str = "openrouter";
/// The key OmniRoute itself checks on `/v1`; not a model-provider key.
pub const OMNIROUTE: &str = "omniroute";
pub const OPENAI: &str = "openai";
pub const ANTHROPIC: &str = "anthropic";
pub const CURSOR: &str = "cursor";

const SERVICE: &str = "rigel";

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("no key stored for {0}")]
    Missing(String),

    #[error("the OS keychain refused the request: {0}")]
    Keychain(String),
}

/// A credential backend. Implementations never add environment fallback themselves; callers use
/// [`get_from`] and [`present_in`] so the same env-first rule applies to every backend.
pub trait CredentialStore: Send + Sync {
    fn get(&self, provider: &str) -> Result<Secret, KeyError>;
    fn present(&self, provider: &str) -> bool;
    fn set(&self, provider: &str, value: &Secret) -> Result<(), KeyError>;
    fn delete(&self, provider: &str) -> Result<(), KeyError>;
}

/// The production store backed by the operating system credential manager.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeychainCredentialStore;

impl KeychainCredentialStore {
    fn entry(provider: &str) -> Result<keyring::Entry, KeyError> {
        keyring::Entry::new(SERVICE, provider)
            .map_err(|error| KeyError::Keychain(error.to_string()))
    }
}

impl CredentialStore for KeychainCredentialStore {
    fn get(&self, provider: &str) -> Result<Secret, KeyError> {
        match Self::entry(provider)?.get_password() {
            Ok(value) if !value.trim().is_empty() => Ok(Secret::new(value)),
            Ok(_) | Err(keyring::Error::NoEntry) => Err(KeyError::Missing(provider.to_string())),
            Err(error) => Err(KeyError::Keychain(error.to_string())),
        }
    }

    fn present(&self, provider: &str) -> bool {
        self.get(provider).is_ok()
    }

    fn set(&self, provider: &str, value: &Secret) -> Result<(), KeyError> {
        Self::entry(provider)?
            .set_password(value.expose())
            .map_err(|error| KeyError::Keychain(error.to_string()))
    }

    fn delete(&self, provider: &str) -> Result<(), KeyError> {
        match Self::entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(KeyError::Keychain(error.to_string())),
        }
    }
}

/// A deterministic credential backend for tests and embedded callers that must not touch the OS
/// keychain. Its `Debug` output deliberately reports only the entry names.
#[derive(Default)]
pub struct MemoryCredentialStore {
    values: Mutex<BTreeMap<String, Secret>>,
}

impl std::fmt::Debug for MemoryCredentialStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys = self
            .values
            .lock()
            .map(|values| values.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        formatter
            .debug_struct("MemoryCredentialStore")
            .field("providers", &keys)
            .finish()
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn get(&self, provider: &str) -> Result<Secret, KeyError> {
        self.values
            .lock()
            .map_err(|error| KeyError::Keychain(error.to_string()))?
            .get(provider)
            .cloned()
            .filter(|value| !value.expose().trim().is_empty())
            .ok_or_else(|| KeyError::Missing(provider.to_string()))
    }

    fn present(&self, provider: &str) -> bool {
        self.get(provider).is_ok()
    }

    fn set(&self, provider: &str, value: &Secret) -> Result<(), KeyError> {
        self.values
            .lock()
            .map_err(|error| KeyError::Keychain(error.to_string()))?
            .insert(provider.to_string(), value.clone());
        Ok(())
    }

    fn delete(&self, provider: &str) -> Result<(), KeyError> {
        self.values
            .lock()
            .map_err(|error| KeyError::Keychain(error.to_string()))?
            .remove(provider);
        Ok(())
    }
}

/// Cline stores one credential per selected model provider. P4.4 will write its per-agent config;
/// defining the keychain name here keeps that later code on the same naming contract.
pub fn cline_provider(provider: &str) -> String {
    format!("cline.{}", provider.trim().to_ascii_lowercase())
}

/// Read a credential with the documented environment-first precedence.
pub fn get_from(store: &dyn CredentialStore, provider: &str) -> Result<Secret, KeyError> {
    env_key(provider).map_or_else(|| store.get(provider), |value| Ok(Secret::new(value)))
}

/// Check credential presence with the same environment-first precedence as [`get_from`].
pub fn present_in(store: &dyn CredentialStore, provider: &str) -> bool {
    env_key(provider).is_some() || store.present(provider)
}

/// Store or replace a key. Called by `rigel keys set` with the value read from stdin.
pub fn set(provider: &str, value: &str) -> Result<(), KeyError> {
    KeychainCredentialStore.set(provider, &Secret::new(value))
}

pub fn delete(provider: &str) -> Result<(), KeyError> {
    KeychainCredentialStore.delete(provider)
}

/// Whether a usable key exists, for the settings screen. Never returns the key.
pub fn present(provider: &str) -> bool {
    present_in(&KeychainCredentialStore, provider)
}

/// The key itself. Environment first so a test or CI run can override the keychain.
pub fn get(provider: &str) -> Result<String, KeyError> {
    get_from(&KeychainCredentialStore, provider).map(Secret::into_inner)
}

fn env_key(provider: &str) -> Option<String> {
    let name = match provider {
        OPENROUTER => "OPENROUTER_API_KEY",
        OMNIROUTE => "OMNIROUTE_API_KEY",
        OPENAI => "OPENAI_API_KEY",
        ANTHROPIC => "ANTHROPIC_API_KEY",
        CURSOR => "CURSOR_API_KEY",
        _ => return None,
    };
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// A credential value whose debug representation is always redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn memory_store_supports_the_full_lifecycle() {
        let store = MemoryCredentialStore::default();
        assert!(!store.present(OPENAI));
        assert!(matches!(store.get(OPENAI), Err(KeyError::Missing(_))));

        store.set(OPENAI, &Secret::new("memory-key")).unwrap();
        assert!(store.present(OPENAI));
        assert_eq!(store.get(OPENAI).unwrap().expose(), "memory-key");

        store.delete(OPENAI).unwrap();
        assert!(!store.present(OPENAI));
        store.delete(OPENAI).unwrap();
    }

    #[test]
    fn environment_overrides_the_backing_store() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var_os("CURSOR_API_KEY");
        let store = MemoryCredentialStore::default();
        store.set(CURSOR, &Secret::new("stored-key")).unwrap();

        // SAFETY: this test serialises its own access to CURSOR_API_KEY and restores the prior
        // value before returning. No other credential test mutates this variable.
        unsafe { std::env::set_var("CURSOR_API_KEY", "environment-key") };
        assert_eq!(
            get_from(&store, CURSOR).unwrap().expose(),
            "environment-key"
        );

        // SAFETY: see the matching set above.
        unsafe {
            if let Some(value) = original {
                std::env::set_var("CURSOR_API_KEY", value);
            } else {
                std::env::remove_var("CURSOR_API_KEY");
            }
        }
    }

    #[test]
    fn cline_names_are_provider_scoped() {
        assert_eq!(cline_provider(" OpenRouter "), "cline.openrouter");
    }

    #[test]
    fn secrets_and_memory_store_debug_are_redacted() {
        let secret = Secret::new("sk-live-abc");
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");

        let store = MemoryCredentialStore::default();
        store.set(OPENAI, &secret).unwrap();
        let debug = format!("{store:?}");
        assert!(debug.contains(OPENAI));
        assert!(!debug.contains("sk-live-abc"));
    }
}

//! Provider keys in the OS keychain (§10, D14).
//!
//! One entry per provider, named `rigel/<provider>`. The value is never written to a config
//! file, never logged, and never returned to the renderer: the UI can ask *whether* a key
//! exists, not what it is.
//!
//! `OPENROUTER_API_KEY` is honoured as a fallback for headless use. That is a deliberate
//! exception for the CLI and CI, not a storage location: a key in the environment is the
//! user's choice, and Rigel still never writes it anywhere.

pub const OPENROUTER: &str = "openrouter";

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
    if env_key(provider).is_some() {
        return true;
    }
    keyring::Entry::new(SERVICE, provider)
        .ok()
        .and_then(|entry| entry.get_password().ok())
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

/// The key itself. Environment first so a test or CI run can override the keychain.
pub fn get(provider: &str) -> Result<String, KeyError> {
    if let Some(value) = env_key(provider) {
        return Ok(value);
    }
    let entry = keyring::Entry::new(SERVICE, provider)
        .map_err(|error| KeyError::Keychain(error.to_string()))?;
    match entry.get_password() {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        Ok(_) => Err(KeyError::Missing(provider.to_string())),
        Err(keyring::Error::NoEntry) => Err(KeyError::Missing(provider.to_string())),
        Err(error) => Err(KeyError::Keychain(error.to_string())),
    }
}

fn env_key(provider: &str) -> Option<String> {
    let name = match provider {
        OPENROUTER => "OPENROUTER_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        _ => return None,
    };
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
}

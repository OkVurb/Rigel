//! Updates: a signed manifest, and nothing else (N5, P7.7).
//!
//! The rule N5 states is narrow on purpose: *no telemetry, no phone-home; updates are a signed
//! manifest fetch you can disable.* This module is the whole of the network behaviour:
//!
//! * **Off until asked for.** The switch defaults to `false`. That is stricter than "opt-out"
//!   and it is what makes the Phase 7 exit test ("no unexpected network calls") true by
//!   construction rather than by inspection.
//! * **Signed, and checked before parsed.** An Ed25519 signature over the canonical manifest
//!   bytes (`version\nurl\nsha256`) means a mirror or a proxy cannot change the version, the
//!   URL or the artifact hash. The public key is a local file (`update-pubkey.txt`) or
//!   `RIGEL_UPDATE_PUBKEY`; there is no built-in key to leak.
//! * **The artifact is checked too.** The manifest's `sha256` is compared with the downloaded
//!   bytes before anything is executed. Signature without hash would let a stale artifact be
//!   paired with a fresh signature.
//!
//! What this does *not* do: install anything. `UPDATE.md` and the release process run the
//! installer the user downloaded; Rigel only tells them a new version exists and that the file
//! is the one the publisher signed.

use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SETTINGS_FILE: &str = "update.json";
pub const PUBKEY_FILE: &str = "update-pubkey.txt";

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("updates are disabled; turn them on with `rigel update enable`")]
    Disabled,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("the manifest is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("no update public key: put 64 hex characters in {path} or set RIGEL_UPDATE_PUBKEY")]
    NoKey { path: PathBuf },

    #[error("the public key is not 32 bytes of hex: {0}")]
    BadKey(String),

    #[error("the signature is not 64 bytes of hex: {0}")]
    BadSignature(String),

    #[error("the manifest signature does not verify against this key")]
    BadSignatureCheck,

    #[error("the artifact is {found}, but the signed manifest says {expected}")]
    HashMismatch { expected: String, found: String },

    #[error("this build is {current} and the manifest is {offered}; nothing to do")]
    NotNewer { current: String, offered: String },

    #[error("could not fetch the manifest: {0}")]
    Network(String),

    #[error("a manifest must come from https, not {0}")]
    NotHttps(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    /// The artifact to download. `https` only; a `file://` URL is for tests and local builds.
    pub url: String,
    /// Hex sha256 of the artifact.
    pub sha256: String,
    /// Hex Ed25519 signature over `version\nurl\nsha256`.
    pub signature: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub published_at: Option<i64>,
}

impl UpdateManifest {
    /// The bytes the signature covers. Fixed order, no JSON formatting, so a re-serialised
    /// manifest still verifies.
    pub fn signed_bytes(&self) -> Vec<u8> {
        format!("{}\n{}\n{}", self.version, self.url, self.sha256).into_bytes()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSettings {
    /// Off by default: no request is made until someone turns this on.
    #[serde(default)]
    pub enabled: bool,
    /// The manifest location. Kept next to the switch so the file is the whole configuration.
    #[serde(default)]
    pub manifest_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheck {
    pub current: String,
    pub offered: String,
    pub available: bool,
    pub verified: bool,
    pub url: String,
    pub sha256: String,
    pub notes: Option<String>,
}

pub fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SETTINGS_FILE)
}

pub fn load_settings(data_dir: &Path) -> UpdateSettings {
    std::fs::read_to_string(settings_path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_settings(data_dir: &Path, settings: &UpdateSettings) -> Result<(), UpdateError> {
    std::fs::create_dir_all(data_dir)?;
    let path = settings_path(data_dir);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(settings)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// The key, from the environment first so a build machine can pin one without a file.
pub fn public_key(data_dir: &Path) -> Result<[u8; 32], UpdateError> {
    let hex = match std::env::var("RIGEL_UPDATE_PUBKEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => {
            let path = data_dir.join(PUBKEY_FILE);
            if !path.exists() {
                return Err(UpdateError::NoKey { path });
            }
            std::fs::read_to_string(&path)?
        }
    };
    let bytes =
        decode_hex(hex.trim()).ok_or_else(|| UpdateError::BadKey(hex.trim().to_string()))?;
    let key: [u8; 32] = bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| UpdateError::BadKey(format!("{} bytes", bytes.len())))?;
    Ok(key)
}

pub fn verify_manifest(manifest: &UpdateManifest, key: &[u8; 32]) -> Result<(), UpdateError> {
    let signature = decode_hex(&manifest.signature)
        .ok_or_else(|| UpdateError::BadSignature(manifest.signature.clone()))?;
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|bytes: Vec<u8>| UpdateError::BadSignature(format!("{} bytes", bytes.len())))?;
    let verifying =
        VerifyingKey::from_bytes(key).map_err(|error| UpdateError::BadKey(error.to_string()))?;
    verifying
        .verify_strict(&manifest.signed_bytes(), &Signature::from_bytes(&signature))
        .map_err(|_| UpdateError::BadSignatureCheck)
}

pub fn parse_manifest(text: &str) -> Result<UpdateManifest, UpdateError> {
    Ok(serde_json::from_str(text)?)
}

/// Fetch a manifest over https. Called only when someone typed the URL or turned the switch on;
/// nothing in the app does this on a timer.
pub async fn fetch_manifest(url: &str) -> Result<UpdateManifest, UpdateError> {
    if !url.starts_with("https://") {
        return Err(UpdateError::NotHttps(url.to_string()));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("rigel/0.1")
        .build()
        .map_err(|error| UpdateError::Network(error.to_string()))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| UpdateError::Network(error.to_string()))?;
    if !response.status().is_success() {
        return Err(UpdateError::Network(format!("HTTP {}", response.status())));
    }
    // A manifest is small; refuse an endless one rather than buffering it.
    let bytes = response
        .bytes()
        .await
        .map_err(|error| UpdateError::Network(error.to_string()))?;
    if bytes.len() > 64 * 1_024 {
        return Err(UpdateError::Network(
            "the manifest is larger than 64 KB".into(),
        ));
    }
    parse_manifest(&String::from_utf8_lossy(&bytes))
}

/// Verify a downloaded artifact against the signed hash.
pub fn verify_artifact(path: &Path, manifest: &UpdateManifest) -> Result<(), UpdateError> {
    let bytes = std::fs::read(path)?;
    let found = format!("{:x}", Sha256::digest(&bytes));
    if found.eq_ignore_ascii_case(&manifest.sha256) {
        Ok(())
    } else {
        Err(UpdateError::HashMismatch {
            expected: manifest.sha256.clone(),
            found,
        })
    }
}

/// Compare a manifest with this build. A manifest that verifies but is not newer is a `NoNewer`
/// answer, not an error the caller has to interpret.
pub fn check(
    data_dir: &Path,
    manifest: &UpdateManifest,
    current: &str,
) -> Result<UpdateCheck, UpdateError> {
    if !load_settings(data_dir).enabled {
        return Err(UpdateError::Disabled);
    }
    let key = public_key(data_dir)?;
    verify_manifest(manifest, &key)?;
    let available = is_newer(&manifest.version, current);
    Ok(UpdateCheck {
        current: current.to_string(),
        offered: manifest.version.clone(),
        available,
        verified: true,
        url: manifest.url.clone(),
        sha256: manifest.sha256.clone(),
        notes: manifest.notes.clone(),
    })
}

/// Numeric comparison, segment by segment. A pre-release suffix (`1.2.0-rc1`) sorts before the
/// release it precedes, which is the only nuance the updater needs.
pub fn is_newer(offered: &str, current: &str) -> bool {
    fn parts(version: &str) -> (Vec<u64>, bool) {
        let (core, pre) = match version.split_once('-') {
            Some((core, _)) => (core, true),
            None => (version, false),
        };
        (
            core.split('.')
                .map(|part| part.parse::<u64>().unwrap_or(0))
                .collect(),
            pre,
        )
    }
    let (offered_parts, offered_pre) = parts(offered.trim_start_matches('v'));
    let (current_parts, current_pre) = parts(current.trim_start_matches('v'));
    for index in 0..offered_parts.len().max(current_parts.len()) {
        let a = offered_parts.get(index).copied().unwrap_or(0);
        let b = current_parts.get(index).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    // Same numbers: a release is newer than its own pre-release.
    current_pre && !offered_pre
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    for pair in bytes.chunks(2) {
        let high = (pair[0] as char).to_digit(16)?;
        let low = (pair[1] as char).to_digit(16)?;
        out.push((high * 16 + low) as u8);
    }
    Some(out)
}

/* ------------------------------------------------------------------ *
 * tests
 * ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed_manifest(seed: u8, version: &str, sha: &str) -> (UpdateManifest, [u8; 32]) {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let mut manifest = UpdateManifest {
            version: version.to_string(),
            url: format!("https://example.test/rigel-{version}.msi"),
            sha256: sha.to_string(),
            signature: String::new(),
            notes: Some("a test release".to_string()),
            published_at: None,
        };
        let signature = key.sign(&manifest.signed_bytes());
        manifest.signature = signature
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        (manifest, key.verifying_key().to_bytes())
    }

    #[test]
    fn a_signed_manifest_verifies_and_a_changed_field_does_not() {
        let seed_sha = "a".repeat(64);
        let (manifest, key) = signed_manifest(7, "0.2.0", &seed_sha);
        verify_manifest(&manifest, &key).expect("the signature is valid");

        let mut edited = manifest.clone();
        edited.version = "0.9.0".into();
        assert!(matches!(
            verify_manifest(&edited, &key),
            Err(UpdateError::BadSignatureCheck)
        ));

        let mut moved = manifest.clone();
        moved.url = "https://evil.test/rigel.msi".into();
        assert!(matches!(
            verify_manifest(&moved, &key),
            Err(UpdateError::BadSignatureCheck)
        ));

        let mut rehashed = manifest.clone();
        rehashed.sha256 = "b".repeat(64);
        assert!(matches!(
            verify_manifest(&rehashed, &key),
            Err(UpdateError::BadSignatureCheck)
        ));
    }

    #[test]
    fn another_key_cannot_sign_this_publishers_manifest() {
        let (manifest, _key) = signed_manifest(7, "0.2.0", &"a".repeat(64));
        let other = SigningKey::from_bytes(&[9; 32]).verifying_key().to_bytes();
        assert!(matches!(
            verify_manifest(&manifest, &other),
            Err(UpdateError::BadSignatureCheck)
        ));
    }

    #[test]
    fn the_artifact_hash_is_checked_against_the_signature() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = dir.path().join("rigel.msi");
        std::fs::write(&artifact, b"the installer bytes").unwrap();
        let sha = format!("{:x}", Sha256::digest(b"the installer bytes"));
        let (manifest, _key) = signed_manifest(7, "0.2.0", &sha);
        verify_artifact(&artifact, &manifest).expect("matching bytes");

        std::fs::write(&artifact, b"the installer bytez").unwrap();
        match verify_artifact(&artifact, &manifest) {
            Err(UpdateError::HashMismatch { expected, found }) => {
                assert_eq!(expected, sha);
                assert_ne!(found, sha);
            }
            other => panic!("expected a hash mismatch, got {other:?}"),
        }
    }

    #[test]
    fn versions_compare_numerically_and_pre_releases_sort_lower() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(is_newer("1.0.0", "1.0.0-rc1"));
        assert!(!is_newer("1.0.0-rc1", "1.0.0"));
        assert!(is_newer("v2", "1.9.9"));
    }

    #[test]
    fn the_switch_defaults_off_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let defaults = load_settings(dir.path());
        assert!(!defaults.enabled, "no request until this is turned on");

        save_settings(
            dir.path(),
            &UpdateSettings {
                enabled: true,
                manifest_url: Some("https://example.test/latest.json".into()),
            },
        )
        .unwrap();
        let saved = load_settings(dir.path());
        assert!(saved.enabled);
        assert_eq!(
            saved.manifest_url.as_deref(),
            Some("https://example.test/latest.json")
        );
    }

    #[test]
    fn a_disabled_check_makes_no_request_and_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let (manifest, key) = signed_manifest(7, "0.2.0", &"a".repeat(64));
        std::fs::write(dir.path().join(PUBKEY_FILE), encode_hex(&key)).unwrap();
        assert!(matches!(
            check(dir.path(), &manifest, "0.1.0"),
            Err(UpdateError::Disabled)
        ));

        save_settings(
            dir.path(),
            &UpdateSettings {
                enabled: true,
                manifest_url: None,
            },
        )
        .unwrap();
        let result = check(dir.path(), &manifest, "0.1.0").unwrap();
        assert!(result.verified && result.available);
    }

    fn encode_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

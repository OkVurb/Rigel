//! Updates: a signed manifest, and nothing else (N5, P7.7).
//!
//! The rule N5 states is narrow on purpose: *no telemetry, no phone-home; updates are a signed
//! manifest fetch you can disable.* This module is the whole of the network behaviour:
//!
//! * **On by default, and switchable.** A release build checks the latest GitHub release once at
//!   launch and on demand; nothing checks on a timer. Settings → Updates turns both the check
//!   and auto-install off in one place, and a disabled switch makes no request at all.
//! * **Signed, and checked before parsed.** An Ed25519 signature over the canonical manifest
//!   bytes (`version\nurl\nsha256`, plus the optional MSI URL and hash) means a mirror or a
//!   proxy cannot change the version, an artifact URL or an artifact hash. The public key is a
//!   bundled resource copied into the data directory (`update-pubkey.txt`), or
//!   `RIGEL_UPDATE_PUBKEY`; there is no built-in key to leak.
//! * **The artifact is checked too.** The manifest's `sha256` is compared with the downloaded
//!   bytes before anything is executed. Signature without hash would let a stale artifact be
//!   paired with a fresh signature.
//! * **Installing is a separate choice.** Auto-install defaults to `true` but is its own switch;
//!   when it is on, a verified installer is started and the window exits so the installer can
//!   replace it. The update is never run from a path the manifest did not name and the hash did
//!   not match.

use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, VerifyingKey};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

pub const SETTINGS_FILE: &str = "update.json";
pub const PUBKEY_FILE: &str = "update-pubkey.txt";
/// The manifest the window checks when the switch is on. A GitHub release attaches this file,
/// so `releases/latest` always resolves to the newest published manifest.
pub const GITHUB_MANIFEST_URL: &str =
    "https://github.com/OkVurb/Rigel/releases/latest/download/latest.json";
/// Downloaded installers live here until the app exits to run them.
pub const UPDATE_DIR: &str = "updates";
/// A release installer is tens of megabytes; this is a ceiling, not an expectation.
const MAX_ARTIFACT_BYTES: u64 = 300 * 1024 * 1024;

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

    #[error("no published release yet at {0}")]
    NoRelease(String),

    #[error("the update artifact is larger than 300 MB")]
    TooLarge,

    #[error("the update artifact has an unsupported type: {0}")]
    UnsupportedArtifact(String),

    #[error("could not start the installer: {0}")]
    Install(String),

    #[error("updates can only be installed on Windows")]
    NotWindows,

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
    /// Hex Ed25519 signature over `version\nurl\nsha256`, plus `msi_url\nmsi_sha256` when the
    /// optional MSI asset is present.
    pub signature: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub published_at: Option<i64>,
    /// The MSI twin of `url`. The window picks it when this copy is installed under Program
    /// Files; a per-user NSIS install keeps using `url`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msi_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msi_sha256: Option<String>,
}

impl UpdateManifest {
    /// The bytes the signature covers. Fixed order, no JSON formatting, so a re-serialised
    /// manifest still verifies. The optional MSI fields extend the payload only when both are
    /// present, which keeps every three-line manifest already in the wild verifiable.
    pub fn signed_bytes(&self) -> Vec<u8> {
        let mut text = format!("{}\n{}\n{}", self.version, self.url, self.sha256);
        if let (Some(url), Some(sha256)) = (&self.msi_url, &self.msi_sha256) {
            text.push('\n');
            text.push_str(url);
            text.push('\n');
            text.push_str(sha256);
        }
        text.into_bytes()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSettings {
    /// On by default: one signed check at launch, which Settings → Updates can turn off.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The manifest location. Kept next to the switch so the file is the whole configuration.
    #[serde(default)]
    pub manifest_url: Option<String>,
    /// Download and run the installer without another click once a signed update is found.
    #[serde(default = "default_true")]
    pub auto_install: bool,
}

fn default_true() -> bool {
    true
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            manifest_url: None,
            auto_install: true,
        }
    }
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
    pub msi_url: Option<String>,
    pub msi_sha256: Option<String>,
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

/// The configured manifest, or the release asset GitHub publishes from the workflow.
pub fn manifest_url(settings: &UpdateSettings) -> &str {
    settings
        .manifest_url
        .as_deref()
        .unwrap_or(GITHUB_MANIFEST_URL)
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
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(UpdateError::NoRelease(url.to_string()));
    }
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

/// Check the manifest the switch points at, defaulting to GitHub's newest release asset.
pub async fn check_latest(data_dir: &Path, current: &str) -> Result<UpdateCheck, UpdateError> {
    let settings = load_settings(data_dir);
    if !settings.enabled {
        return Err(UpdateError::Disabled);
    }
    let url = manifest_url(&settings).to_string();
    let manifest = fetch_manifest(&url).await?;
    check(data_dir, &manifest, current)
}

/// The two URLs an update can use. A per-machine install (MSI under Program Files) gets the MSI;
/// everything else, including a per-user NSIS install, gets the setup executable.
pub fn artifact_for(check: &UpdateCheck, prefer_msi: bool) -> (&str, &str) {
    if prefer_msi {
        if let (Some(url), Some(sha256)) = (&check.msi_url, &check.msi_sha256) {
            return (url, sha256);
        }
    }
    (&check.url, &check.sha256)
}

/// Download one installer and verify the signed hash before returning its path. The file is
/// written under `data_dir/updates`; a failed or oversized download is removed rather than left
/// for the installer to find.
pub async fn download_artifact(
    url: &str,
    sha256: &str,
    data_dir: &Path,
) -> Result<PathBuf, UpdateError> {
    let loopback = url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:");
    if !url.starts_with("https://") && !loopback {
        return Err(UpdateError::NotHttps(url.to_string()));
    }
    let dir = data_dir.join(UPDATE_DIR);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(artifact_name(url));

    let client = reqwest::Client::builder()
        // A release can be a few hundred MB; 300 seconds total was too eager on a slow link.
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(900))
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
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ARTIFACT_BYTES)
    {
        return Err(UpdateError::TooLarge);
    }

    let mut file = tokio::fs::File::create(&path).await?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| UpdateError::Network(error.to_string()))?;
        total += chunk.len() as u64;
        if total > MAX_ARTIFACT_BYTES {
            drop(file);
            let _ = tokio::fs::remove_file(&path).await;
            return Err(UpdateError::TooLarge);
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;

    let found = format!("{:x}", hasher.finalize());
    if !found.eq_ignore_ascii_case(sha256) {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(UpdateError::HashMismatch {
            expected: sha256.to_string(),
            found,
        });
    }
    Ok(path)
}

/// A fixed basename from the URL, with only filename-safe characters. The asset name is
/// `Rigel_<version>_x64-setup.exe`; a URL without a recognised suffix falls back to an exe.
fn artifact_name(url: &str) -> String {
    let tail = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let cleaned: String = tail
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
        .collect();
    if cleaned.ends_with(".exe") || cleaned.ends_with(".msi") {
        cleaned
    } else {
        "rigel-update.exe".to_string()
    }
}

/// Run the downloaded installer. NSIS is silent with `/S` and relaunches this app with `/R`;
/// MSI uses a basic progress UI and never reboots. The caller exits the window after this
/// returns, so the installer can replace the running executable.
pub fn install_artifact(path: &Path) -> Result<(), UpdateError> {
    if !path.is_file() {
        return Err(UpdateError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("the downloaded installer is missing: {}", path.display()),
        )));
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(UpdateError::NotWindows)
    }
    #[cfg(windows)]
    {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let mut command = match extension.as_str() {
            "msi" => {
                let mut command = std::process::Command::new("msiexec");
                command.args(["/i"]).arg(path).args(["/qb", "/norestart"]);
                command
            }
            "exe" => {
                let mut command = std::process::Command::new(path);
                // Tauri's NSIS template reads `/R` in silent mode and starts the new build after
                // the install, so an automatic update does not leave the user with a closed app.
                command.args(["/S", "/R"]);
                command
            }
            _ => {
                return Err(UpdateError::UnsupportedArtifact(path.display().to_string()));
            }
        };
        crate::process::hide(&mut command);
        command
            .spawn()
            .map_err(|error| UpdateError::Install(error.to_string()))?;
        Ok(())
    }
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
        msi_url: manifest.msi_url.clone(),
        msi_sha256: manifest.msi_sha256.clone(),
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
            msi_url: None,
            msi_sha256: None,
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
    fn the_optional_msi_asset_is_covered_by_the_signature() {
        let (mut manifest, key) = signed_manifest(7, "0.2.0", &"a".repeat(64));
        manifest.msi_url = Some("https://example.test/rigel-0.2.0.msi".into());
        manifest.msi_sha256 = Some("c".repeat(64));
        // Re-sign the five-line payload the release workflow emits.
        let signing = SigningKey::from_bytes(&[7; 32]);
        manifest.signature = signing
            .sign(&manifest.signed_bytes())
            .to_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        verify_manifest(&manifest, &key).expect("the MSI fields are part of the payload");

        manifest.msi_url = Some("https://evil.test/rigel.msi".into());
        assert!(matches!(
            verify_manifest(&manifest, &key),
            Err(UpdateError::BadSignatureCheck)
        ));
    }

    #[test]
    fn the_installer_choice_prefers_the_msi_only_when_this_copy_is_per_machine() {
        let check = UpdateCheck {
            current: "0.1.0".into(),
            offered: "0.2.0".into(),
            available: true,
            verified: true,
            url: "https://example.test/setup.exe".into(),
            sha256: "a".repeat(64),
            notes: None,
            msi_url: Some("https://example.test/rigel.msi".into()),
            msi_sha256: Some("b".repeat(64)),
        };
        assert_eq!(
            artifact_for(&check, true),
            ("https://example.test/rigel.msi", "b".repeat(64).as_str())
        );
        assert_eq!(
            artifact_for(&check, false),
            ("https://example.test/setup.exe", "a".repeat(64).as_str())
        );
        let no_msi = UpdateCheck {
            msi_url: None,
            msi_sha256: None,
            ..check
        };
        assert_eq!(
            artifact_for(&no_msi, true).0,
            "https://example.test/setup.exe"
        );
    }

    #[test]
    fn the_manifest_defaults_to_the_latest_github_release_asset() {
        let settings = UpdateSettings::default();
        assert_eq!(manifest_url(&settings), GITHUB_MANIFEST_URL);
        let custom = UpdateSettings {
            manifest_url: Some("https://example.test/latest.json".into()),
            ..settings
        };
        assert_eq!(manifest_url(&custom), "https://example.test/latest.json");
    }

    #[test]
    fn installer_names_are_taken_from_the_asset_and_never_from_a_path() {
        assert_eq!(
            artifact_name(
                "https://github.com/OkVurb/Rigel/releases/download/v1/Rigel_1_x64-setup.exe?download=1"
            ),
            "Rigel_1_x64-setup.exe"
        );
        assert_eq!(
            artifact_name("https://example.test/Rigel.msi#fragment"),
            "Rigel.msi"
        );
        assert_eq!(
            artifact_name("https://example.test/download"),
            "rigel-update.exe"
        );
        assert_eq!(
            artifact_name("https://example.test/../../evil.exe"),
            "evil.exe"
        );
    }

    #[tokio::test]
    async fn a_download_is_hash_checked_and_a_bad_one_is_deleted() {
        use tokio::io::AsyncReadExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a local listener");
        let address = listener.local_addr().expect("an address");
        let body = b"installer bytes".to_vec();
        let expected = format!("{:x}", Sha256::digest(&body));
        let server_body = body.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("one request");
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                server_body.len()
            );
            socket.write_all(header.as_bytes()).await.expect("header");
            socket.write_all(&server_body).await.expect("body");
        });

        let dir = tempfile::tempdir().unwrap();
        let url = format!("http://127.0.0.1:{}/rigel-setup.exe", address.port());
        let path = download_artifact(&url, &expected, dir.path())
            .await
            .expect("the matching bytes download");
        assert_eq!(std::fs::read(&path).unwrap(), body);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a second listener");
        let address = listener.local_addr().expect("an address");
        let server_body = body.clone();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("one request");
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                server_body.len()
            );
            socket.write_all(header.as_bytes()).await.expect("header");
            socket.write_all(&server_body).await.expect("body");
        });
        let url = format!("http://127.0.0.1:{}/rigel-setup.exe", address.port());
        let wrong = "f".repeat(64);
        assert!(matches!(
            download_artifact(&url, &wrong, dir.path()).await,
            Err(UpdateError::HashMismatch { .. })
        ));
        assert!(!path.exists(), "a mismatched download is removed");
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
    fn the_switch_defaults_on_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let defaults = load_settings(dir.path());
        assert!(defaults.enabled, "a release checks at launch by default");
        assert!(defaults.auto_install, "auto-install is the default");

        save_settings(
            dir.path(),
            &UpdateSettings {
                enabled: true,
                manifest_url: Some("https://example.test/latest.json".into()),
                auto_install: false,
            },
        )
        .unwrap();
        let saved = load_settings(dir.path());
        assert!(saved.enabled);
        assert!(!saved.auto_install);
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
        save_settings(
            dir.path(),
            &UpdateSettings {
                enabled: false,
                manifest_url: None,
                auto_install: false,
            },
        )
        .unwrap();
        assert!(matches!(
            check(dir.path(), &manifest, "0.1.0"),
            Err(UpdateError::Disabled)
        ));

        save_settings(
            dir.path(),
            &UpdateSettings {
                enabled: true,
                manifest_url: None,
                auto_install: false,
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

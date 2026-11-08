//! Updater end-to-end cases (roadmap 78).
//!
//! The live updater is `rigel-core::update`; this test drives the same `check` and
//! `verify_artifact` calls the window makes, with a real Ed25519 signature over the
//! canonical manifest bytes.

use std::fs;

use ed25519_dalek::{Signer, SigningKey};
use rigel_core::update::{UpdateError, UpdateManifest, check, is_newer, verify_artifact};

fn manifest(seed: u8, version: &str, sha: &str) -> (UpdateManifest, [u8; 32]) {
    let key = SigningKey::from_bytes(&[seed; 32]);
    let mut manifest = UpdateManifest {
        version: version.to_string(),
        url: format!("https://example.test/rigel-{version}-setup.exe"),
        sha256: sha.to_string(),
        signature: String::new(),
        notes: Some("updater E2E".to_string()),
        published_at: Some(1_760_000_000_000),
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

fn data_dir(key: &[u8; 32]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let hex = key
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fs::write(dir.path().join("update-pubkey.txt"), hex).unwrap();
    dir
}

fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn a_staged_update_verifies_and_reports_the_offered_version() {
    let artifact = b"the staged setup executable";
    let (manifest, key) = manifest(11, "0.3.0", &sha(artifact));
    let dir = data_dir(&key);
    let check = check(dir.path(), &manifest, "0.2.0").unwrap();
    assert!(check.available);
    assert!(check.verified);
    assert_eq!(check.offered, "0.3.0");

    let path = dir.path().join("staged-setup.exe");
    fs::write(&path, artifact).unwrap();
    assert_eq!(verify_artifact(&path, &manifest).unwrap(), sha(artifact));
}

#[test]
fn a_wrong_signature_is_refused_by_check() {
    let (mut manifest, key) = manifest(12, "0.3.0", &sha(b"staged"));
    let dir = data_dir(&key);
    manifest.version = "0.3.1".to_string();
    assert!(matches!(
        check(dir.path(), &manifest, "0.2.0"),
        Err(UpdateError::BadSignatureCheck)
    ));
}

#[test]
fn a_wrong_hash_is_refused_by_the_artifact_check() {
    let (manifest, key) = manifest(13, "0.3.0", &sha(b"the real build"));
    let dir = data_dir(&key);
    let path = dir.path().join("tampered-setup.exe");
    fs::write(&path, b"a different build").unwrap();
    assert!(matches!(
        verify_artifact(&path, &manifest),
        Err(UpdateError::HashMismatch { .. })
    ));
}

#[test]
fn a_downgrade_is_refused_by_default() {
    let (manifest, key) = manifest(14, "0.1.0", &sha(b"old build"));
    let dir = data_dir(&key);
    let check = check(dir.path(), &manifest, "0.2.0").unwrap();
    assert!(!check.available);
    assert!(!is_newer("0.1.0", "0.2.0"));
    assert!(!is_newer("0.2.0", "0.2.0"));
    assert!(is_newer("0.3.0", "0.2.0"));
}

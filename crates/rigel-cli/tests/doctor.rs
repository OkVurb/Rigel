//! `rigel doctor` as a program: rows, evidence and an exit code a script can read.
//!
//! The binary is run rather than the functions called, because P4.12 and P7.1 are promises
//! about *this command's* output: one row per check, one row per harness, and an exit code
//! that distinguishes a warning from a failure.

use std::path::Path;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn rigel(data: &Path, args: &[&str]) -> (i32, Value) {
    let exe = env!("CARGO_BIN_EXE_rigel");
    let output = Command::new(exe)
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .output()
        .expect("rigel runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|error| {
        panic!(
            "not json: {error}\n{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap_or(-1), json)
}

fn status_of<'a>(json: &'a Value, id: &str) -> Option<&'a str> {
    json["checks"]
        .as_array()?
        .iter()
        .find(|check| check["id"] == id)
        .and_then(|check| check["status"].as_str())
}

#[tokio::test]
async fn a_clean_machine_has_no_failures_and_one_row_per_harness() {
    let data = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();

    let (code, json) = rigel(
        data.path(),
        &[
            "doctor",
            "--project",
            project.path().to_str().unwrap(),
            "--json",
        ],
    );

    assert_eq!(status_of(&json, "db.index.integrity"), Some("ok"), "{json}");
    assert_eq!(status_of(&json, "schema.index"), Some("ok"));
    assert_eq!(status_of(&json, "db.project"), Some("ok"));

    // One harness row each, whatever their state: a missing harness is a warning, not a
    // missing row (P4.12).
    let harnesses: Vec<&Value> = json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|check| {
            check["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("harness:"))
        })
        .collect();
    assert_eq!(harnesses.len(), 4, "{json}");
    assert!(harnesses.iter().all(|row| row["detail"].is_string()));

    assert_eq!(
        json["exit_code"].as_i64(),
        Some(code as i64),
        "the reported code matches the process status"
    );
    // The code is the summary: failures are 2, warnings alone are 1, neither is 0.
    let failures = json["failures"].as_i64().unwrap_or(0);
    let warnings = json["warnings"].as_i64().unwrap_or(0);
    let expected = if failures > 0 {
        2
    } else if warnings > 0 {
        1
    } else {
        0
    };
    assert_eq!(code, expected, "{json}");
}

#[tokio::test]
async fn a_dangling_blob_is_a_failure_with_the_count_and_an_exit_code() {
    let data = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();

    // Create the index, then point a skill at a body that is not on disk.
    rigel(data.path(), &["skills", "stats", "--json"]);
    let store = rigel_core::Store::open_index(data.path()).await.unwrap();
    sqlx::query(
        "INSERT INTO skills (id, name, source, body_sha, first_seen_at, last_seen_at)
         VALUES ('local:ghost', 'ghost', 'local', 'deadbeef', 1, 1)",
    )
    .execute(store.pool())
    .await
    .unwrap();

    let (code, json) = rigel(
        data.path(),
        &[
            "doctor",
            "--project",
            project.path().to_str().unwrap(),
            "--json",
        ],
    );

    assert_eq!(code, 2, "{json}");
    assert_eq!(json["exit_code"].as_i64(), Some(2));
    assert_eq!(status_of(&json, "blobs"), Some("fail"));
    let detail = json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "blobs")
        .unwrap()["detail"]
        .as_str()
        .unwrap();
    assert!(detail.contains("gone"), "{detail}");
    assert!(detail.contains('1'), "{detail}");
}

#[tokio::test]
async fn a_project_database_is_checked_without_being_created_by_doctor() {
    let data = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();

    // No project database yet: doctor says so and does not create one.
    let (_, json) = rigel(
        data.path(),
        &[
            "doctor",
            "--project",
            project.path().to_str().unwrap(),
            "--json",
        ],
    );
    let detail = json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "db.project")
        .unwrap()["detail"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(detail.contains("not created yet"), "{detail}");
    assert!(!project.path().join(".rigel").join("rigel.db").exists());

    // Create it through the documented path, then doctor reads it and adds the project rows.
    let created = Command::new(env!("CARGO_BIN_EXE_rigel"))
        .arg("--data-dir")
        .arg(data.path())
        .args(["migrate", "--project", project.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "migrate failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );

    let (code, json) = rigel(
        data.path(),
        &[
            "doctor",
            "--project",
            project.path().to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(
        status_of(&json, "db.project.integrity"),
        Some("ok"),
        "{json}"
    );
    assert_eq!(status_of(&json, "schema.project"), Some("ok"));
    assert!(status_of(&json, "claims").is_some());
    assert!(status_of(&json, "budget").is_some());
    assert!(code == 0 || code == 1, "only warnings are allowed: {json}");
}

#[tokio::test]
async fn an_install_that_never_finished_is_a_warning_with_its_own_row() {
    let data = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();

    rigel(data.path(), &["skills", "stats", "--json"]);
    let store = rigel_core::Store::open_index(data.path()).await.unwrap();
    sqlx::query(
        "INSERT INTO install_ledger
           (id, name, package_id, command, trust_score, decision, host, verification)
         VALUES ('half', 'ripgrep', 'BurntSushi.ripgrep.MSVC',
                 'winget install --id BurntSushi.ripgrep.MSVC -e', 98.0,
                 'user-approved', 'windows', 'winget hashes the installer')",
    )
    .execute(store.pool())
    .await
    .unwrap();

    let (code, json) = rigel(
        data.path(),
        &[
            "doctor",
            "--project",
            project.path().to_str().unwrap(),
            "--json",
        ],
    );

    assert_eq!(status_of(&json, "installs"), Some("warn"), "{json}");
    assert_eq!(code, 1, "{json}");
    let detail = json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "installs")
        .unwrap()["detail"]
        .as_str()
        .unwrap();
    assert!(detail.contains("never finished"), "{detail}");
}

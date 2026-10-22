//! Pre-destructive snapshots against real git repositories and plain directories.

use std::path::Path;
use std::process::Command;

use rigel_core::{MAX_SNAPSHOT_FILES, SnapshotError, SnapshotKind, SnapshotService};

fn git(root: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn has_git() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    assert!(git(tmp.path(), &["init", "-q"]));
    assert!(git(tmp.path(), &["config", "user.email", "rigel@test"]));
    assert!(git(tmp.path(), &["config", "user.name", "Rigel Test"]));
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/lib.rs"), "fn version() -> u8 { 1 }\n").unwrap();
    std::fs::write(tmp.path().join(".gitignore"), "ignored.txt\n").unwrap();
    assert!(git(tmp.path(), &["add", "."]));
    assert!(git(
        tmp.path(),
        &["commit", "-q", "-m", "the committed state"]
    ));
    tmp
}

fn service(root: &Path) -> (SnapshotService, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    (SnapshotService::new(root, data.path()), data)
}

#[test]
fn a_git_snapshot_restores_tracked_and_untracked_files() {
    if !has_git() {
        return;
    }
    let tmp = repo();
    std::fs::write(tmp.path().join("src/lib.rs"), "fn version() -> u8 { 2 }\n").unwrap();
    std::fs::write(tmp.path().join("notes.txt"), "remember the tests\n").unwrap();
    std::fs::write(tmp.path().join("ignored.txt"), "ignored before\n").unwrap();

    let (snapshots, _data) = service(tmp.path());
    let snapshot = snapshots.capture("the tests").unwrap();

    assert_eq!(snapshot.kind, SnapshotKind::Git);
    assert!(snapshot.stash.is_some(), "tracked change should be stashed");
    let manifest = snapshots.load(&snapshot.id).unwrap();
    assert_eq!(
        manifest.root,
        tmp.path().canonicalize().unwrap().to_string_lossy()
    );
    assert_eq!(manifest.files.len(), 2, "{:?}", manifest.files);
    let paths: Vec<&str> = manifest
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert!(paths.contains(&"notes.txt"), "{paths:?}");
    assert!(
        paths.contains(&"src/lib.rs"),
        "the changed tracked file is copied raw for byte-identical restore"
    );
    assert!(
        manifest.files.iter().all(|file| file.path != "ignored.txt"),
        "an ignored file is not part of the snapshot"
    );

    // What a destructive command might do: rewrite tracked work and delete the untracked copy.
    std::fs::write(tmp.path().join("src/lib.rs"), "destroyed\n").unwrap();
    std::fs::remove_file(tmp.path().join("notes.txt")).unwrap();

    let report = snapshots.restore(&snapshot.id, tmp.path()).unwrap();
    assert!(report.git_restored);
    assert_eq!(report.files_restored, 2);
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("src/lib.rs")).unwrap(),
        "fn version() -> u8 { 2 }\n"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("notes.txt")).unwrap(),
        "remember the tests\n"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("ignored.txt")).unwrap(),
        "ignored before\n"
    );
}

#[test]
fn a_deleted_tracked_file_comes_back_deleted() {
    if !has_git() {
        return;
    }
    let tmp = repo();
    std::fs::remove_file(tmp.path().join("src/lib.rs")).unwrap();

    let (snapshots, _data) = service(tmp.path());
    let snapshot = snapshots.capture("deleted a file").unwrap();
    let manifest = snapshots.load(&snapshot.id).unwrap();
    assert_eq!(manifest.deleted, vec!["src/lib.rs".to_string()]);

    // Simulate a command that recreates it; restore must reproduce the deleted state.
    std::fs::write(tmp.path().join("src/lib.rs"), "recreated\n").unwrap();
    snapshots.restore(&snapshot.id, tmp.path()).unwrap();
    assert!(!tmp.path().join("src/lib.rs").exists());
}

#[test]
fn the_stash_object_is_kept_by_a_ref() {
    if !has_git() {
        return;
    }
    let tmp = repo();
    std::fs::write(tmp.path().join("src/lib.rs"), "changed\n").unwrap();
    let (snapshots, _data) = service(tmp.path());
    let snapshot = snapshots.capture("keep the object").unwrap();
    let manifest = snapshots.load(&snapshot.id).unwrap();

    let reference = manifest.git_ref.expect("a ref keeps the object alive");
    let output = Command::new("git")
        .arg("-C")
        .arg(tmp.path())
        .args(["rev-parse", &reference])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        manifest.stash.unwrap()
    );
}

#[test]
fn a_plain_directory_is_copied_whole_and_restored() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("nested/deeper")).unwrap();
    std::fs::write(tmp.path().join("a.txt"), "alpha\n").unwrap();
    std::fs::write(tmp.path().join("nested/deeper/b.txt"), "beta\n").unwrap();
    std::fs::create_dir_all(tmp.path().join(".rigel")).unwrap();
    std::fs::write(tmp.path().join(".rigel/session.db"), "not project work").unwrap();

    let (snapshots, _data) = service(tmp.path());
    let snapshot = snapshots.capture("plain tree").unwrap();
    assert_eq!(snapshot.kind, SnapshotKind::Tree);
    assert_eq!(
        snapshot.file_count, 2,
        "the session store is not project work"
    );

    std::fs::write(tmp.path().join("a.txt"), "destroyed\n").unwrap();
    std::fs::remove_file(tmp.path().join("nested/deeper/b.txt")).unwrap();
    snapshots.restore(&snapshot.id, tmp.path()).unwrap();

    assert_eq!(
        std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "alpha\n"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("nested/deeper/b.txt")).unwrap(),
        "beta\n"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join(".rigel/session.db")).unwrap(),
        "not project work"
    );
}

#[test]
fn a_repository_with_no_commit_is_copied_whole() {
    if !has_git() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    assert!(git(tmp.path(), &["init", "-q"]));
    std::fs::write(tmp.path().join("first.txt"), "not committed\n").unwrap();

    let (snapshots, _data) = service(tmp.path());
    let snapshot = snapshots.capture("no commit").unwrap();
    assert_eq!(snapshot.kind, SnapshotKind::Tree);
    assert!(snapshot.stash.is_none());
    assert_eq!(snapshot.file_count, 1);
}

#[test]
fn the_cap_refuses_before_creating_a_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    for index in 0..=MAX_SNAPSHOT_FILES {
        std::fs::write(tmp.path().join(format!("file-{index}.txt")), "").unwrap();
    }
    let (snapshots, data) = service(tmp.path());

    match snapshots.capture("too many files") {
        Err(SnapshotError::TooLarge { files, .. }) => {
            assert_eq!(files, MAX_SNAPSHOT_FILES + 1);
        }
        other => panic!("expected TooLarge, got {other:?}"),
    }
    assert!(!data.path().join("snapshots").exists() || snapshots.list().unwrap().is_empty());
}

#[test]
fn restoring_into_a_different_project_is_refused() {
    if !has_git() {
        return;
    }
    let tmp = repo();
    std::fs::write(tmp.path().join("src/lib.rs"), "changed\n").unwrap();
    let (snapshots, _data) = service(tmp.path());
    let snapshot = snapshots.capture("wrong root").unwrap();

    let other = tempfile::tempdir().unwrap();
    match snapshots.restore(&snapshot.id, other.path()) {
        Err(SnapshotError::WrongRoot { .. }) => {}
        other => panic!("expected WrongRoot, got {other:?}"),
    }
}

#[test]
fn list_returns_the_snapshot_with_its_label_and_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
    let (snapshots, _data) = service(tmp.path());
    let snapshot = snapshots.capture("listed").unwrap();

    let listed = snapshots.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, snapshot.id);
    assert_eq!(listed[0].files.len(), 1);
}

//! Pre-destructive snapshots (D32, D38).
//!
//! The state worth protecting is the one with no other copy: the uncommitted tree. In a git
//! repository with a commit, tracked changes go into a `git stash create` object (which touches
//! neither the index nor the worktree) and untracked, non-ignored files are copied beside it.
//! A `refs/rigel/snapshots/<id>` ref keeps the object alive past git's next prune. A repository
//! without a commit, or a directory that is not a repository, gets its whole tree copied instead,
//! because there is no object store to hold it.
//!
//! This is not a disk image or a Windows restore point: those protect system files and leave
//! personal files alone, which is backwards for an agent working in a project. Restore is
//! `git checkout <sha> -- .` plus putting the copied files back, exposed as
//! `rigel snapshot restore <id>`.
//!
//! Every snapshot is capped at 2 000 files / 200 MB. Above that Rigel refuses and says so; it
//! never pretends a partial copy is a backup.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

pub const MAX_SNAPSHOT_FILES: usize = 2_000;
pub const MAX_SNAPSHOT_BYTES: u64 = 200 * 1024 * 1024;

const MANIFEST: &str = "manifest.json";
const FILES: &str = "files";

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("git: {0}")]
    Git(String),
    #[error(
        "refusing to snapshot {files} files / {bytes} bytes: past the 2 000-file / {max_bytes}-byte cap. This tree has no other copy; reduce it or commit it first"
    )]
    TooLarge {
        files: usize,
        bytes: u64,
        max_bytes: u64,
    },
    #[error("snapshot {0} does not exist")]
    NotFound(String),
    #[error("{0} is not a snapshot id")]
    BadId(String),
    #[error("this snapshot was taken in {snapshot}; {target} is a different project")]
    WrongRoot { snapshot: String, target: String },
    #[error("manifest: {0}")]
    Manifest(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    /// `git stash create` for tracked changes plus copied untracked files.
    Git,
    /// A whole-tree copy for a non-git directory or a repository with no commit.
    Tree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Project-relative, with `/` separators.
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub created_at: i64,
    /// Absolute project root when the snapshot was taken.
    pub root: String,
    pub kind: SnapshotKind,
    /// `git stash create` commit, when there were tracked changes.
    #[serde(default)]
    pub stash: Option<String>,
    #[serde(default)]
    pub git_ref: Option<String>,
    /// Tracked paths that were deleted; `checkout` restores contents but not absences.
    #[serde(default)]
    pub deleted: Vec<String>,
    pub files: Vec<FileEntry>,
    pub total_bytes: u64,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub id: String,
    pub kind: SnapshotKind,
    pub stash: Option<String>,
    pub file_count: usize,
    pub total_bytes: u64,
    pub directory: PathBuf,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreReport {
    pub id: String,
    pub kind: SnapshotKind,
    pub files_restored: usize,
    pub git_restored: bool,
    pub total_bytes: u64,
}

/// One project's snapshot directory under the app data dir.
pub struct SnapshotService {
    root: PathBuf,
    base: PathBuf,
}

impl SnapshotService {
    /// `data_dir` is the app data directory (`%APPDATA%\Rigel`), not the project.
    pub fn new(root: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            base: data_dir.into().join("snapshots"),
        }
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Take one snapshot. Refuses above the cap before creating anything.
    pub fn capture(&self, label: &str) -> Result<Snapshot, SnapshotError> {
        let root = canonical(&self.root)?;
        let id = format!(
            "{}{}",
            now_ms(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );

        let git = GitRepo::detect(&root);
        let (kind, files) = match &git {
            Some(repo) if repo_has_head(&root) => {
                // The stash object holds tracked state, but checkout is subject to the user's
                // autocrlf/smudge filters. Copying the changed files raw is what makes restore
                // byte-identical; the stash still carries deletions and the audit trail.
                let mut files = repo.untracked(&root)?;
                files.extend(repo.changed(&root)?);
                files.sort_by(|left, right| left.0.cmp(&right.0));
                files.dedup_by(|left, right| left.0 == right.0);
                (SnapshotKind::Git, files)
            }
            _ => (
                SnapshotKind::Tree,
                walk_files(&root, &self.base).map_err(|error| SnapshotError::Io {
                    path: root.clone(),
                    source: error,
                })?,
            ),
        };
        let total_bytes: u64 = files.iter().map(|file| file.1).sum();
        if files.len() > MAX_SNAPSHOT_FILES || total_bytes > MAX_SNAPSHOT_BYTES {
            return Err(SnapshotError::TooLarge {
                files: files.len(),
                bytes: total_bytes,
                max_bytes: MAX_SNAPSHOT_BYTES,
            });
        }

        let directory = self.base.join(&id);
        fs::create_dir_all(directory.join(FILES)).map_err(|source| SnapshotError::Io {
            path: directory.clone(),
            source,
        })?;

        // Copy first, then make the git object: a copy that fails leaves no dangling ref.
        let mut copied = Vec::with_capacity(files.len());
        for (relative, _bytes) in files {
            let source_path = root.join(&relative);
            let destination = directory.join(FILES).join(&relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|source| SnapshotError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            match fs::copy(&source_path, &destination) {
                Ok(bytes) => copied.push(FileEntry {
                    path: relative,
                    bytes,
                }),
                Err(error) => {
                    let _ = fs::remove_dir_all(&directory);
                    return Err(SnapshotError::Io {
                        path: source_path,
                        source: error,
                    });
                }
            }
        }

        let (stash, git_ref, note) = match &git {
            Some(repo) if kind == SnapshotKind::Git => match repo.stash_create(&root, &id)? {
                Some(sha) => {
                    let reference = format!("refs/rigel/snapshots/{id}");
                    repo.update_ref(&root, &reference, &sha)?;
                    (
                        Some(sha),
                        Some(reference),
                        format!(
                            "tracked changes in git stash object, {} working-tree files copied",
                            copied.len()
                        ),
                    )
                }
                None => (
                    None,
                    None,
                    format!(
                        "no tracked changes, {} untracked files copied",
                        copied.len()
                    ),
                ),
            },
            _ => (
                None,
                None,
                format!(
                    "whole tree copied ({}); no git commit to hold a stash object",
                    if git.is_some() {
                        "repository has no commit"
                    } else {
                        "not a git repository"
                    }
                ),
            ),
        };

        let manifest = Manifest {
            id: id.clone(),
            created_at: now_ms(),
            root: root.to_string_lossy().to_string(),
            kind,
            stash,
            git_ref,
            deleted: match &git {
                Some(repo) if kind == SnapshotKind::Git => repo.deleted(&root)?,
                _ => Vec::new(),
            },
            files: copied,
            total_bytes,
            note: note.clone(),
        };
        write_manifest(&directory, &manifest)?;
        if !label.trim().is_empty() {
            // The label is for humans and is not consulted by restore.
            let _ = fs::write(directory.join("label.txt"), label.trim());
        }

        Ok(Snapshot {
            id,
            kind,
            stash: manifest.stash,
            file_count: manifest.files.len(),
            total_bytes: manifest.total_bytes,
            directory,
            note,
        })
    }

    pub fn list(&self) -> Result<Vec<Manifest>, SnapshotError> {
        let mut manifests = Vec::new();
        let entries = match fs::read_dir(&self.base) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(manifests),
            Err(source) => {
                return Err(SnapshotError::Io {
                    path: self.base.clone(),
                    source,
                });
            }
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }
            let path = entry.path().join(MANIFEST);
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(manifest) = serde_json::from_str::<Manifest>(&text) {
                    manifests.push(manifest);
                }
            }
        }
        manifests.sort_by_key(|manifest| std::cmp::Reverse(manifest.created_at));
        Ok(manifests)
    }

    pub fn load(&self, id: &str) -> Result<Manifest, SnapshotError> {
        if !valid_id(id) {
            return Err(SnapshotError::BadId(id.to_string()));
        }
        let path = self.base.join(id).join(MANIFEST);
        let text = fs::read_to_string(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SnapshotError::NotFound(id.to_string())
            } else {
                SnapshotError::Io {
                    path,
                    source: error,
                }
            }
        })?;
        serde_json::from_str(&text).map_err(|error| SnapshotError::Manifest(error.to_string()))
    }

    /// Put a snapshot back. Refuses to restore into a different project: a snapshot records
    /// absolute paths, and restoring elsewhere would silently write the wrong tree.
    pub fn restore(&self, id: &str, target: &Path) -> Result<RestoreReport, SnapshotError> {
        let manifest = self.load(id)?;
        let target = canonical(target)?;
        let recorded = PathBuf::from(&manifest.root);
        if !same_root(&recorded, &target) {
            return Err(SnapshotError::WrongRoot {
                snapshot: manifest.root.clone(),
                target: target.to_string_lossy().to_string(),
            });
        }
        let directory = self.base.join(&manifest.id);

        let mut git_restored = false;
        if let Some(stash) = &manifest.stash {
            let repo = GitRepo::detect(&target).ok_or_else(|| {
                SnapshotError::Git(
                    "the snapshot recorded a stash but this is not a repository".to_string(),
                )
            })?;
            repo.checkout_tree(&target, stash)?;
            git_restored = true;
        }

        for path in &manifest.deleted {
            let doomed = target.join(path);
            match fs::remove_file(&doomed) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(SnapshotError::Io {
                        path: doomed,
                        source,
                    });
                }
            }
        }

        let mut files_restored = 0;
        for entry in &manifest.files {
            let source = directory.join(FILES).join(&entry.path);
            let destination = target.join(&entry.path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|source| SnapshotError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            fs::copy(&source, &destination).map_err(|source| SnapshotError::Io {
                path: destination,
                source,
            })?;
            files_restored += 1;
        }

        Ok(RestoreReport {
            id: manifest.id,
            kind: manifest.kind,
            files_restored,
            git_restored,
            total_bytes: manifest.total_bytes,
        })
    }
}

struct GitRepo {
    executable: String,
}

impl GitRepo {
    fn detect(root: &Path) -> Option<Self> {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--is-inside-work-tree"]);
        crate::process::hide(&mut command);
        let output = command.output().ok()?;
        let inside = String::from_utf8_lossy(&output.stdout);
        output
            .status
            .success()
            .then(|| inside.trim() == "true")
            .filter(|inside| *inside)?;
        Some(Self {
            executable: "git".to_string(),
        })
    }

    fn run(&self, root: &Path, args: &[&str]) -> Result<std::process::Output, SnapshotError> {
        let mut command = Command::new(&self.executable);
        command.arg("-C").arg(root).args(args);
        crate::process::hide(&mut command);
        command
            .output()
            .map_err(|error| SnapshotError::Git(format!("could not run git: {error}")))
    }

    fn head(root: &Path) -> Option<String> {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--verify", "HEAD"]);
        crate::process::hide(&mut command);
        let output = command.output().ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Modified tracked files, including staged ones. Deletions have no bytes to copy and are
    /// recorded separately by [`Self::deleted`].
    fn changed(&self, root: &Path) -> Result<Vec<(String, u64)>, SnapshotError> {
        let output = self.run(root, &["diff", "--name-only", "-z", "--no-renames", "HEAD"])?;
        if !output.status.success() {
            return Err(SnapshotError::Git(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        let mut files = Vec::new();
        for path in String::from_utf8_lossy(&output.stdout)
            .split('\0')
            .filter(|path| !path.is_empty())
        {
            if path.starts_with(".rigel/") || path == ".rigel" {
                continue;
            }
            let full = root.join(path);
            if let Ok(metadata) = fs::metadata(&full) {
                if metadata.is_file() {
                    files.push((path.replace('\\', "/"), metadata.len()));
                }
            }
        }
        Ok(files)
    }

    fn untracked(&self, root: &Path) -> Result<Vec<(String, u64)>, SnapshotError> {
        let output = self.run(root, &["ls-files", "-z", "--others", "--exclude-standard"])?;
        if !output.status.success() {
            return Err(SnapshotError::Git(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        let mut files = Vec::new();
        for path in String::from_utf8_lossy(&output.stdout)
            .split('\0')
            .filter(|path| !path.is_empty())
        {
            if path.starts_with(".rigel/") || path == ".rigel" {
                continue;
            }
            let full = root.join(path);
            if let Ok(metadata) = fs::metadata(&full) {
                if metadata.is_file() {
                    files.push((path.replace('\\', "/"), metadata.len()));
                }
            }
        }
        Ok(files)
    }

    fn deleted(&self, root: &Path) -> Result<Vec<String>, SnapshotError> {
        let output = self.run(root, &["diff", "--name-only", "--diff-filter=D", "HEAD"])?;
        if !output.status.success() {
            return Err(SnapshotError::Git(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.trim().replace('\\', "/"))
            .filter(|line| !line.is_empty())
            .collect())
    }

    fn stash_create(&self, root: &Path, id: &str) -> Result<Option<String>, SnapshotError> {
        let output = self.run(root, &["stash", "create", &format!("rigel snapshot {id}")])?;
        if !output.status.success() {
            return Err(SnapshotError::Git(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((!sha.is_empty()).then_some(sha))
    }

    fn update_ref(&self, root: &Path, reference: &str, sha: &str) -> Result<(), SnapshotError> {
        let output = self.run(root, &["update-ref", reference, sha])?;
        if output.status.success() {
            Ok(())
        } else {
            Err(SnapshotError::Git(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ))
        }
    }

    fn checkout_tree(&self, root: &Path, sha: &str) -> Result<(), SnapshotError> {
        let output = self.run(root, &["checkout", sha, "--", "."])?;
        if output.status.success() {
            Ok(())
        } else {
            Err(SnapshotError::Git(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ))
        }
    }
}

/// Whether the repository has a commit. `git stash create` needs one; without it the tree is
/// copied whole, exactly like a non-git directory.
fn repo_has_head(root: &Path) -> bool {
    GitRepo::head(root).is_some()
}

fn walk_files(root: &Path, base: &Path) -> std::io::Result<Vec<(String, u64)>> {
    let mut files = Vec::new();
    walk(root, root, base, &mut files, 0)?;
    Ok(files)
}

fn walk(
    root: &Path,
    directory: &Path,
    base: &Path,
    files: &mut Vec<(String, u64)>,
    depth: usize,
) -> std::io::Result<()> {
    if depth > 64 {
        return Ok(());
    }
    let entries = fs::read_dir(directory)?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" || name == ".rigel" {
            continue;
        }
        let path = entry.path();
        if same_root(&path, base) {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            walk(root, &path, base, files, depth + 1)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, metadata.len()));
        }
    }
    Ok(())
}

fn write_manifest(directory: &Path, manifest: &Manifest) -> Result<(), SnapshotError> {
    let text = serde_json::to_string_pretty(manifest)
        .map_err(|error| SnapshotError::Manifest(error.to_string()))?;
    let path = directory.join(MANIFEST);
    let temporary = directory.join("manifest.json.tmp");
    fs::write(&temporary, text).map_err(|source| SnapshotError::Io {
        path: temporary.clone(),
        source,
    })?;
    fs::rename(&temporary, &path).map_err(|source| SnapshotError::Io { path, source })
}

fn canonical(path: &Path) -> Result<PathBuf, SnapshotError> {
    path.canonicalize().map_err(|source| SnapshotError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn same_root(left: &Path, right: &Path) -> bool {
    let clean = |path: &Path| {
        let text = path.to_string_lossy().replace('\\', "/");
        let text = text.strip_prefix("//?/").unwrap_or(&text).to_string();
        if cfg!(windows) {
            text.trim_end_matches('/').to_ascii_lowercase()
        } else {
            text.trim_end_matches('/').to_string()
        }
    };
    clean(left) == clean(right)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

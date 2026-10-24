//! One git worktree per spawned agent, merged back through the patch pipeline (P5.5).
//!
//! Why a worktree at all: two agents editing one tree is the thing claims *advertise* rather
//! than enforce. A worktree gives each agent its own checkout, so a subagent's half-finished
//! edit cannot land in the main tree by accident. The merge back is where the honesty lives:
//!
//! 1. each changed file is compared with the **base commit's** copy;
//! 2. if the main tree still matches that base, the worktree's content becomes a normal patch —
//!    journalled, CAS-checked, revertible;
//! 3. if the main tree moved, the file is reported `conflicted` and **nothing is written**. There
//!    is no three-way text merge here; picking a winner silently is exactly the behaviour the
//!    product exists to avoid.
//!
//! `git worktree add --detach` is used rather than a branch: a branch would show up in the
//! user's `git branch` output and survive a removal. Detached HEADs are disposable, which is
//! what a subagent's checkout should be.
//!
//! Deletions and renames are reported, not applied: the patch pipeline models content, not the
//! absence of a file. Pretending an empty file was a deletion is the kind of lie a diff view
//! cannot recover from.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use sqlx::SqlitePool;

use crate::patches::{PatchPipeline, sha256_empty, sha256_hex};

/// Where a worktree lives, under the per-user data directory rather than inside the project:
/// a checkout inside the repository would appear in its own `status` and in every diff.
pub const WORKTREES_DIR: &str = "worktrees";

#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("git is not installed or not on PATH")]
    GitMissing,

    #[error("git {command} failed: {message}")]
    Git { command: String, message: String },

    #[error("agent {0} has no worktree")]
    NotFound(String),

    #[error("{0} is not a git repository")]
    NotARepository(PathBuf),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("patch error: {0}")]
    Patch(#[from] crate::patches::PatchError),
}

#[derive(Debug, Clone, Serialize)]
pub struct Worktree {
    pub path: PathBuf,
    pub base: String,
    pub agent_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeStatus {
    Applied,
    /// The main tree changed since the base commit; nothing was written.
    Conflicted,
    /// The file is gone in the worktree. The pipeline cannot express a deletion.
    Deleted,
    /// A rename or a mode change; reported, not merged.
    Unsupported,
    /// The same bytes on both sides; no patch was needed.
    Unchanged,
}

#[derive(Debug, Clone, Serialize)]
pub struct MergeLine {
    pub path: String,
    pub status: MergeStatus,
    pub patch_id: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MergeReport {
    pub agent_id: String,
    pub base: String,
    pub lines: Vec<MergeLine>,
    pub applied: usize,
    pub conflicted: usize,
    pub skipped: usize,
}

impl MergeReport {
    pub fn clean(&self) -> bool {
        self.conflicted == 0
    }
}

#[derive(Debug, Clone)]
struct Change {
    path: String,
    status: &'static str,
}

pub struct WorktreeService {
    root: PathBuf,
    base_dir: PathBuf,
    pool: SqlitePool,
}

impl WorktreeService {
    pub fn new(root: impl Into<PathBuf>, data_dir: &Path, pool: SqlitePool) -> Self {
        WorktreeService {
            root: root.into(),
            base_dir: data_dir.join(WORKTREES_DIR),
            pool,
        }
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Create a detached worktree at HEAD and record it on the agent row.
    pub async fn create(&self, agent_id: &str) -> Result<Worktree, WorktreeError> {
        self.ensure_repository()?;
        let base = self.git(&["rev-parse", "HEAD"])?;
        if base.trim().is_empty() {
            return Err(WorktreeError::Git {
                command: "rev-parse HEAD".into(),
                message: "the repository has no commits yet".into(),
            });
        }

        let repo = self
            .root
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string());
        let short = &agent_id[..agent_id.len().min(8)];
        let path = self.base_dir.join(format!("{repo}-{short}"));
        if path.exists() {
            // A leftover from a kill: `git worktree add` refuses a non-empty directory, and
            // silently reusing a stranger's checkout is worse than saying so.
            return Err(WorktreeError::Git {
                command: "worktree add".into(),
                message: format!("{} already exists; remove it first", path.display()),
            });
        }
        std::fs::create_dir_all(&self.base_dir)?;

        self.git(&[
            "worktree",
            "add",
            "--detach",
            &path.to_string_lossy(),
            base.trim(),
        ])?;

        sqlx::query("UPDATE agents SET worktree = ? WHERE id = ?")
            .bind(path.to_string_lossy().into_owned())
            .bind(agent_id)
            .execute(&self.pool)
            .await?;

        Ok(Worktree {
            path,
            base: base.trim().to_string(),
            agent_id: agent_id.to_string(),
        })
    }

    pub async fn get(&self, agent_id: &str) -> Result<Worktree, WorktreeError> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT worktree FROM agents WHERE id = ?")
                .bind(agent_id)
                .fetch_optional(&self.pool)
                .await?;
        let path = row
            .and_then(|(path,)| path)
            .ok_or_else(|| WorktreeError::NotFound(agent_id.to_string()))?;
        let base = self.git_in(Path::new(&path), &["rev-parse", "HEAD"])?;
        Ok(Worktree {
            path: PathBuf::from(path),
            base: base.trim().to_string(),
            agent_id: agent_id.to_string(),
        })
    }

    /// Every checkout under the data directory, as git reports them.
    pub fn list(&self) -> Result<Vec<PathBuf>, WorktreeError> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.base_dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                out.push(entry.path());
            }
        }
        out.sort();
        Ok(out)
    }

    /// Files the agent touched, relative to its worktree, with git's short status letter.
    fn changes(&self, worktree: &Path) -> Result<Vec<Change>, WorktreeError> {
        let raw = self.git_in(
            worktree,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )?;
        let mut out = Vec::new();
        for entry in raw.split('\0').filter(|entry| !entry.is_empty()) {
            if entry.len() < 4 {
                continue;
            }
            let (code, path) = entry.split_at(3);
            let code = &code[..2];
            let path = path.trim_start_matches(' ');
            let status = if code.contains('D') {
                "deleted"
            } else if code.contains('R') || code.contains('C') {
                "renamed"
            } else if code == "??" {
                "untracked"
            } else if code.contains('M') || code.contains('A') || code.contains('T') {
                "modified"
            } else {
                "unsupported"
            };
            out.push(Change {
                path: path.replace('\\', "/"),
                status,
            });
        }
        Ok(out)
    }

    /// What the agent changed, without writing anything.
    pub async fn preview(&self, agent_id: &str) -> Result<Vec<(String, String)>, WorktreeError> {
        let worktree = self.get(agent_id).await?;
        Ok(self
            .changes(&worktree.path)?
            .into_iter()
            .map(|change| (change.path, change.status.to_string()))
            .collect())
    }

    /// The changes as one unified diff, with the paths it covers (D115).
    ///
    /// Git's status order is stable, so the same worktree always produces the same bytes and
    /// the same hash. Deletions, renames and binary files are left out here exactly as
    /// `merge_back` leaves them out; the caller sees them in the merge report instead of a
    /// diff it cannot act on.
    pub async fn diff(&self, agent_id: &str) -> Result<(String, Vec<String>), WorktreeError> {
        let worktree = self.get(agent_id).await?;
        let mut out = String::new();
        let mut files = Vec::new();
        for change in self.changes(&worktree.path)? {
            if matches!(change.status, "deleted" | "renamed" | "unsupported") {
                continue;
            }
            let Ok(bytes) = std::fs::read(worktree.path.join(&change.path)) else {
                continue;
            };
            let Ok(content) = String::from_utf8(bytes) else {
                continue;
            };
            let base = self
                .git_bytes(&["show", &format!("{}:{}", worktree.base, change.path)])
                .unwrap_or_default();
            let base = String::from_utf8(base).unwrap_or_default();
            let diff = crate::patches::unified_diff(&change.path, &base, &content);
            if diff.is_empty() {
                continue;
            }
            files.push(change.path.clone());
            out.push_str(&diff);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        Ok((out, files))
    }

    /// Move every change back into the main tree through a patch, or explain why not.
    pub async fn merge_back(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> Result<MergeReport, WorktreeError> {
        let worktree = self.get(agent_id).await?;
        let pipeline = PatchPipeline::new(&self.root, self.pool.clone());
        let mut lines = Vec::new();
        let mut applied = 0;
        let mut conflicted = 0;
        let mut skipped = 0;

        for change in self.changes(&worktree.path)? {
            if change.status == "deleted"
                || change.status == "renamed"
                || change.status == "unsupported"
            {
                skipped += 1;
                let detail = match change.status {
                    "deleted" => {
                        "deleting a file is not a patch; remove it after reviewing the worktree"
                    }
                    "renamed" => {
                        "a rename is not a patch; move the file after reviewing the worktree"
                    }
                    _ => "git reported a change this pipeline does not express",
                };
                lines.push(MergeLine {
                    path: change.path,
                    status: if change.status == "deleted" {
                        MergeStatus::Deleted
                    } else {
                        MergeStatus::Unsupported
                    },
                    patch_id: None,
                    detail: Some(detail.to_string()),
                });
                continue;
            }

            let source = worktree.path.join(&change.path);
            let bytes = match std::fs::read(&source) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    skipped += 1;
                    lines.push(MergeLine {
                        path: change.path,
                        status: MergeStatus::Deleted,
                        patch_id: None,
                        detail: Some("the file disappeared after git reported it".into()),
                    });
                    continue;
                }
                Err(error) => return Err(WorktreeError::Io(error)),
            };
            let content = match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => {
                    skipped += 1;
                    lines.push(MergeLine {
                        path: change.path,
                        status: MergeStatus::Unsupported,
                        patch_id: None,
                        detail: Some("binary file; the patch pipeline refuses it".into()),
                    });
                    continue;
                }
            };

            // The base commit's copy is what both sides started from. If the main tree still
            // holds those bytes, the worktree is a clean change; otherwise it is a conflict.
            let base_bytes = self
                .git_bytes(&["show", &format!("{}:{}", worktree.base, change.path)])
                .unwrap_or_default();
            let base_hash = sha256_hex(&base_bytes);

            let main_path = self.root.join(&change.path);
            let main_bytes = match std::fs::read(&main_path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(WorktreeError::Io(error)),
            };
            let main_hash = main_bytes
                .as_deref()
                .map(sha256_hex)
                .unwrap_or_else(sha256_empty);

            if main_hash == sha256_hex(content.as_bytes()) {
                lines.push(MergeLine {
                    path: change.path,
                    status: MergeStatus::Unchanged,
                    patch_id: None,
                    detail: None,
                });
                continue;
            }

            if main_hash != base_hash {
                conflicted += 1;
                lines.push(MergeLine {
                    path: change.path,
                    status: MergeStatus::Conflicted,
                    patch_id: None,
                    detail: Some(format!(
                        "the main tree moved since the worktree was created ({} → {})",
                        short(&base_hash),
                        short(&main_hash)
                    )),
                });
                continue;
            }

            match pipeline
                .propose(session_id, agent_id, &change.path, &main_hash, &content)
                .await
            {
                Ok(patch) => {
                    let patch = pipeline.apply(&patch.id).await.unwrap_or(patch);
                    if patch.status == "applied" {
                        applied += 1;
                    } else {
                        conflicted += 1;
                    }
                    lines.push(MergeLine {
                        path: change.path,
                        status: if patch.status == "applied" {
                            MergeStatus::Applied
                        } else {
                            MergeStatus::Conflicted
                        },
                        patch_id: Some(patch.id),
                        detail: None,
                    });
                }
                Err(error) => {
                    conflicted += 1;
                    lines.push(MergeLine {
                        path: change.path,
                        status: MergeStatus::Conflicted,
                        patch_id: None,
                        detail: Some(error.to_string()),
                    });
                }
            }
        }

        Ok(MergeReport {
            agent_id: agent_id.to_string(),
            base: worktree.base,
            lines,
            applied,
            conflicted,
            skipped,
        })
    }

    /// Remove the checkout and clear the agent's pointer. Refuses a dirty tree unless `force`,
    /// because discarding unmerged work silently is the failure mode this whole module avoids.
    pub async fn remove(&self, agent_id: &str, force: bool) -> Result<(), WorktreeError> {
        let worktree = self.get(agent_id).await?;
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        let path = worktree.path.to_string_lossy().into_owned();
        args.push(&path);
        self.git(&args)?;

        sqlx::query("UPDATE agents SET worktree = NULL WHERE id = ?")
            .bind(agent_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /* ---------------- git plumbing ---------------- */

    /// The project root must be the *top level* of its repository.
    ///
    /// `git -C <dir> rev-parse --is-inside-work-tree` answers `true` for any subdirectory of any
    /// repository above it — including a temporary directory under a home directory that happens
    /// to be versioned. Cutting a worktree of that parent and merging it into this project would
    /// be a surprising and destructive reading of "the project is a git repo".
    fn ensure_repository(&self) -> Result<(), WorktreeError> {
        let top = match self.git(&["rev-parse", "--show-toplevel"]) {
            Ok(top) => top.trim().to_string(),
            Err(_) => return Err(WorktreeError::NotARepository(self.root.clone())),
        };
        let same = match (
            std::fs::canonicalize(&top),
            std::fs::canonicalize(&self.root),
        ) {
            (Ok(a), Ok(b)) => a == b,
            _ => Path::new(&top) == self.root,
        };
        if same {
            Ok(())
        } else {
            Err(WorktreeError::NotARepository(self.root.clone()))
        }
    }

    fn git(&self, args: &[&str]) -> Result<String, WorktreeError> {
        self.git_in(&self.root, args)
    }

    fn git_in(&self, cwd: &Path, args: &[&str]) -> Result<String, WorktreeError> {
        let mut command = Command::new("git");
        command.arg("-C").arg(cwd).args(args);
        crate::process::hide(&mut command);
        let output = command.output().map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => WorktreeError::GitMissing,
            _ => WorktreeError::Io(error),
        })?;
        if !output.status.success() {
            return Err(WorktreeError::Git {
                command: args.join(" "),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn git_bytes(&self, args: &[&str]) -> Result<Vec<u8>, WorktreeError> {
        let mut command = Command::new("git");
        command.arg("-C").arg(&self.root).args(args);
        crate::process::hide(&mut command);
        let output = command.output().map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => WorktreeError::GitMissing,
            _ => WorktreeError::Io(error),
        })?;
        if !output.status.success() {
            return Err(WorktreeError::Git {
                command: args.join(" "),
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(output.stdout)
    }

    /// The base commit and each changed path, for a caller that wants the whole picture.
    pub async fn summary(&self, agent_id: &str) -> Result<BTreeMap<String, String>, WorktreeError> {
        let worktree = self.get(agent_id).await?;
        Ok(self
            .changes(&worktree.path)?
            .into_iter()
            .map(|change| (change.path, change.status.to_string()))
            .collect())
    }
}

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

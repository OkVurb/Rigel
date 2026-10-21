//! The only path by which Rigel writes project files (§8.4, D8, D19).
//!
//! Order is the guarantee:
//!
//! 1. read the file and compare its hash with `base_hash` — compare-and-swap (I4);
//! 2. insert the `patches` row (`proposed`) with the diff, base and result hashes;
//! 3. write the journal file and fsync it (I5, write-ahead);
//! 4. write a temp file beside the target, fsync it, rename it over the target;
//! 5. mark the row `applied`, delete the journal.
//!
//! A process killed between 3 and 4 leaves a journal and an untouched file; `recover`
//! reports the patch as pending and applies nothing (Q57: never auto-apply). A kill between
//! 4 and 5 leaves the target already at `result_hash`; `recover` marks the row applied
//! *without writing again*, which is what makes resume idempotent.
//!
//! `StopAt` exists so the crash tests and the QA script can stop at exactly those two
//! points. It is a deliberate seam, not a hidden mode: the code after it is the real code.

use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::migrate::now_ms;

/// Where `propose` should stop, for crash simulation. `Never` is production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopAt {
    Never,
    /// Row and journal are durable; the target file is untouched.
    AfterJournal,
    /// The target has been replaced; the row still says `proposed`.
    AfterRename,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patch {
    pub id: String,
    pub session_id: String,
    pub agent_id: String,
    pub path: String,
    pub base_hash: String,
    pub result_hash: String,
    pub diff: String,
    pub status: String,
    pub created_at: i64,
    pub applied_at: Option<i64>,
    pub reverted_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryState {
    /// The rename had happened; the row is now `applied`.
    Applied,
    /// The rename had not happened; the patch is still `proposed` and the tree unchanged.
    Pending,
    /// The file no longer matches either hash; a human resolves it.
    Conflicted,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recovery {
    pub patch_id: String,
    pub path: String,
    pub state: RecoveryState,
}

#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    #[error("{0} is outside the project root")]
    OutsideRoot(String),

    #[error("{0} is not a project-relative path")]
    InvalidPath(String),

    #[error("{0} does not exist")]
    NotFound(String),

    #[error("{0} is not valid UTF-8; Rigel refuses to patch binary files")]
    Binary(String),

    #[error("the file changed since it was read: expected {expected}, found {current}")]
    StaleBase { expected: String, current: String },

    #[error("the patch makes no change")]
    NoChanges,

    #[error("patch {0} does not exist")]
    PatchNotFound(String),

    #[error("patch {id} cannot be {action} from status {status}")]
    WrongStatus {
        id: String,
        status: String,
        action: &'static str,
    },

    #[error("the working tree no longer matches this patch; resolve it by hand")]
    Conflicted,

    #[error("the stored diff could not be reversed")]
    BadDiff,

    #[error("stopped at {0:?} for a crash test")]
    Stopped(StopAt),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// The durable journal record. Plain JSON on disk, in `.rigel/journals/<patch>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Journal {
    patch_id: String,
    session_id: String,
    agent_id: String,
    path: String,
    base_hash: String,
    result_hash: String,
    tmp: String,
    created_at: i64,
}

pub struct PatchPipeline {
    root: PathBuf,
    pool: SqlitePool,
    journal_dir: PathBuf,
}

impl PatchPipeline {
    pub fn new(root: &Path, pool: SqlitePool) -> Self {
        let rigel = root.join(".rigel");
        Self {
            root: root.to_path_buf(),
            pool,
            journal_dir: rigel.join("journals"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn journal_dir(&self) -> &Path {
        &self.journal_dir
    }

    /// Resolve `rel` under the root without trusting it. Absolute paths, `..`, and paths
    /// whose canonical parent escapes the root are all rejected. Symlinks are resolved
    /// before the check, so a link cannot point a patch outside the project.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, PatchError> {
        let rel = rel.replace('\\', "/");
        let candidate = Path::new(&rel);
        if candidate.is_absolute()
            || rel.starts_with('/')
            || rel.starts_with("//")
            || rel.contains('\0')
        {
            return Err(PatchError::InvalidPath(rel));
        }
        for component in candidate.components() {
            match component {
                Component::Normal(_) | Component::CurDir => {}
                _ => return Err(PatchError::InvalidPath(rel)),
            }
        }

        let joined = self.root.join(candidate);
        let parent = joined.parent().unwrap_or(&self.root);
        let canonical_root = self.root.canonicalize().map_err(PatchError::Io)?;
        let canonical_parent = if parent.exists() {
            parent.canonicalize().map_err(PatchError::Io)?
        } else {
            return Err(PatchError::NotFound(rel));
        };
        if !canonical_parent.starts_with(&canonical_root) {
            return Err(PatchError::OutsideRoot(rel));
        }

        if joined.exists() {
            let canonical = joined.canonicalize().map_err(PatchError::Io)?;
            if !canonical.starts_with(&canonical_root) {
                return Err(PatchError::OutsideRoot(rel));
            }
        }
        Ok(joined)
    }

    pub fn read(&self, rel: &str) -> Result<(String, String), PatchError> {
        let path = self.resolve(rel)?;
        if !path.exists() {
            return Err(PatchError::NotFound(rel.to_string()));
        }
        let bytes = fs::read(&path)?;
        let hash = sha256_hex(&bytes);
        let text = String::from_utf8(bytes).map_err(|_| PatchError::Binary(rel.to_string()))?;
        Ok((text, hash))
    }

    pub async fn propose(
        &self,
        session_id: &str,
        agent_id: &str,
        rel: &str,
        base_hash: &str,
        new_content: &str,
    ) -> Result<Patch, PatchError> {
        self.propose_with(
            session_id,
            agent_id,
            rel,
            base_hash,
            new_content,
            StopAt::Never,
        )
        .await
    }

    pub async fn propose_with(
        &self,
        session_id: &str,
        agent_id: &str,
        rel: &str,
        base_hash: &str,
        new_content: &str,
        stop: StopAt,
    ) -> Result<Patch, PatchError> {
        let path = self.resolve(rel)?;
        let current_bytes = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(PatchError::Io(error)),
        };
        let current_text = match &current_bytes {
            Some(bytes) => Some(
                String::from_utf8(bytes.clone())
                    .map_err(|_| PatchError::Binary(rel.to_string()))?,
            ),
            None => None,
        };
        let current_hash = sha256_hex(current_bytes.as_deref().unwrap_or(b""));

        if current_hash != base_hash {
            return Err(PatchError::StaleBase {
                expected: base_hash.to_string(),
                current: current_hash,
            });
        }

        let diff = unified_diff(rel, current_text.as_deref().unwrap_or(""), new_content);
        if diff.is_empty() {
            return Err(PatchError::NoChanges);
        }

        let id = uuid::Uuid::new_v4().to_string();
        let result_hash = sha256_hex(new_content.as_bytes());
        let created_at = now_ms();

        sqlx::query(
            "INSERT INTO patches
               (id, session_id, agent_id, path, base_hash, result_hash, diff, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'proposed', ?)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(agent_id)
        .bind(rel)
        .bind(base_hash)
        .bind(&result_hash)
        .bind(&diff)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        let tmp = path
            .parent()
            .unwrap_or(&self.root)
            .join(format!(".{}.rigel-{id}.tmp", file_name(&path)));
        let journal = Journal {
            patch_id: id.clone(),
            session_id: session_id.to_string(),
            agent_id: agent_id.to_string(),
            path: rel.to_string(),
            base_hash: base_hash.to_string(),
            result_hash: result_hash.clone(),
            tmp: tmp.to_string_lossy().into_owned(),
            created_at,
        };
        self.write_journal(&journal)?;

        if stop == StopAt::AfterJournal {
            return Err(PatchError::Stopped(stop));
        }

        self.write_and_rename(&tmp, &path, new_content)?;

        if stop == StopAt::AfterRename {
            return Err(PatchError::Stopped(stop));
        }

        self.mark_applied(&id).await?;
        self.remove_journal(&id)?;

        self.get(&id)
            .await?
            .ok_or_else(|| PatchError::PatchNotFound(id))
    }

    /// Apply a patch that is still `proposed` (normally after recovery surfaced it).
    ///
    /// Idempotent: a patch already at `result_hash` is marked applied without a write.
    pub async fn apply(&self, id: &str) -> Result<Patch, PatchError> {
        let patch = self
            .get(id)
            .await?
            .ok_or_else(|| PatchError::PatchNotFound(id.to_string()))?;

        match patch.status.as_str() {
            "applied" => return Ok(patch),
            "proposed" | "conflicted" => {}
            other => {
                return Err(PatchError::WrongStatus {
                    id: id.to_string(),
                    status: other.to_string(),
                    action: "applied",
                });
            }
        }

        let target = self.resolve(&patch.path)?;
        let current = match fs::read(&target) {
            Ok(bytes) => sha256_hex(&bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => sha256_hex(b""),
            Err(error) => return Err(PatchError::Io(error)),
        };

        if current == patch.result_hash {
            self.mark_applied(id).await?;
            self.remove_journal(id)?;
            return self
                .get(id)
                .await?
                .ok_or_else(|| PatchError::PatchNotFound(id.to_string()));
        }
        if current != patch.base_hash {
            sqlx::query("UPDATE patches SET status = 'conflicted' WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
            return Err(PatchError::Conflicted);
        }

        // Rebuild the content from the diff so the journal and row stay the single record.
        let text = if target.exists() {
            self.read(&patch.path)?.0
        } else {
            String::new()
        };
        let new_content = reverse_apply(&patch.diff, &text)?;
        let tmp = target
            .parent()
            .unwrap_or(&self.root)
            .join(format!(".{}.rigel-{id}.apply.tmp", file_name(&target)));
        self.write_and_rename(&tmp, &target, &new_content)?;
        self.mark_applied(id).await?;
        self.remove_journal(id)?;

        self.get(id)
            .await?
            .ok_or_else(|| PatchError::PatchNotFound(id.to_string()))
    }

    /// Reverse the diff, but only over a file that is still exactly the result (I4's mirror).
    pub async fn revert(&self, id: &str) -> Result<Patch, PatchError> {
        let patch = self
            .get(id)
            .await?
            .ok_or_else(|| PatchError::PatchNotFound(id.to_string()))?;
        if patch.status != "applied" {
            return Err(PatchError::WrongStatus {
                id: id.to_string(),
                status: patch.status,
                action: "reverted",
            });
        }

        let target = self.resolve(&patch.path)?;
        let bytes = fs::read(&target)?;
        let current_hash = sha256_hex(&bytes);
        if current_hash != patch.result_hash {
            sqlx::query("UPDATE patches SET status = 'conflicted' WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
            return Err(PatchError::Conflicted);
        }

        let current =
            String::from_utf8(bytes).map_err(|_| PatchError::Binary(patch.path.clone()))?;
        let original = apply_reverse(&patch.diff, &current)?;
        let tmp = target
            .parent()
            .unwrap_or(&self.root)
            .join(format!(".{}.rigel-{id}.revert.tmp", file_name(&target)));
        self.write_and_rename(&tmp, &target, &original)?;

        let reverted_at = now_ms();
        sqlx::query(
            "UPDATE patches SET status = 'reverted', reverted_at = ? WHERE id = ? AND status = 'applied'",
        )
        .bind(reverted_at)
        .bind(id)
        .execute(&self.pool)
        .await?;

        self.get(id)
            .await?
            .ok_or_else(|| PatchError::PatchNotFound(id.to_string()))
    }

    /// Walk every journal for a session and reconcile it with the tree. Never writes a
    /// project file: it only decides which of the two crash windows the process died in.
    pub async fn recover(&self, session_id: &str) -> Result<Vec<Recovery>, PatchError> {
        if !self.journal_dir.exists() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for entry in fs::read_dir(&self.journal_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            let Ok(journal) = serde_json::from_str::<Journal>(&text) else {
                // A torn journal is not trustworthy; leave it for `rigel doctor`.
                continue;
            };
            if journal.session_id != session_id {
                continue;
            }
            let target = match self.resolve(&journal.path) {
                Ok(path) => path,
                Err(_) => {
                    self.mark_conflicted(&journal.patch_id).await?;
                    out.push(Recovery {
                        patch_id: journal.patch_id.clone(),
                        path: journal.path.clone(),
                        state: RecoveryState::Conflicted,
                    });
                    continue;
                }
            };

            let current = match fs::read(&target) {
                Ok(bytes) => sha256_hex(&bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => sha256_hex(b""),
                Err(error) => return Err(PatchError::Io(error)),
            };

            if current == journal.result_hash {
                self.mark_applied(&journal.patch_id).await?;
                self.remove_journal(&journal.patch_id)?;
                out.push(Recovery {
                    patch_id: journal.patch_id.clone(),
                    path: journal.path.clone(),
                    state: RecoveryState::Applied,
                });
            } else if current == journal.base_hash {
                // The rename never happened. Remove the orphan temp file, keep the journal
                // and the row, and let the user decide (§8.5: never auto-apply).
                let _ = fs::remove_file(Path::new(&journal.tmp));
                out.push(Recovery {
                    patch_id: journal.patch_id.clone(),
                    path: journal.path.clone(),
                    state: RecoveryState::Pending,
                });
            } else {
                self.mark_conflicted(&journal.patch_id).await?;
                self.remove_journal(&journal.patch_id)?;
                out.push(Recovery {
                    patch_id: journal.patch_id.clone(),
                    path: journal.path.clone(),
                    state: RecoveryState::Conflicted,
                });
            }
        }

        // Rows whose journal was already removed by step 5 are invisible here; a second
        // call to recover therefore returns the same answer and changes nothing.
        Ok(out)
    }

    pub async fn list(&self, session_id: &str) -> Result<Vec<Patch>, PatchError> {
        let rows: Vec<PatchRow> = sqlx::query_as(
            "SELECT id, session_id, agent_id, path, base_hash, result_hash, diff, status,
                    created_at, applied_at, reverted_at
             FROM patches WHERE session_id = ? ORDER BY created_at, id",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Patch::from).collect())
    }

    pub async fn get(&self, id: &str) -> Result<Option<Patch>, PatchError> {
        let row: Option<PatchRow> = sqlx::query_as(
            "SELECT id, session_id, agent_id, path, base_hash, result_hash, diff, status,
                    created_at, applied_at, reverted_at
             FROM patches WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Patch::from))
    }

    fn write_journal(&self, journal: &Journal) -> Result<(), PatchError> {
        fs::create_dir_all(&self.journal_dir)?;
        let path = self.journal_dir.join(format!("{}.json", journal.patch_id));
        let tmp = self.journal_dir.join(format!(".{}.tmp", journal.patch_id));
        {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(&serde_json::to_vec_pretty(journal)?)?;
            file.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        sync_dir(&self.journal_dir);
        Ok(())
    }

    fn remove_journal(&self, id: &str) -> Result<(), PatchError> {
        match fs::remove_file(self.journal_dir.join(format!("{id}.json"))) {
            Ok(()) => {
                sync_dir(&self.journal_dir);
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(PatchError::Io(error)),
        }
    }

    fn write_and_rename(&self, tmp: &Path, target: &Path, content: &str) -> Result<(), PatchError> {
        {
            let mut file = fs::File::create(tmp)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(tmp, target)?;
        if let Some(dir) = target.parent() {
            sync_dir(dir);
        }
        Ok(())
    }

    async fn mark_applied(&self, id: &str) -> Result<(), PatchError> {
        sqlx::query(
            "UPDATE patches SET status = 'applied', applied_at = ? WHERE id = ? AND status != 'applied'",
        )
        .bind(now_ms())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_conflicted(&self, id: &str) -> Result<(), PatchError> {
        sqlx::query("UPDATE patches SET status = 'conflicted' WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

type PatchRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    Option<i64>,
    Option<i64>,
);

impl From<PatchRow> for Patch {
    fn from(row: PatchRow) -> Self {
        Self {
            id: row.0,
            session_id: row.1,
            agent_id: row.2,
            path: row.3,
            base_hash: row.4,
            result_hash: row.5,
            diff: row.6,
            status: row.7,
            created_at: row.8,
            applied_at: row.9,
            reverted_at: row.10,
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn sha256_empty() -> String {
    sha256_hex(b"")
}

/// Best-effort directory fsync. Windows cannot open a directory as a file without extra
/// flags; NTFS's journal makes the rename durable anyway, and the file itself was fsynced.
fn sync_dir(path: &Path) {
    #[cfg(unix)]
    {
        if let Ok(file) = fs::File::open(path) {
            let _ = file.sync_all();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/* ------------------------------- unified diff ------------------------------- */

const CONTEXT: usize = 3;

/// `split_inclusive` yields one empty item for an empty string, which would make an empty
/// file look like a one-line file. It is not.
fn lines_keep(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split_inclusive('\n').collect()
    }
}

/// One hunk around the change, with up to three lines of context. Rigel does not try to
/// produce a pretty multi-hunk diff: the diff is evidence for a human and the exact inverse
/// for `revert`, and one hunk serves both.
pub fn unified_diff(path: &str, old: &str, new: &str) -> String {
    if old == new {
        return String::new();
    }
    let old_lines = lines_keep(old);
    let new_lines = lines_keep(new);

    let mut prefix = 0;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old_lines.len() - prefix
        && suffix < new_lines.len() - prefix
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let start = prefix.saturating_sub(CONTEXT);
    let old_end = old_lines.len() - suffix + suffix.min(CONTEXT);
    let new_end = new_lines.len() - suffix + suffix.min(CONTEXT);
    let old_change_end = old_lines.len() - suffix;
    let new_change_end = new_lines.len() - suffix;

    let mut out = String::new();
    out.push_str(&format!("--- a/{path}\n+++ b/{path}\n"));
    let old_len = old_end - start;
    let new_len = new_end - start;
    out.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        if old_len == 0 { start } else { start + 1 },
        old_len,
        if new_len == 0 { start } else { start + 1 },
        new_len
    ));
    for line in &old_lines[start..prefix] {
        push_line(&mut out, ' ', line);
    }
    for line in &old_lines[prefix..old_change_end] {
        push_line(&mut out, '-', line);
    }
    for line in &new_lines[prefix..new_change_end] {
        push_line(&mut out, '+', line);
    }
    for line in &old_lines[old_change_end..old_end] {
        push_line(&mut out, ' ', line);
    }
    out
}

fn push_line(out: &mut String, marker: char, line: &str) {
    out.push(marker);
    out.push_str(line);
    if !line.ends_with('\n') {
        out.push_str("\n\\ No newline at end of file\n");
    }
}

/// Parse the single hunk, preserving line endings so a body can be compared with the file.
fn parse_hunks(diff: &str) -> Result<Vec<(char, String)>, PatchError> {
    let raw: Vec<&str> = diff.split_inclusive('\n').collect();
    if raw.len() < 4 || !raw[2].starts_with("@@ ") {
        return Err(PatchError::BadDiff);
    }

    let mut hunks = Vec::new();
    for (index, line) in raw.iter().enumerate().skip(3) {
        if line.starts_with('\\') {
            continue;
        }
        let mut chars = line.chars();
        let marker = chars.next().ok_or(PatchError::BadDiff)?;
        if !matches!(marker, ' ' | '-' | '+') {
            return Err(PatchError::BadDiff);
        }
        let mut body: String = chars.collect();
        // A "No newline" marker means the newline after this body was synthesised by
        // `push_line`, not part of the file.
        if index + 1 < raw.len() && raw[index + 1].starts_with('\\') && body.ends_with('\n') {
            body.pop();
        }
        hunks.push((marker, body));
    }
    Ok(hunks)
}

/// Apply the hunk to `current`, removing lines marked `removed` and inserting the other side.
/// Forward application removes `-`; reversing the patch removes `+`.
fn apply_hunks(diff: &str, current: &str, removed: char) -> Result<String, PatchError> {
    let hunks = parse_hunks(diff)?;
    let appended = match removed {
        '-' => '+',
        '+' => '-',
        _ => return Err(PatchError::BadDiff),
    };
    let text_lines = lines_keep(current);

    // Locate the hunk by matching its unchanged and removed lines in order.
    let mut start = None;
    for candidate in 0..=text_lines.len() {
        let mut index = candidate;
        let mut ok = true;
        for (marker, body) in &hunks {
            if *marker == appended {
                continue;
            }
            if text_lines.get(index).copied() != Some(body.as_str()) {
                ok = false;
                break;
            }
            index += 1;
        }
        if ok {
            start = Some(candidate);
            break;
        }
    }
    let mut index = start.ok_or(PatchError::BadDiff)?;

    let mut out: Vec<String> = text_lines[..index].iter().map(|s| s.to_string()).collect();
    for (marker, body) in &hunks {
        if *marker == ' ' {
            if text_lines.get(index).copied() != Some(body.as_str()) {
                return Err(PatchError::BadDiff);
            }
            out.push(body.clone());
            index += 1;
        } else if *marker == removed {
            if text_lines.get(index).copied() != Some(body.as_str()) {
                return Err(PatchError::BadDiff);
            }
            index += 1;
        } else if *marker == appended {
            out.push(body.clone());
        } else {
            return Err(PatchError::BadDiff);
        }
    }
    out.extend(text_lines[index..].iter().map(|s| s.to_string()));
    Ok(out.concat())
}

fn apply_reverse(diff: &str, current: &str) -> Result<String, PatchError> {
    apply_hunks(diff, current, '+')
}

fn reverse_apply(diff: &str, current: &str) -> Result<String, PatchError> {
    apply_hunks(diff, current, '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_reverses_exactly() {
        let old = "one\ntwo\nthree\nfour\nfive\n";
        let new = "one\ntwo\nTHREE\nfour\nfive\n";
        let diff = unified_diff("a.txt", old, new);
        assert!(diff.contains("-three"));
        assert!(diff.contains("+THREE"));
        assert_eq!(reverse_apply(&diff, old).unwrap(), new);
        assert_eq!(apply_reverse(&diff, new).unwrap(), old);
    }

    #[test]
    fn a_missing_trailing_newline_survives_the_round_trip() {
        let old = "a\nb";
        let new = "a\nB";
        let diff = unified_diff("x", old, new);
        assert_eq!(reverse_apply(&diff, old).unwrap(), new);
        assert_eq!(apply_reverse(&diff, new).unwrap(), old);
    }

    #[test]
    fn an_insertion_into_an_empty_file_round_trips() {
        let diff = unified_diff("x", "", "hello\n");
        assert_eq!(reverse_apply(&diff, "").unwrap(), "hello\n");
        assert_eq!(apply_reverse(&diff, "hello\n").unwrap(), "");
    }

    #[tokio::test]
    async fn paths_that_escape_the_root_are_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let pool = sqlx::SqlitePool::connect_lazy("sqlite::memory:").unwrap();
        let pipeline = PatchPipeline::new(tmp.path(), pool);
        for bad in ["../x", "a/../../x", "C:/Windows/x", "/etc/passwd"] {
            assert!(pipeline.resolve(bad).is_err(), "{bad} should be rejected");
        }
    }
}

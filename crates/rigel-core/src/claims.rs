//! The claim ledger (P5.1, D5): advisory intent, never a lock.
//!
//! A claim says "I am about to edit this file". It does **not** stop anyone, and the word
//! "locked" never appears in a claim's output: the UI, the CLI and the MCP tools all say
//! "claimed by X, unverified". A violation is recorded, not prevented.
//!
//! Rows are never deleted (I3). Releasing sets `released_at` and a reason; expiry is a release
//! with reason `expired`, so the history stays readable. The schema's partial unique index on
//! `path WHERE released_at IS NULL` is what makes "one active claim per path" true, which is why
//! a second claimant is told who holds it instead of being given a second row.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::migrate::now_ms;
use crate::patches::{
    apply_patch_text, reverse_patch, sha256_hex, Patch, PatchError, PatchPipeline, StopAt,
};

/// §8.1: a claim path is project-relative and at most 200 characters.
pub const MAX_PATH_CHARS: usize = 200;
/// The default lifetime of one claim before a stale scan may expire it.
pub const CLAIM_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// A heartbeat older than this makes a claim stale; stale is reported, never auto-released.
pub const STALE_AFTER: Duration = Duration::from_secs(90);
/// A claim keeps the text it started from for the conflict view, up to this size (D79).
pub const MAX_CLAIM_TEXT_BYTES: usize = 1024 * 1024;
/// How many of one agent's patches the conflict view will walk forward (D79). A longer chain is
/// still a chain; it is just not worth an unbounded query to render one conflict.
const MAX_PATCH_CHAIN: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum ClaimError {
    #[error("{path:?} is not a project-relative path")]
    InvalidPath { path: String },

    #[error("{path:?} is {chars} characters; the limit is {MAX_PATH_CHARS}")]
    PathTooLong { path: String, chars: usize },

    #[error("agent {0} is not registered in this project")]
    UnknownAgent(String),

    #[error("no conflict {0} in this project")]
    UnknownConflict(String),

    #[error("conflict {0} is already resolved")]
    ConflictClosed(String),

    #[error("this conflict cannot be merged: {0}")]
    SidesUnavailable(String),

    #[error("patch error: {0}")]
    Patch(#[from] PatchError),

    #[error("merge error: {0}")]
    Merge(#[from] crate::merge::MergeError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// One row of the ledger, joined with who holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub path: String,
    pub agent_id: String,
    /// The agent's role when it has one, else its harness.
    pub agent: String,
    pub harness: String,
    pub base_hash: Option<String>,
    pub acquired_at: i64,
    pub heartbeat_at: i64,
    pub released_at: Option<i64>,
    pub release_reason: Option<String>,
    /// No heartbeat for [`STALE_AFTER`]. Reported, never released automatically.
    pub stale: bool,
    /// `released_at.is_none() && !stale`.
    pub held: bool,
}

/// What `acquire` did for one path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimResult {
    pub path: String,
    /// True when this agent holds the live claim after the call.
    pub held: bool,
    /// Present when someone else holds it: the advisory answer, not a refusal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_by: Option<Claim>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Claim {
    fn stale_at(&self, now: i64) -> bool {
        self.released_at.is_none()
            && now.saturating_sub(self.heartbeat_at) > STALE_AFTER.as_millis() as i64
    }
}

/// A conflict row: a claimed file changed, and the writer is not the claimant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimConflict {
    pub id: String,
    pub path: String,
    pub claimant: Option<String>,
    pub writer: Option<String>,
    pub base_hash: Option<String>,
    pub observed_hash: Option<String>,
    pub detected_at: i64,
    pub resolved_at: Option<i64>,
    pub resolution: Option<String>,
}

/// Who a merged file is attributed to in the patch ledger (D79). It is not an agent: the human
/// resolved the conflict, and the Diff view shows `you` in the hunk's gutter.
pub const MERGE_AUTHOR: &str = "user";

/// One version of a conflicted file: its text and the hash those bytes must have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictText {
    pub hash: String,
    pub text: String,
}

/// The three rendered versions of a conflict (D79).
///
/// `base` is what the claimant started from, `ours` is the claimant's version, `theirs` is what
/// the file holds. Only `theirs` is read from disk; the other two are rebuilt from the diffs Rigel
/// wrote and kept only when the rebuilt bytes hash to the recorded value. A side that cannot be
/// rebuilt is `None`, and `note` says which and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictSides {
    pub id: String,
    pub path: String,
    pub base: Option<ConflictText>,
    pub ours: Option<ConflictText>,
    pub theirs: Option<ConflictText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The patch that carried a merged file, once a resolution wrote one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_patch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_hash: Option<String>,
}

/// The text [`ClaimService::merge`] would write, before anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergedConflict {
    pub id: String,
    pub path: String,
    /// `<<<<<<< ours` … `>>>>>>> theirs` where the two sides disagree.
    pub merged: String,
    /// How many blocks need a human.
    pub conflicts: usize,
    pub clean: bool,
}

/// What a merged resolution did. `patch` is `None` when the file already held the text.
#[derive(Debug, Clone, Serialize)]
pub struct MergeResolution {
    pub path: String,
    pub patch: Option<Patch>,
    pub unchanged: bool,
}

/// The captured sides, before they are stored.
#[derive(Default)]
struct ConflictCapture {
    base: Option<ConflictText>,
    ours: Option<ConflictText>,
    theirs: Option<ConflictText>,
    note: Option<String>,
}

#[derive(Clone)]
pub struct ClaimService {
    pool: SqlitePool,
    root: PathBuf,
}

impl ClaimService {
    pub fn new(pool: SqlitePool, root: impl Into<PathBuf>) -> Self {
        Self {
            pool,
            root: root.into(),
        }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Claim each path. Never blocks and never steals: a live claim held by another agent is
    /// returned as `held_by`, and this agent stays `held: false` for that path.
    pub async fn acquire(
        &self,
        agent_id: &str,
        paths: &[String],
        _reason: &str,
    ) -> Result<Vec<ClaimResult>, ClaimError> {
        let now = now_ms();
        self.ensure_agent(agent_id).await?;
        let mut results = Vec::new();
        for raw in paths {
            let path = match self.normalize(raw) {
                Ok(path) => path,
                Err(error) => {
                    results.push(ClaimResult {
                        path: raw.clone(),
                        held: false,
                        held_by: None,
                        error: Some(error.to_string()),
                    });
                    continue;
                }
            };
            if let Some(existing) = self.active(&path).await? {
                if existing.agent_id == agent_id {
                    sqlx::query("UPDATE claims SET heartbeat_at = ? WHERE id = ?")
                        .bind(now)
                        .bind(&existing.id)
                        .execute(&self.pool)
                        .await?;
                    results.push(ClaimResult {
                        path,
                        held: true,
                        held_by: Some(Claim {
                            heartbeat_at: now,
                            ..existing
                        }),
                        error: None,
                    });
                } else {
                    // Advisory: say who holds it, record the collision, and let the caller
                    // decide. The partial unique index means there can only be one live row.
                    self.record_conflict(
                        &path,
                        Some(&existing.agent_id),
                        Some(agent_id),
                        existing.base_hash.clone(),
                        None,
                    )
                    .await?;
                    results.push(ClaimResult {
                        path,
                        held: false,
                        held_by: Some(existing),
                        error: None,
                    });
                }
                continue;
            }

            let base_hash = read_hash(&self.root, &path);
            let base_text = read_text(&self.root, &path);
            let id = new_claim_id(&path, agent_id, now);
            sqlx::query(
                "INSERT INTO claims (id, path, agent_id, base_hash, acquired_at, heartbeat_at,
                                     base_text)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(&path)
            .bind(agent_id)
            .bind(&base_hash)
            .bind(now)
            .bind(now)
            .bind(&base_text)
            .execute(&self.pool)
            .await?;
            results.push(ClaimResult {
                path: path.clone(),
                held: true,
                held_by: self.active(&path).await?.or(Some(Claim {
                    id,
                    path,
                    agent_id: agent_id.to_string(),
                    agent: String::new(),
                    harness: String::new(),
                    base_hash,
                    acquired_at: now,
                    heartbeat_at: now,
                    released_at: None,
                    release_reason: None,
                    stale: false,
                    held: true,
                })),
                error: None,
            });
        }
        Ok(results)
    }

    /// Refresh every active claim this agent holds. Returns how many rows moved.
    pub async fn heartbeat(&self, agent_id: &str) -> Result<u64, ClaimError> {
        let now = now_ms();
        let moved = sqlx::query(
            "UPDATE claims SET heartbeat_at = ? WHERE agent_id = ? AND released_at IS NULL",
        )
        .bind(now)
        .bind(agent_id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(moved)
    }

    /// Release this agent's claims on `paths` (or all of them when the list is empty). A claim is
    /// never deleted: the row stays with a timestamp and a reason (I3).
    pub async fn release(
        &self,
        agent_id: &str,
        paths: &[String],
        reason: &str,
    ) -> Result<Vec<String>, ClaimError> {
        let now = now_ms();
        let reason = if reason.trim().is_empty() {
            "released"
        } else {
            reason.trim()
        };
        let mut released = Vec::new();
        if paths.is_empty() {
            let rows = sqlx::query(
                "SELECT id, path FROM claims WHERE agent_id = ? AND released_at IS NULL",
            )
            .bind(agent_id)
            .fetch_all(&self.pool)
            .await?;
            for row in rows {
                sqlx::query(
                    "UPDATE claims SET released_at = ?, release_reason = ?
                     WHERE id = ? AND released_at IS NULL",
                )
                .bind(now)
                .bind(reason)
                .bind(row.get::<String, _>("id"))
                .execute(&self.pool)
                .await?;
                released.push(row.get::<String, _>("path"));
            }
            return Ok(released);
        }
        for raw in paths {
            let path = self.normalize(raw)?;
            let updated = sqlx::query(
                "UPDATE claims SET released_at = ?, release_reason = ?
                 WHERE path = ? AND agent_id = ? AND released_at IS NULL",
            )
            .bind(now)
            .bind(reason)
            .bind(&path)
            .bind(agent_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
            if updated > 0 {
                released.push(path);
            }
        }
        Ok(released)
    }

    /// Auto-release claims that outlived the 24 h TTL. Expiry is a release with reason
    /// `expired`, which is what keeps the "never DELETE" invariant while still clearing the path.
    pub async fn expire(&self, now: i64) -> Result<Vec<String>, ClaimError> {
        let cutoff = now.saturating_sub(CLAIM_TTL.as_millis() as i64);
        let rows = sqlx::query(
            "SELECT id, path FROM claims WHERE released_at IS NULL AND acquired_at < ?",
        )
        .bind(cutoff)
        .fetch_all(&self.pool)
        .await?;
        let mut expired = Vec::new();
        for row in rows {
            sqlx::query(
                "UPDATE claims SET released_at = ?, release_reason = 'expired'
                 WHERE id = ? AND released_at IS NULL",
            )
            .bind(now)
            .bind(row.get::<String, _>("id"))
            .execute(&self.pool)
            .await?;
            expired.push(row.get::<String, _>("path"));
        }
        Ok(expired)
    }

    /// Every held claim, optionally filtered by exact path or prefix. `held` includes stale
    /// claims, because a stale claim is still a statement someone made; `stale` says so.
    pub async fn held(&self, filter: Option<&str>) -> Result<Vec<Claim>, ClaimError> {
        let rows = self.claims(None, filter, true).await?;
        let now = now_ms();
        Ok(rows
            .into_iter()
            .map(|claim| Claim {
                stale: claim.stale_at(now),
                held: true,
                ..claim
            })
            .collect())
    }

    /// The whole ledger, newest first, including released rows (the audit trail).
    pub async fn history(&self, filter: Option<&str>) -> Result<Vec<Claim>, ClaimError> {
        let rows = self.claims(None, filter, false).await?;
        let now = now_ms();
        Ok(rows
            .into_iter()
            .map(|claim| {
                let stale = claim.stale_at(now);
                Claim {
                    held: claim.released_at.is_none() && !stale,
                    stale,
                    ..claim
                }
            })
            .collect())
    }

    /// Files a live claim covers whose current bytes no longer match the claimed base hash.
    /// `agent` is the claimant, because that is who said they would edit it; when a patch in the
    /// same project also touched the path the writer is named instead.
    pub async fn unverified(&self) -> Result<Vec<UnverifiedChange>, ClaimError> {
        let claims = self.held(None).await?;
        let mut changes = Vec::new();
        for claim in claims {
            let Some(base) = claim.base_hash.as_deref() else {
                // A claim on a file that did not exist yet cannot be compared; say nothing.
                continue;
            };
            let current = read_hash(&self.root, &claim.path);
            if current.as_deref() != Some(base) {
                let writer: Option<String> = sqlx::query_scalar(
                    "SELECT agent_id FROM patches WHERE path = ? AND status = 'applied'
                     ORDER BY applied_at DESC LIMIT 1",
                )
                .bind(&claim.path)
                .fetch_optional(&self.pool)
                .await?
                .flatten();
                changes.push(UnverifiedChange {
                    path: claim.path.clone(),
                    agent: writer.clone().or_else(|| Some(claim.agent_id.clone())),
                    claimant: Some(claim.agent_id.clone()),
                    base_hash: Some(base.to_string()),
                    observed_hash: current,
                    ambiguous: writer.is_some_and(|writer| writer != claim.agent_id),
                });
            }
        }
        Ok(changes)
    }

    /// Record a violation: `path` changed under a claim, and `writer` is not the claimant.
    pub async fn record_conflict(
        &self,
        path: &str,
        claimant: Option<&str>,
        writer: Option<&str>,
        base_hash: Option<String>,
        observed_hash: Option<String>,
    ) -> Result<ClaimConflict, ClaimError> {
        let now = now_ms();
        let id = format!("cf-{}-{}", now, short_hash(path));
        sqlx::query(
            "INSERT INTO claim_conflicts
               (id, path, claimant_id, writer_id, base_hash, observed_hash, detected_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(path)
        .bind(claimant)
        .bind(writer)
        .bind(&base_hash)
        .bind(&observed_hash)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // The conflict row is the record; the renderable sides are what makes it answerable.
        // A capture that fails must not lose the conflict, so its error is not propagated —
        // the row is still resolvable, it just cannot be merged.
        let captured = self
            .capture_sides(
                path,
                claimant,
                base_hash.as_deref(),
                observed_hash.as_deref(),
            )
            .await;
        let _ = self.store_sides(&id, &captured).await;

        Ok(ClaimConflict {
            id,
            path: path.to_string(),
            claimant: claimant.map(str::to_string),
            writer: writer.map(str::to_string),
            base_hash,
            observed_hash,
            detected_at: now,
            resolved_at: None,
            resolution: None,
        })
    }

    /// Read the three versions a conflict is about, once, when it is recorded (D79).
    ///
    /// `theirs` is the file as it stands. `base` is the text captured when the claim was taken —
    /// the version the claimant started from — and `ours` is the claimant's applied patches walked
    /// forward from it. Every step is checked against the hash the ledger recorded, so a rendered
    /// pane is never a reconstruction that disagrees with the record.
    ///
    /// When no claim text exists (a claim from before 0010, or a binary file) the sides are rebuilt
    /// backwards from the writer's patch instead. Either way a side that cannot be rebuilt is
    /// `None`, and `note` says why.
    async fn capture_sides(
        &self,
        path: &str,
        claimant: Option<&str>,
        claim_base: Option<&str>,
        observed: Option<&str>,
    ) -> ConflictCapture {
        let mut captured = ConflictCapture::default();
        let mut notes: Vec<String> = Vec::new();

        let pipeline = PatchPipeline::new(&self.root, self.pool.clone());
        let (theirs_text, theirs_hash) = match pipeline.read(path) {
            Ok(pair) => pair,
            Err(_) => {
                captured.note = Some(
                    "the file could not be read after the conflict (binary or gone)".to_string(),
                );
                return captured;
            }
        };
        match observed {
            Some(observed) if observed != theirs_hash => {
                notes.push("the file changed again after the conflict was recorded".to_string());
            }
            Some(_) => {}
            None => notes.push(
                "no observed version was recorded (a claim collision, or a deleted file); the pane shows the file as it stands"
                    .to_string(),
            ),
        }
        captured.theirs = Some(ConflictText {
            hash: theirs_hash.clone(),
            text: theirs_text.clone(),
        });

        match self.claimed_text(path, claimant).await {
            Some(base) => {
                captured.base = Some(base.clone());
                match self.claimant_version(path, claimant, &base).await {
                    Some(ours) => captured.ours = Some(ours),
                    None => notes.push(
                        "ours could not be rebuilt: the claimant's patches no longer hash to the recorded text"
                            .to_string(),
                    ),
                }
            }
            None => {
                let (base, ours) = self
                    .rebuilt_backwards(
                        path,
                        &theirs_text,
                        &theirs_hash,
                        claimant,
                        claim_base,
                        &mut notes,
                    )
                    .await;
                captured.base = base;
                captured.ours = ours;
            }
        }

        captured.note = if notes.is_empty() {
            None
        } else {
            Some(format!("{}.", notes.join("; ")))
        };
        captured
    }

    /// The text a claim started from, if it was captured and still hashes to the claim's hash.
    async fn claimed_text(&self, path: &str, claimant: Option<&str>) -> Option<ConflictText> {
        let agent = claimant?;
        let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT base_hash, base_text FROM claims
             WHERE path = ? AND agent_id = ?
             ORDER BY acquired_at DESC LIMIT 1",
        )
        .bind(path)
        .bind(agent)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let (hash, text) = row?;
        let (hash, text) = (hash?, text?);
        (sha256_hex(text.as_bytes()) == hash).then_some(ConflictText { hash, text })
    }

    /// The claimant's version: its applied patches, walked forward from `base` and verified at
    /// every step. With no patches, the claimant's version is the base itself.
    async fn claimant_version(
        &self,
        path: &str,
        claimant: Option<&str>,
        base: &ConflictText,
    ) -> Option<ConflictText> {
        let mut current = base.clone();
        for _ in 0..MAX_PATCH_CHAIN {
            let row: Option<(String, String)> = sqlx::query_as(
                "SELECT diff, result_hash FROM patches
                 WHERE path = ? AND status = 'applied' AND base_hash = ?
                   AND (agent_id = ? OR ? IS NULL)
                 ORDER BY applied_at ASC, created_at ASC LIMIT 1",
            )
            .bind(path)
            .bind(&current.hash)
            .bind(claimant)
            .bind(claimant)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();
            let Some((diff, expected)) = row else {
                return Some(current);
            };
            let next = apply_patch_text(&diff, &current.text).ok()?;
            let hash = sha256_hex(next.as_bytes());
            if hash != expected {
                return None;
            }
            current = ConflictText { hash, text: next };
        }
        None
    }

    /// The pre-0010 fallback: rebuild `ours` and `base` from the writer's patch backwards.
    async fn rebuilt_backwards(
        &self,
        path: &str,
        theirs_text: &str,
        theirs_hash: &str,
        claimant: Option<&str>,
        claim_base: Option<&str>,
        notes: &mut Vec<String>,
    ) -> (Option<ConflictText>, Option<ConflictText>) {
        let writer: Option<(String, String)> = sqlx::query_as(
            "SELECT diff, base_hash FROM patches
             WHERE path = ? AND status = 'applied' AND result_hash = ?
             ORDER BY applied_at DESC, created_at DESC LIMIT 1",
        )
        .bind(path)
        .bind(theirs_hash)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        let Some((diff, expected)) = writer else {
            notes.push(
                "ours could not be rebuilt: the write that produced this file is not in the patch journal"
                    .to_string(),
            );
            return (None, None);
        };
        let ours_text = match reverse_patch(&diff, theirs_text) {
            Ok(text) => text,
            Err(_) => {
                notes
                    .push("ours could not be rebuilt: the stored diff did not reverse".to_string());
                return (None, None);
            }
        };
        let ours_hash = sha256_hex(ours_text.as_bytes());
        if ours_hash != expected {
            notes.push("ours did not match the hash the ledger recorded".to_string());
            return (None, None);
        }
        let ours = ConflictText {
            hash: ours_hash.clone(),
            text: ours_text,
        };

        let previous: Option<(String, String)> = sqlx::query_as(
            "SELECT diff, base_hash FROM patches
             WHERE path = ? AND status = 'applied' AND result_hash = ?
               AND (agent_id = ? OR ? IS NULL)
             ORDER BY applied_at DESC, created_at DESC LIMIT 1",
        )
        .bind(path)
        .bind(&ours_hash)
        .bind(claimant)
        .bind(claimant)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let base = match previous {
            Some((diff, expected)) => reverse_patch(&diff, &ours.text).ok().and_then(|text| {
                let hash = sha256_hex(text.as_bytes());
                (hash == expected).then_some(ConflictText { hash, text })
            }),
            None if claim_base == Some(ours_hash.as_str()) => Some(ours.clone()),
            None => None,
        };
        if base.is_none() {
            notes.push(
                "base could not be rebuilt: the version ours replaced is not in the journal"
                    .to_string(),
            );
        }
        (base, Some(ours))
    }

    async fn store_sides(&self, id: &str, captured: &ConflictCapture) -> Result<(), ClaimError> {
        sqlx::query(
            "INSERT INTO conflict_sides
               (conflict_id, base_text, base_hash, ours_text, ours_hash, theirs_text, theirs_hash,
                note, captured_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(captured.base.as_ref().map(|side| &side.text))
        .bind(captured.base.as_ref().map(|side| &side.hash))
        .bind(captured.ours.as_ref().map(|side| &side.text))
        .bind(captured.ours.as_ref().map(|side| &side.hash))
        .bind(captured.theirs.as_ref().map(|side| &side.text))
        .bind(captured.theirs.as_ref().map(|side| &side.hash))
        .bind(&captured.note)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Open conflict rows, newest first.
    pub async fn conflicts(&self) -> Result<Vec<ClaimConflict>, ClaimError> {
        let rows = sqlx::query(
            "SELECT id, path, claimant_id, writer_id, base_hash, observed_hash, detected_at,
                    resolved_at, resolution
             FROM claim_conflicts WHERE resolved_at IS NULL ORDER BY detected_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| ClaimConflict {
                id: row.get("id"),
                path: row.get("path"),
                claimant: row.get("claimant_id"),
                writer: row.get("writer_id"),
                base_hash: row.get("base_hash"),
                observed_hash: row.get("observed_hash"),
                detected_at: row.get("detected_at"),
                resolved_at: row.get("resolved_at"),
                resolution: row.get("resolution"),
            })
            .collect())
    }

    /// Mark a conflict seen/handled. The row stays; `resolution` says what happened.
    pub async fn resolve_conflict(&self, id: &str, resolution: &str) -> Result<(), ClaimError> {
        sqlx::query(
            "UPDATE claim_conflicts SET resolved_at = ?, resolution = ?
             WHERE id = ? AND resolved_at IS NULL",
        )
        .bind(now_ms())
        .bind(resolution)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The three rendered versions of a conflict, for the merge view.
    pub async fn sides(&self, id: &str) -> Result<ConflictSides, ClaimError> {
        let conflict = self.conflict(id).await?;
        type SideRow = (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let row: Option<SideRow> = sqlx::query_as(
            "SELECT base_text, base_hash, ours_text, ours_hash, theirs_text, theirs_hash,
                    note, merged_patch_id, merged_hash
             FROM conflict_sides WHERE conflict_id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let Some((
            base_text,
            base_hash,
            ours_text,
            ours_hash,
            theirs_text,
            theirs_hash,
            note,
            patch_id,
            merged_hash,
        )) = row
        else {
            return Ok(ConflictSides {
                id: conflict.id,
                path: conflict.path,
                base: None,
                ours: None,
                theirs: None,
                note: Some(
                    "recorded before Rigel kept the three versions; it can be resolved but not merged"
                        .to_string(),
                ),
                merged_patch_id: None,
                merged_hash: None,
            });
        };

        let side = |text: Option<String>, hash: Option<String>| match (text, hash) {
            (Some(text), Some(hash)) => Some(ConflictText { hash, text }),
            _ => None,
        };
        Ok(ConflictSides {
            id: conflict.id,
            path: conflict.path,
            base: side(base_text, base_hash),
            ours: side(ours_text, ours_hash),
            theirs: side(theirs_text, theirs_hash),
            note,
            merged_patch_id: patch_id,
            merged_hash,
        })
    }

    /// One conflict row by id, resolved or not.
    pub async fn conflict(&self, id: &str) -> Result<ClaimConflict, ClaimError> {
        let row = sqlx::query(
            "SELECT id, path, claimant_id, writer_id, base_hash, observed_hash, detected_at,
                    resolved_at, resolution
             FROM claim_conflicts WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Err(ClaimError::UnknownConflict(id.to_string()));
        };
        Ok(ClaimConflict {
            id: row.get("id"),
            path: row.get("path"),
            claimant: row.get("claimant_id"),
            writer: row.get("writer_id"),
            base_hash: row.get("base_hash"),
            observed_hash: row.get("observed_hash"),
            detected_at: row.get("detected_at"),
            resolved_at: row.get("resolved_at"),
            resolution: row.get("resolution"),
        })
    }

    /// The merge text for a conflict, computed and not written (D79). Refuses rather than guesses
    /// when a side could not be rebuilt.
    pub async fn merge(&self, id: &str) -> Result<MergedConflict, ClaimError> {
        let sides = self.sides(id).await?;
        let (base, ours, theirs) = match (&sides.base, &sides.ours, &sides.theirs) {
            (Some(base), Some(ours), Some(theirs)) => (base, ours, theirs),
            _ => {
                return Err(ClaimError::SidesUnavailable(sides.note.unwrap_or_else(
                    || "a side is missing and Rigel will not invent it".to_string(),
                )));
            }
        };
        let result = crate::merge::three_way(&base.text, &ours.text, &theirs.text)?;
        Ok(MergedConflict {
            id: sides.id,
            path: sides.path,
            merged: result.merged,
            conflicts: result.conflicts,
            clean: result.clean,
        })
    }

    /// Resolve a conflict by writing the merged text through the CAS patch pipeline (D79).
    ///
    /// The compare-and-swap base is the file *as it is now*, so a write that lands between the
    /// merge preview and this call fails with `StaleBase` instead of overwriting it. Nothing is
    /// marked resolved until the file has been written.
    pub async fn resolve_with_merge(
        &self,
        pipeline: &PatchPipeline,
        session_id: &str,
        id: &str,
        merged: &str,
    ) -> Result<MergeResolution, ClaimError> {
        let conflict = self.conflict(id).await?;
        if conflict.resolved_at.is_some() {
            return Err(ClaimError::ConflictClosed(id.to_string()));
        }
        let sides = self.sides(id).await?;

        let (current_text, current_hash) = match pipeline.read(&conflict.path) {
            Ok(pair) => pair,
            Err(PatchError::NotFound(_)) => {
                return Err(ClaimError::SidesUnavailable(format!(
                    "{} no longer exists; nothing to write the merge into",
                    conflict.path
                )));
            }
            Err(error) => return Err(ClaimError::Patch(error)),
        };

        // The merge the human read was built from the captured sides. If the file has moved on
        // since then, writing would overwrite a version nobody saw: refuse instead.
        if let Some(theirs) = &sides.theirs {
            if current_hash != theirs.hash {
                return Err(ClaimError::Patch(PatchError::StaleBase {
                    expected: theirs.hash.clone(),
                    current: current_hash,
                }));
            }
        }

        if merged == current_text {
            self.finish_merge(
                id,
                None,
                &current_hash,
                "merged: the file already held the merged text",
            )
            .await?;
            return Ok(MergeResolution {
                path: conflict.path,
                patch: None,
                unchanged: true,
            });
        }

        let patch = pipeline
            .propose_with(
                session_id,
                MERGE_AUTHOR,
                &conflict.path,
                &current_hash,
                merged,
                StopAt::Never,
            )
            .await?;
        self.finish_merge(
            id,
            Some(&patch.id),
            &patch.result_hash,
            "merged the three versions",
        )
        .await?;
        Ok(MergeResolution {
            path: conflict.path,
            patch: Some(patch),
            unchanged: false,
        })
    }

    async fn finish_merge(
        &self,
        id: &str,
        patch_id: Option<&str>,
        merged_hash: &str,
        resolution: &str,
    ) -> Result<(), ClaimError> {
        sqlx::query(
            "UPDATE claim_conflicts SET resolved_at = ?, resolution = ?
             WHERE id = ? AND resolved_at IS NULL",
        )
        .bind(now_ms())
        .bind(resolution)
        .bind(id)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "UPDATE conflict_sides SET merged_patch_id = ?, merged_hash = ? WHERE conflict_id = ?",
        )
        .bind(patch_id)
        .bind(merged_hash)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// A project-relative path, or an error naming the reason. Normalisation is what makes two
    /// agents' `./src/app.tsx` and `src/app.tsx` the same claim.
    pub fn normalize(&self, raw: &str) -> Result<String, ClaimError> {
        let raw = raw.trim();
        if raw.chars().count() > MAX_PATH_CHARS {
            return Err(ClaimError::PathTooLong {
                path: raw.to_string(),
                chars: raw.chars().count(),
            });
        }
        let candidate = Path::new(raw);
        if raw.is_empty() || candidate.is_absolute() {
            return Err(ClaimError::InvalidPath {
                path: raw.to_string(),
            });
        }
        let mut parts = Vec::new();
        for part in candidate.components() {
            match part {
                std::path::Component::Normal(part) => {
                    parts.push(part.to_string_lossy().into_owned())
                }
                std::path::Component::CurDir => {}
                _ => {
                    return Err(ClaimError::InvalidPath {
                        path: raw.to_string(),
                    });
                }
            }
        }
        if parts.is_empty() {
            return Err(ClaimError::InvalidPath {
                path: raw.to_string(),
            });
        }
        let joined = parts.join("/");
        // A path that resolves outside the root through a symlink is not project-relative.
        let full = self.root.join(&joined);
        if let (Ok(canonical_root), Ok(canonical)) = (self.root.canonicalize(), full.canonicalize())
        {
            if !canonical.starts_with(&canonical_root) {
                return Err(ClaimError::InvalidPath {
                    path: raw.to_string(),
                });
            }
        }
        Ok(joined)
    }

    async fn active(&self, path: &str) -> Result<Option<Claim>, ClaimError> {
        let rows = self.claims(Some(path), None, true).await?;
        Ok(rows.into_iter().next())
    }

    /// One query builder for every read: `exact` matches one path, `prefix` matches a path or a
    /// directory below it, and `live_only` hides released rows.
    async fn claims(
        &self,
        exact: Option<&str>,
        filter: Option<&str>,
        live_only: bool,
    ) -> Result<Vec<Claim>, ClaimError> {
        let exact = match exact {
            Some(raw) => Some(self.normalize(raw)?),
            None => None,
        };
        let filter = match filter {
            Some(raw) => Some(self.normalize(raw)?),
            None => None,
        };
        let mut sql = String::from(
            "SELECT c.id, c.path, c.agent_id, c.base_hash, c.acquired_at, c.heartbeat_at,
                    c.released_at, c.release_reason, a.role, a.harness
             FROM claims c LEFT JOIN agents a ON a.id = c.agent_id WHERE 1 = 1",
        );
        if live_only {
            sql.push_str(" AND c.released_at IS NULL");
        }
        if exact.is_some() {
            sql.push_str(" AND c.path = ?");
        }
        if filter.is_some() {
            sql.push_str(" AND (c.path = ? OR c.path LIKE ?)");
        }
        sql.push_str(" ORDER BY c.acquired_at DESC");
        let mut query = sqlx::query(&sql);
        if let Some(exact) = &exact {
            query = query.bind(exact);
        }
        if let Some(filter) = &filter {
            query = query.bind(filter).bind(format!("{filter}/%"));
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let role: Option<String> = row.get("role");
                let harness: String = row.get("harness");
                Claim {
                    id: row.get("id"),
                    path: row.get("path"),
                    agent_id: row.get("agent_id"),
                    agent: role.unwrap_or_else(|| harness.clone()),
                    harness,
                    base_hash: row.get("base_hash"),
                    acquired_at: row.get("acquired_at"),
                    heartbeat_at: row.get("heartbeat_at"),
                    released_at: row.get("released_at"),
                    release_reason: row.get("release_reason"),
                    stale: false,
                    held: false,
                }
            })
            .collect())
    }

    async fn ensure_agent(&self, agent_id: &str) -> Result<(), ClaimError> {
        let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM agents WHERE id = ?")
            .bind(agent_id)
            .fetch_optional(&self.pool)
            .await?;
        if exists.is_some() {
            Ok(())
        } else {
            Err(ClaimError::UnknownAgent(agent_id.to_string()))
        }
    }
}

/// A file that a claim covers but whose bytes have moved, plus the best guess at who moved them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnverifiedChange {
    pub path: String,
    pub agent: Option<String>,
    pub claimant: Option<String>,
    pub base_hash: Option<String>,
    pub observed_hash: Option<String>,
    /// More than one agent could have written it: the UI must say so rather than pick one.
    pub ambiguous: bool,
}

/// A claim id that is unique within a project and readable in a log line.
fn new_claim_id(path: &str, agent_id: &str, now: i64) -> String {
    format!("cl-{}-{}-{}", now, short_hash(agent_id), short_hash(path))
}

fn short_hash(value: &str) -> String {
    sha256_hex(value.as_bytes())[..12].to_string()
}

/// `sha256` of a file, or `None` when it does not exist yet.
fn read_hash(root: &Path, path: &str) -> Option<String> {
    let bytes = std::fs::read(root.join(path)).ok()?;
    Some(sha256_hex(&bytes))
}

/// The claimed file's text, when it is text and small enough to keep on the claim row.
///
/// This is the only moment the version the claimant started from is on disk (D79). A binary or
/// oversized file stores nothing: the claim still works, the conflict just shows fewer sides.
fn read_text(root: &Path, path: &str) -> Option<String> {
    let bytes = std::fs::read(root.join(path)).ok()?;
    if bytes.len() > MAX_CLAIM_TEXT_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionService;

    async fn service() -> (tempfile::TempDir, ClaimService, String, String) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/app.tsx"), "export const A = 1;\n").unwrap();
        let sessions = SessionService::open(tmp.path()).await.unwrap();
        let session = sessions.start(Some("claims"), Some(100)).await.unwrap();
        let one = sessions
            .register_agent(&session.id, "native", Some("writer"))
            .await
            .unwrap();
        let two = sessions
            .register_agent(&session.id, "codex", Some("reader"))
            .await
            .unwrap();
        let claims = ClaimService::new(sessions.pool().clone(), tmp.path());
        (tmp, claims, one.id, two.id)
    }

    #[tokio::test]
    async fn a_claim_is_advisory_and_a_second_claimant_is_told_who_holds_it() {
        let (_tmp, claims, one, two) = service().await;
        let path = vec!["src/app.tsx".to_string()];
        let first = claims
            .acquire(&one, &path, "edit the button")
            .await
            .unwrap();
        assert!(first[0].held);
        assert!(first[0].held_by.as_ref().unwrap().base_hash.is_some());

        let second = claims
            .acquire(&two, &path, "also the button")
            .await
            .unwrap();
        assert!(!second[0].held, "the incumbent keeps the live row");
        assert_eq!(second[0].held_by.as_ref().unwrap().agent_id, one);

        // The collision is recorded as a conflict row, not hidden.
        let conflicts = claims.conflicts().await.unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].path, "src/app.tsx");
        assert_eq!(conflicts[0].claimant.as_deref(), Some(one.as_str()));
        assert_eq!(conflicts[0].writer.as_deref(), Some(two.as_str()));
    }

    #[tokio::test]
    async fn claiming_twice_refreshes_the_heartbeat_rather_than_adding_a_row() {
        let (_tmp, claims, one, _two) = service().await;
        let path = vec!["./src/app.tsx".to_string()];
        claims.acquire(&one, &path, "first").await.unwrap();
        let second = claims
            .acquire(&one, &["src/app.tsx".to_string()], "again")
            .await
            .unwrap();
        assert!(second[0].held);
        let history = claims.history(None).await.unwrap();
        assert_eq!(history.len(), 1, "normalised to the same path");
        assert_eq!(history[0].path, "src/app.tsx");
    }

    #[tokio::test]
    async fn a_release_is_a_tombstone_and_a_reclaim_gets_a_new_row() {
        let (_tmp, claims, one, _two) = service().await;
        let path = vec!["src/app.tsx".to_string()];
        claims.acquire(&one, &path, "work").await.unwrap();
        let released = claims.release(&one, &path, "done").await.unwrap();
        assert_eq!(released, vec!["src/app.tsx".to_string()]);
        assert!(claims.held(None).await.unwrap().is_empty());
        let history = claims.history(None).await.unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].released_at.is_some());
        assert_eq!(history[0].release_reason.as_deref(), Some("done"));

        claims.acquire(&one, &path, "again").await.unwrap();
        let history = claims.history(None).await.unwrap();
        assert_eq!(history.len(), 2, "history keeps the released row");
    }

    #[tokio::test]
    async fn expiry_releases_after_twenty_four_hours_and_says_why() {
        let (_tmp, claims, one, _two) = service().await;
        claims
            .acquire(&one, &["src/app.tsx".to_string()], "old")
            .await
            .unwrap();
        let now = now_ms() + 25 * 60 * 60 * 1000;
        let expired = claims.expire(now).await.unwrap();
        assert_eq!(expired, vec!["src/app.tsx".to_string()]);
        let history = claims.history(None).await.unwrap();
        assert_eq!(history[0].release_reason.as_deref(), Some("expired"));
        assert!(claims.held(None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_stale_claim_is_reported_and_never_auto_released() {
        let (_tmp, claims, one, _two) = service().await;
        claims
            .acquire(&one, &["src/app.tsx".to_string()], "slow")
            .await
            .unwrap();
        // Backdate the heartbeat past the stale threshold.
        let old = now_ms() - STALE_AFTER.as_millis() as i64 - 1_000;
        sqlx::query("UPDATE claims SET heartbeat_at = ?")
            .bind(old)
            .execute(&claims.pool)
            .await
            .unwrap();

        let held = claims.held(None).await.unwrap();
        assert_eq!(held.len(), 1, "stale is still held");
        assert!(held[0].stale);
        assert!(held[0].held);
        assert_eq!(claims.conflicts().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn a_changed_file_under_a_claim_is_reported_as_unverified() {
        let (_tmp, claims, one, two) = service().await;
        claims
            .acquire(&one, &["src/app.tsx".to_string()], "edit")
            .await
            .unwrap();
        // Somebody wrote the file without a patch: the claim's base hash no longer matches.
        std::fs::write(_tmp.path().join("src/app.tsx"), "export const A = 2;\n").unwrap();
        let changes = claims.unverified().await.unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "src/app.tsx");
        assert_eq!(changes[0].claimant.as_deref(), Some(one.as_str()));
        assert_eq!(changes[0].agent.as_deref(), Some(one.as_str()));
        assert!(!changes[0].ambiguous);
        // `two` never claimed anything, so nothing is attributed to it.
        let theirs = claims.held(Some("src")).await.unwrap();
        assert_eq!(theirs.len(), 1);
        assert_ne!(theirs[0].agent_id, two);
    }

    #[tokio::test]
    async fn a_path_outside_the_project_is_refused_with_a_reason() {
        let (_tmp, claims, one, _two) = service().await;
        for bad in ["../secrets", "/etc/passwd", "", "a/../../b"] {
            let results = claims
                .acquire(&one, &[bad.to_string()], "nope")
                .await
                .unwrap();
            assert!(!results[0].held, "{bad:?}");
            assert!(results[0].error.is_some(), "{bad:?}");
        }
        assert!(claims.held(None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn who_lists_claims_by_path_and_prefix_with_the_holders_name() {
        let (_tmp, claims, one, _two) = service().await;
        std::fs::write(_tmp.path().join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        claims
            .acquire(
                &one,
                &["src/app.tsx".to_string(), "src/lib.rs".to_string()],
                "both",
            )
            .await
            .unwrap();
        assert_eq!(claims.held(Some("src/app.tsx")).await.unwrap().len(), 1);
        assert_eq!(claims.held(Some("src")).await.unwrap().len(), 2);
        let held = claims.held(Some("src")).await.unwrap();
        assert!(held.iter().all(|claim| claim.agent == "writer"));
        assert_ne!(held[0].id, held[1].id);
    }
}

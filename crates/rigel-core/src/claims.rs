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
use crate::patches::sha256_hex;

/// §8.1: a claim path is project-relative and at most 200 characters.
pub const MAX_PATH_CHARS: usize = 200;
/// The default lifetime of one claim before a stale scan may expire it.
pub const CLAIM_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// A heartbeat older than this makes a claim stale; stale is reported, never auto-released.
pub const STALE_AFTER: Duration = Duration::from_secs(90);

#[derive(Debug, thiserror::Error)]
pub enum ClaimError {
    #[error("{path:?} is not a project-relative path")]
    InvalidPath { path: String },

    #[error("{path:?} is {chars} characters; the limit is {MAX_PATH_CHARS}")]
    PathTooLong { path: String, chars: usize },

    #[error("agent {0} is not registered in this project")]
    UnknownAgent(String),

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
            let id = new_claim_id(&path, agent_id, now);
            sqlx::query(
                "INSERT INTO claims (id, path, agent_id, base_hash, acquired_at, heartbeat_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(&path)
            .bind(agent_id)
            .bind(&base_hash)
            .bind(now)
            .bind(now)
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

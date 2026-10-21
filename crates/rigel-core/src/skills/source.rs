//! The remote-source contract.
//!
//! A `Source` knows how to talk to one place skills come from, and nothing else: it does not
//! write the index and it does not decide what the UI shows. `github` implements repo →
//! skills; `hub` implements the two marketplaces, which can be searched but not walked.
//!
//! Every method has a default that reports "not supported by this source" rather than an
//! `unimplemented!()`, so adding a source means implementing the two or three methods it
//! actually has an answer for.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::skills::{Capability, SkillsError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncPolicy {
    /// Ignore the freshness window and revalidate every cached URL, conditional where the
    /// server gave us an ETag. Never implies "ignore the cache": a `304` still saves the body.
    pub force: bool,
}

impl SyncPolicy {
    pub fn normal() -> Self {
        Self { force: false }
    }

    pub fn forced() -> Self {
        Self { force: true }
    }
}

impl Default for SyncPolicy {
    fn default() -> Self {
        Self::normal()
    }
}

/// One row of the index, before it is written.
#[derive(Debug, Clone)]
pub struct SkillRecord {
    /// `source:repo:path` for GitHub, `source:slug` for a marketplace.
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub repo: Option<String>,
    pub path: Option<String>,
    pub url: Option<String>,
    pub body_sha: Option<String>,
    pub grade: Option<String>,
    pub grade_scope: Option<String>,
    pub license: Option<String>,
    pub stars: Option<i64>,
    pub installs: Option<i64>,
    pub tokens: Option<i64>,
    pub capabilities: Vec<Capability>,
    pub simhash: Option<i64>,
}

impl SkillRecord {
    pub fn source_id(&self) -> &str {
        &self.source
    }

    pub fn capabilities_json(&self) -> Option<String> {
        serde_json::to_string(&self.capabilities).ok()
    }

    /// Frontmatter plus body come to roughly four characters per token, which is the same
    /// estimate the prototype used and is only ever shown as "about".
    pub fn estimate_tokens(body: &str) -> i64 {
        ((body.len() as f64 / 4.0).round() as i64).max(120)
    }
}

#[derive(Debug, Clone)]
pub struct FetchedSkill {
    pub record: SkillRecord,
    pub body: String,
}

/// The result of walking one repository.
#[derive(Debug, Clone)]
pub struct RepoSnapshot {
    pub source: String,
    pub repo: String,
    pub branch: String,
    pub stars: i64,
    pub license: Option<String>,
    pub truncated: bool,
    pub found: usize,
    /// HTTP requests this walk actually made. `0` on a re-index inside the freshness window,
    /// which is exactly what P2.11 asserts.
    pub http_requests: usize,
    pub skills: Vec<FetchedSkill>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceState {
    Ok,
    Stale,
    Broken,
}

impl SourceState {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceState::Ok => "ok",
            SourceState::Stale => "stale",
            SourceState::Broken => "broken",
        }
    }
}

/// What the shelf's status line shows. `last_sync_at` is kept even when the source is broken:
/// "last reached GitHub 3h ago" is more useful than "broken".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceStatus {
    pub id: String,
    pub base_url: Option<String>,
    pub state: SourceState,
    pub last_sync_at: Option<i64>,
    pub error: Option<String>,
}

/// Read one source's health from the `sources` row and the clock.
///
/// The three states mean something specific to the user:
/// * `ok` — synced inside the last [`crate::skills::STALE_AFTER_MS`];
/// * `stale` — reachable last time, but the data is older than that, or has never been synced;
/// * `broken` — the last attempt failed; `error` says how, `last_sync_at` says when it worked.
///
/// It never makes a network call, because the status line must render offline.
pub async fn load_status(
    pool: &sqlx::SqlitePool,
    id: &str,
    base_url: Option<&str>,
) -> Result<SourceStatus, sqlx::Error> {
    let row: Option<(Option<i64>, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT last_sync_at, status, error FROM sources WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;

    let now = crate::migrate::now_ms();
    let (last_sync_at, state, error) = match row {
        None => (None, SourceState::Stale, None),
        Some((last_sync_at, status, error)) => {
            let state = match status.as_deref() {
                Some("broken") => SourceState::Broken,
                Some("ok")
                    if last_sync_at.is_some_and(|at| now - at < crate::skills::STALE_AFTER_MS) =>
                {
                    SourceState::Ok
                }
                _ => SourceState::Stale,
            };
            (last_sync_at, state, error)
        }
    };

    Ok(SourceStatus {
        id: id.to_string(),
        base_url: base_url.map(ToOwned::to_owned),
        state,
        last_sync_at,
        error,
    })
}

#[async_trait]
pub trait Source: Send + Sync {
    fn id(&self) -> &'static str;

    /// Source health, read from the `sources` row and the clock — never from a network call.
    async fn status(&self) -> Result<SourceStatus, SkillsError>;

    /// Full-text search over the source itself. Marketplaces have one; GitHub does not.
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SkillRecord>, SkillsError> {
        let _ = (query, limit);
        Err(SkillsError::Unsupported("search", self.id()))
    }

    /// Walk a repository (or a directory inside it) and return every skill it contains.
    async fn fetch_repo(
        &self,
        target: &str,
        policy: SyncPolicy,
    ) -> Result<RepoSnapshot, SkillsError> {
        let _ = (target, policy);
        Err(SkillsError::Unsupported("fetch_repo", self.id()))
    }

    /// One skill's body, by index id.
    async fn fetch_skill(&self, id: &str) -> Result<FetchedSkill, SkillsError> {
        let _ = id;
        Err(SkillsError::Unsupported("fetch_skill", self.id()))
    }
}

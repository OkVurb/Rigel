//! `SkillIndex` — the one place that writes the index.
//!
//! Ordering inside an ingest is the storage rule from §6.4: **blob first, then row**. A crash
//! between the two leaves an orphan body that `rigel gc` collects; the other order leaves a
//! row pointing at a body that never existed, which is unrecoverable without a re-fetch.
//!
//! Everything here is offline-capable by construction: `search`, `get` and `status` read only
//! SQLite and the blob directory, and take no `Source`. Only `index_repo` and the marketplace
//! sync methods touch the network.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sqlx::Row;

use super::blob::BlobStore;
use super::cluster;
use super::evidence::{Evidence, EvidenceSummary, claims_from_description};
use super::github::GitHubSource;
use super::hub::{SkillHubSource, SkillsMpSource};
use super::scan::Capability;
use super::search::{SearchHit, SearchOrder, fts_query};
use super::source::{
    RepoSnapshot, SkillRecord, Source, SourceState, SourceStatus, SyncPolicy, load_status,
};
use crate::migrate::now_ms;
use crate::skills::{DEFAULT_TTL, SkillsError};
use crate::storage::Store;

const GITHUB_BASE: &str = "https://api.github.com";

pub struct SkillIndex {
    store: Store,
    blobs: BlobStore,
}

#[derive(Debug, Clone, Serialize)]
pub struct IngestReport {
    pub source: String,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub found: usize,
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub with_body: usize,
    pub http_requests: usize,
    pub truncated: bool,
    pub clusters: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub imported: usize,
    pub skipped: usize,
    pub invalid: usize,
}

/// One `rigel skills sweep` — both marketplace walks, their row counts, and what the
/// meta-only duplicate pass dropped. Errors are fields, not aborts: a half-swept index
/// is still an index.
#[derive(Debug, Clone, Serialize)]
pub struct SweepReport {
    pub skillhub_pages: usize,
    pub skillhub_rows: usize,
    pub skillhub_error: Option<String>,
    pub skillsmp_queries: usize,
    pub skillsmp_pages: usize,
    pub skillsmp_rows: usize,
    pub skillsmp_error: Option<String>,
    pub duplicates_dropped: i64,
    pub skills_before: i64,
    pub skills_after: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct VariantRow {
    pub id: String,
    pub name: String,
    pub repo: Option<String>,
    pub grade: Option<String>,
    pub license: Option<String>,
    pub last_seen_at: Option<i64>,
    /// A human sentence for the disclosure: "identical content", "3 bits away", "no license".
    pub delta: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillDetail {
    pub skill: SearchHit,
    pub capabilities: Vec<Capability>,
    pub evidence: Vec<EvidenceSummary>,
    pub variants: Vec<VariantRow>,
    pub first_seen_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexStats {
    pub skills: i64,
    pub with_body: i64,
    pub clusters: i64,
    pub blobs: usize,
    pub blob_bytes: u64,
    pub sources: Vec<SourceStatus>,
}

impl SkillIndex {
    pub async fn open(data_dir: &Path) -> Result<Self, SkillsError> {
        let store = Store::open_index(data_dir).await?;
        Ok(Self {
            store,
            blobs: BlobStore::new(data_dir),
        })
    }

    pub fn from_store(store: Store, data_dir: &Path) -> Self {
        Self {
            store,
            blobs: BlobStore::new(data_dir),
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    pub fn data_dir(&self) -> PathBuf {
        self.store
            .path()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    }

    pub async fn github(&self) -> Result<GitHubSource, SkillsError> {
        GitHubSource::new(self.store.pool().clone(), self.blobs.clone(), DEFAULT_TTL).await
    }

    pub async fn skillsmp(&self) -> Result<SkillsMpSource, SkillsError> {
        SkillsMpSource::new(self.store.pool().clone(), self.blobs.clone(), DEFAULT_TTL).await
    }

    pub async fn skillhub(&self) -> Result<SkillHubSource, SkillsError> {
        SkillHubSource::new(self.store.pool().clone(), self.blobs.clone(), DEFAULT_TTL).await
    }

    /// Walk a repository and write what it finds. Marks `github` broken on failure, so the
    /// shelf can say "last reached 3h ago" instead of showing a stale list as if it were live.
    pub async fn index_repo(
        &self,
        target: &str,
        policy: SyncPolicy,
    ) -> Result<IngestReport, SkillsError> {
        self.walk_repo(target, policy, None).await
    }

    /// Index one skill by its directory name. The skills.sh import uses this so a page costs
    /// one body, not a whole repository's worth of requests.
    pub async fn index_skill(
        &self,
        target: &str,
        policy: SyncPolicy,
        slug: &str,
    ) -> Result<IngestReport, SkillsError> {
        self.walk_repo(target, policy, Some(slug)).await
    }

    async fn walk_repo(
        &self,
        target: &str,
        policy: SyncPolicy,
        slug: Option<&str>,
    ) -> Result<IngestReport, SkillsError> {
        let mut source = self.github().await?;
        if let Some(slug) = slug {
            source = source.with_slug(slug);
        }
        match source.fetch_repo(target, policy).await {
            Ok(snapshot) => self.ingest(&snapshot).await,
            Err(error) => {
                let _ = self
                    .set_source(
                        "github",
                        GITHUB_BASE,
                        SourceState::Broken,
                        Some(&error.to_string()),
                    )
                    .await;
                Err(error)
            }
        }
    }

    pub async fn ingest(&self, snapshot: &RepoSnapshot) -> Result<IngestReport, SkillsError> {
        let mut added = 0;
        let mut updated = 0;
        let mut unchanged = 0;

        for skill in &snapshot.skills {
            // §6.4: the body is on disk before the row that names it.
            let sha = self.blobs.put(skill.body.as_bytes())?;
            let mut record = skill.record.clone();
            record.body_sha = Some(sha.clone());

            let previous: Option<String> =
                sqlx::query_scalar("SELECT body_sha FROM skills WHERE id = ?")
                    .bind(&record.id)
                    .fetch_optional(self.store.pool())
                    .await?;

            match previous.as_deref() {
                None => added += 1,
                Some(prev) if prev == sha => unchanged += 1,
                Some(_) => updated += 1,
            }

            self.write_skill(&record).await?;

            sqlx::query(
                "INSERT OR IGNORE INTO skill_versions (skill_id, body_sha, fetched_at)
                 VALUES (?, ?, ?)",
            )
            .bind(&record.id)
            .bind(&sha)
            .bind(now_ms())
            .execute(self.store.pool())
            .await?;

            self.record_claims(&record).await?;
        }

        let clusters = self.recluster().await?;
        self.set_source("github", GITHUB_BASE, SourceState::Ok, None)
            .await?;

        Ok(IngestReport {
            source: snapshot.source.clone(),
            repo: Some(snapshot.repo.clone()),
            branch: Some(snapshot.branch.clone()),
            found: snapshot.found,
            added,
            updated,
            unchanged,
            with_body: snapshot.skills.len(),
            http_requests: snapshot.http_requests,
            truncated: snapshot.truncated,
            clusters,
        })
    }

    /// Write one skill. Used for both scanned bodies and bare marketplace rows.
    pub async fn write_skill(&self, record: &SkillRecord) -> Result<(), SkillsError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO skills
               (id, name, description, source, repo, path, url, body_sha, grade, grade_scope,
                license, stars, installs, tokens, cluster_id, first_seen_at, last_seen_at,
                capabilities_json, simhash)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               description = excluded.description,
               repo = COALESCE(excluded.repo, skills.repo),
               path = COALESCE(excluded.path, skills.path),
               url = COALESCE(excluded.url, skills.url),
               body_sha = COALESCE(excluded.body_sha, skills.body_sha),
               grade = COALESCE(excluded.grade, skills.grade),
               grade_scope = COALESCE(excluded.grade_scope, skills.grade_scope),
               license = COALESCE(excluded.license, skills.license),
               stars = COALESCE(excluded.stars, skills.stars),
               installs = COALESCE(excluded.installs, skills.installs),
               tokens = COALESCE(excluded.tokens, skills.tokens),
               last_seen_at = excluded.last_seen_at,
               capabilities_json = COALESCE(excluded.capabilities_json, skills.capabilities_json),
               simhash = COALESCE(excluded.simhash, skills.simhash)",
        )
        .bind(&record.id)
        .bind(&record.name)
        .bind(&record.description)
        .bind(&record.source)
        .bind(&record.repo)
        .bind(&record.path)
        .bind(&record.url)
        .bind(&record.body_sha)
        .bind(&record.grade)
        .bind(&record.grade_scope)
        .bind(&record.license)
        .bind(record.stars)
        .bind(record.installs)
        .bind(record.tokens)
        .bind(now)
        .bind(now)
        .bind(record.capabilities_json())
        .bind(record.simhash.map(|value| format!("{:016x}", value as u64)))
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    async fn record_claims(&self, record: &SkillRecord) -> Result<(), SkillsError> {
        for evidence in
            claims_from_description(&record.id, &record.description, record.url.as_deref())
        {
            evidence.record(self.store.pool()).await?;
        }
        Ok(())
    }

    /// Upsert metadata rows that have no body yet — the marketplaces.
    pub async fn ingest_records(&self, records: &[SkillRecord]) -> Result<usize, SkillsError> {
        for record in records {
            self.write_skill(record).await?;
            self.record_claims(record).await?;
        }
        self.recluster().await?;
        Ok(records.len())
    }

    /// Same upsert, without the per-call recluster. The sweep calls this once per page and
    /// reclusters once at the end, so a 400-page walk does one clustering pass instead of 400.
    pub async fn ingest_records_quiet(
        &self,
        records: &[SkillRecord],
    ) -> Result<usize, SkillsError> {
        for record in records {
            self.write_skill(record).await?;
            self.record_claims(record).await?;
        }
        Ok(records.len())
    }

    async fn skill_count(&self) -> Result<i64, SkillsError> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM skills")
            .fetch_one(self.store.pool())
            .await?)
    }

    /// Drop duplicate metadata rows. Two rows are the same skill only when the trimmed,
    /// case-insensitive name matches **and** the first 160 characters of the description
    /// match, with a description specific enough (≥ 40 chars) to mean anything — a row
    /// with a scanned body is never deleted. Clusters of near-variants that differ in
    /// wording are left to the simhash mechanism, which needs bodies.
    pub async fn deduplicate(&self) -> Result<i64, SkillsError> {
        let result = sqlx::query(
            "DELETE FROM skills WHERE body_sha IS NULL AND id IN (
               SELECT id FROM (
                 SELECT id, ROW_NUMBER() OVER (
                   PARTITION BY lower(trim(name)), substr(trim(description), 1, 160)
                   ORDER BY first_seen_at, id
                 ) AS ranked
                 FROM skills
                 WHERE length(trim(description)) >= 40
                   AND trim(description) <> 'No description in frontmatter.'
               ) WHERE ranked > 1
             )",
        )
        .execute(self.store.pool())
        .await?;
        Ok(result.rows_affected() as i64)
    }

    pub async fn sync_marketplace(
        &self,
        which: &str,
        query: &str,
        limit: usize,
    ) -> Result<usize, SkillsError> {
        let (base, result) = match which {
            "skillsmp" => {
                let source = self.skillsmp().await?;
                (
                    "https://skillsmp.com",
                    source.search_records(query, limit).await,
                )
            }
            "skillhub" => {
                let source = self.skillhub().await?;
                (
                    "https://skills.browseract.com",
                    source.browse(1, limit as u32).await,
                )
            }
            other => return Err(SkillsError::UnknownSource(other.to_string())),
        };

        match result {
            Ok(records) => {
                let count = self.ingest_records(&records).await?;
                self.set_source(which, base, SourceState::Ok, None).await?;
                Ok(count)
            }
            Err(error) => {
                self.set_source(which, base, SourceState::Broken, Some(&error.to_string()))
                    .await?;
                Err(error)
            }
        }
    }

    /// Sweep the two marketplaces across many pages and turn the result into metadata rows —
    /// what a skill costs nothing to know: name, description, stars, where to fetch it.
    /// No body is downloaded. Both walks are independent: a failure in one leaves the other's
    /// rows in the index and reports itself in the table.
    pub async fn sweep(
        &self,
        skillhub_pages: u32,
        skillsmp_queries: &[String],
        skillsmp_pages: u32,
    ) -> Result<SweepReport, SkillsError> {
        let skills_before = self.skill_count().await?;

        let mut skillhub_rows = 0;

        // --- SkillHub: known pages of 100 cards, stop at the first empty page ---
        let mut skillhub_error: Option<String> = None;
        if skillhub_pages > 0 {
            let source = self.skillhub().await?;
            for page in 1..=skillhub_pages {
                match source.browse(page, 100).await {
                    Ok(records) => {
                        self.ingest_records_quiet(&records).await?;
                        skillhub_rows += records.len();
                        if records.is_empty() {
                            break;
                        }
                    }
                    Err(error) => {
                        skillhub_error = Some(error.to_string());
                        break;
                    }
                }
            }
            self.set_source(
                "skillhub",
                "https://skills.browseract.com",
                match skillhub_error {
                    Some(_) => SourceState::Broken,
                    None => SourceState::Ok,
                },
                skillhub_error.as_deref(),
            )
            .await?;
        }

        // --- SkillsMP: pages of 50 per broad query, stop at the first empty page ---
        let mut skillsmp_rows = 0;
        let mut skillsmp_pages_seen = 0;
        let mut skillsmp_error: Option<String> = None;
        let mut queries_seen = 0;
        if skillsmp_pages > 0 {
            let source = self.skillsmp().await?;
            for query in skillsmp_queries {
                let query = query.trim();
                if query.is_empty() {
                    continue;
                }
                queries_seen += 1;
                for page in 1..=skillsmp_pages {
                    match source.search_records_page(query, 50, page).await {
                        Ok(records) => {
                            self.ingest_records_quiet(&records).await?;
                            skillsmp_rows += records.len();
                            skillsmp_pages_seen += 1;
                            if records.is_empty() {
                                break;
                            }
                        }
                        Err(error) => {
                            skillsmp_error.get_or_insert_with(|| error.to_string());
                            break;
                        }
                    }
                }
            }
            self.set_source(
                "skillsmp",
                "https://skillsmp.com",
                match skillsmp_error {
                    Some(_) => SourceState::Broken,
                    None => SourceState::Ok,
                },
                skillsmp_error.as_deref(),
            )
            .await?;
        }

        // One clustering pass and one duplicate pass for the whole walk.
        self.recluster().await?;
        let duplicates_dropped = self.deduplicate().await?;

        Ok(SweepReport {
            skillhub_pages: skillhub_pages as usize,
            skillhub_rows,
            skillhub_error,
            skillsmp_queries: queries_seen,
            skillsmp_pages: skillsmp_pages_seen,
            skillsmp_rows,
            skillsmp_error,
            duplicates_dropped,
            skills_before,
            skills_after: self.skill_count().await?,
        })
    }

    pub async fn set_source(
        &self,
        id: &str,
        base_url: &str,
        state: SourceState,
        error: Option<&str>,
    ) -> Result<(), SkillsError> {
        let now = if matches!(state, SourceState::Ok) {
            Some(now_ms())
        } else {
            None
        };

        sqlx::query(
            "INSERT INTO sources (id, base_url, last_sync_at, status, error)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               base_url = excluded.base_url,
               status = excluded.status,
               error = excluded.error,
               last_sync_at = COALESCE(excluded.last_sync_at, sources.last_sync_at)",
        )
        .bind(id)
        .bind(base_url)
        .bind(now)
        .bind(state.as_str())
        .bind(error)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, SkillsError> {
        let hits = self
            .search_ordered(query, limit, SearchOrder::DEFAULT)
            .await?;
        Ok(hits)
    }

    pub async fn search_ordered(
        &self,
        query: &str,
        limit: usize,
        order: SearchOrder,
    ) -> Result<Vec<SearchHit>, SkillsError> {
        self.search_page(query, limit, 0, order).await
    }

    /// One page of the same search. The window still caps a page at 500 rows; callers that
    /// want the whole catalogue page through it with `offset` (the Shelf does).
    pub async fn search_page(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
        order: SearchOrder,
    ) -> Result<Vec<SearchHit>, SkillsError> {
        let limit = limit.clamp(1, 500) as i64;
        let offset = offset as i64;
        let order_by = order.order_by();
        let match_expr = fts_query(query);

        let sql = match &match_expr {
            Some(_) => format!(
                // FTS5 rejects an alias on the left of MATCH (`WHERE f MATCH ?` is
                // "no such column: f"), so the virtual table is named in full here.
                "SELECT {COLUMNS}, {EXTRA}
                   FROM skills_fts
                   JOIN skills s ON s.rowid = skills_fts.rowid
                   LEFT JOIN (SELECT cluster_id, COUNT(*) AS n FROM skills
                               WHERE cluster_id IS NOT NULL GROUP BY cluster_id) c
                          ON c.cluster_id = s.cluster_id
                  WHERE skills_fts MATCH ?
                  ORDER BY {order_by}
                  LIMIT ? OFFSET ?"
            ),
            None => format!(
                "SELECT {COLUMNS}, {EXTRA}
                   FROM skills s
                   LEFT JOIN (SELECT cluster_id, COUNT(*) AS n FROM skills
                               WHERE cluster_id IS NOT NULL GROUP BY cluster_id) c
                          ON c.cluster_id = s.cluster_id
                  ORDER BY {order_by}
                  LIMIT ? OFFSET ?"
            ),
        };

        let mut statement = sqlx::query(&sql);
        if let Some(expr) = &match_expr {
            statement = statement.bind(expr);
        }
        let rows = statement
            .bind(limit)
            .bind(offset)
            .fetch_all(self.store.pool())
            .await?;

        Ok(rows.iter().map(row_to_hit).collect())
    }

    /// How many rows a search would return. `query` empty counts the whole shelf.
    pub async fn count(&self, query: &str) -> Result<i64, SkillsError> {
        let match_expr = fts_query(query);
        let rows: i64 = match &match_expr {
            Some(expr) => {
                sqlx::query_scalar(
                    "SELECT COUNT(*) FROM skills_fts
                       JOIN skills s ON s.rowid = skills_fts.rowid
                      WHERE skills_fts MATCH ?",
                )
                .bind(expr)
                .fetch_one(self.store.pool())
                .await?
            }
            None => {
                sqlx::query_scalar("SELECT COUNT(*) FROM skills")
                    .fetch_one(self.store.pool())
                    .await?
            }
        };
        Ok(rows)
    }

    pub async fn get(&self, id: &str) -> Result<Option<SkillDetail>, SkillsError> {
        let sql = format!(
            "SELECT {COLUMNS}, {EXTRA}, s.first_seen_at, s.capabilities_json
               FROM skills s
               LEFT JOIN (SELECT cluster_id, COUNT(*) AS n FROM skills
                           WHERE cluster_id IS NOT NULL GROUP BY cluster_id) c
                      ON c.cluster_id = s.cluster_id
              WHERE s.id = ?"
        );

        let Some(row) = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.store.pool())
            .await?
        else {
            return Ok(None);
        };

        let skill = row_to_hit(&row);
        let capabilities: Vec<Capability> = row
            .try_get::<Option<String>, _>("capabilities_json")
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let first_seen_at: Option<i64> = row.try_get("first_seen_at").ok();

        Ok(Some(SkillDetail {
            evidence: Evidence::for_skill(self.store.pool(), id).await?,
            variants: self.variants(id).await?,
            skill,
            capabilities,
            first_seen_at,
        }))
    }

    pub async fn variants(&self, id: &str) -> Result<Vec<VariantRow>, SkillsError> {
        // cluster_id, body_sha, simhash, license
        type OwnRow = (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let row: Option<OwnRow> = sqlx::query_as(
            "SELECT cluster_id, body_sha, simhash, license FROM skills WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.store.pool())
        .await?;

        let Some((Some(cluster_id), own_sha, own_simhash, own_license)) = row else {
            return Ok(Vec::new());
        };

        // id, name, repo, grade, license, last_seen_at, simhash, body_sha
        type SiblingRow = (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
        );
        let siblings: Vec<SiblingRow> = sqlx::query_as(
            "SELECT id, name, repo, grade, license, last_seen_at, simhash, body_sha
               FROM skills WHERE cluster_id = ? AND id <> ?
               ORDER BY name COLLATE NOCASE",
        )
        .bind(&cluster_id)
        .bind(id)
        .fetch_all(self.store.pool())
        .await?;

        Ok(siblings
            .into_iter()
            .map(
                |(sid, name, repo, grade, sibling_license, last_seen_at, simhash, sha)| {
                    let delta = describe_delta(
                        own_sha.as_deref(),
                        sha.as_deref(),
                        own_simhash.as_deref(),
                        simhash.as_deref(),
                        own_license.as_deref(),
                        sibling_license.as_deref(),
                    );

                    VariantRow {
                        id: sid,
                        name,
                        repo,
                        grade,
                        license: sibling_license,
                        last_seen_at,
                        delta,
                    }
                },
            )
            .collect())
    }

    /// Recompute cluster ids. Cheap enough to run after every ingest and honest enough to be
    /// the only way cluster ids change: they are derived, never assigned by a source.
    pub async fn recluster(&self) -> Result<usize, SkillsError> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, simhash FROM skills WHERE simhash IS NOT NULL")
                .fetch_all(self.store.pool())
                .await?;

        let pairs: Vec<(String, u64)> = rows
            .iter()
            .filter_map(|(id, hex)| {
                u64::from_str_radix(hex, 16)
                    .ok()
                    .map(|value| (id.clone(), value))
            })
            .collect();

        let assigned = cluster::assign(&pairs);

        let mut tx = self.store.pool().begin().await?;
        for (id, cluster_id) in &assigned {
            sqlx::query(
                "UPDATE skills SET cluster_id = ?
                  WHERE id = ? AND (cluster_id IS NULL OR cluster_id <> ?)",
            )
            .bind(cluster_id)
            .bind(id)
            .bind(cluster_id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "UPDATE skills SET cluster_id = NULL WHERE simhash IS NULL AND cluster_id IS NOT NULL",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let distinct: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT cluster_id) FROM skills WHERE cluster_id IS NOT NULL",
        )
        .fetch_one(self.store.pool())
        .await?;
        Ok(distinct as usize)
    }

    pub async fn body(&self, id: &str) -> Result<Option<(String, String)>, SkillsError> {
        let sha: Option<Option<String>> =
            sqlx::query_scalar::<_, Option<String>>("SELECT body_sha FROM skills WHERE id = ?")
                .bind(id)
                .fetch_optional(self.store.pool())
                .await?;

        let Some(Some(sha)) = sha else {
            return Ok(None);
        };
        let Some(text) = self.blobs.get_text(&sha)? else {
            return Ok(None);
        };
        Ok(Some((text, sha)))
    }

    pub async fn status(&self) -> Result<Vec<SourceStatus>, SkillsError> {
        let rows: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT id, base_url FROM sources ORDER BY id")
                .fetch_all(self.store.pool())
                .await?;

        let mut statuses = Vec::with_capacity(rows.len());
        for (id, base_url) in rows {
            statuses.push(load_status(self.store.pool(), &id, base_url.as_deref()).await?);
        }
        Ok(statuses)
    }

    pub async fn stats(&self) -> Result<IndexStats, SkillsError> {
        let skills: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM skills")
            .fetch_one(self.store.pool())
            .await?;
        let with_body: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM skills WHERE body_sha IS NOT NULL")
                .fetch_one(self.store.pool())
                .await?;
        let clusters: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT cluster_id) FROM skills WHERE cluster_id IS NOT NULL",
        )
        .fetch_one(self.store.pool())
        .await?;

        let mut files = 0_usize;
        let mut bytes = 0_u64;
        if let Ok(entries) = std::fs::read_dir(self.blobs.root()) {
            for entry in entries.flatten() {
                match entry.metadata() {
                    Ok(metadata) if metadata.is_file() => {
                        files += 1;
                        bytes += metadata.len();
                    }
                    _ => {}
                }
            }
        }

        Ok(IndexStats {
            skills,
            with_body,
            clusters,
            blobs: files,
            blob_bytes: bytes,
            sources: self.status().await?,
        })
    }

    /// Live blob hashes, for `rigel gc`.
    ///
    /// Cached HTTP bodies count as live: the tree and repo metadata blobs are what makes a
    /// re-index inside the freshness window free, and collecting them would turn every
    /// re-index into a full download.
    pub async fn live_blobs(&self) -> Result<HashSet<String>, SkillsError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT body_sha FROM skills WHERE body_sha IS NOT NULL
             UNION
             SELECT sha256 FROM http_cache WHERE sha256 IS NOT NULL",
        )
        .fetch_all(self.store.pool())
        .await?;
        Ok(rows.into_iter().map(|(sha,)| sha).collect())
    }

    /// P2.9: take the prototype's `skilldb:entries:v1` rows and put them in the index.
    ///
    /// They arrive with no body — the prototype only ever kept metadata — so they are written
    /// as `local:` rows with their original grade and savedAt, and the shelf shows them as
    /// unscanned-but-known. `skipped` counts rows already present (same URL), `invalid` rows
    /// without a name. Clearing the browser keys is the export snippet's job, not ours: the
    /// app cannot reach another origin's localStorage.
    pub async fn import_prototype_entries(&self, json: &str) -> Result<ImportReport, SkillsError> {
        let entries: Vec<PrototypeEntry> = serde_json::from_str(json)
            .map_err(|error| SkillsError::Other(format!("not a skilldb export: {error}")))?;

        let mut imported = 0;
        let mut skipped = 0;
        let mut invalid = 0;

        for entry in entries {
            let name = entry.name.trim().to_string();
            if name.is_empty() {
                invalid += 1;
                continue;
            }

            if let Some(url) = &entry.url {
                let exists: Option<String> =
                    sqlx::query_scalar("SELECT id FROM skills WHERE url = ? LIMIT 1")
                        .bind(url)
                        .fetch_optional(self.store.pool())
                        .await?;
                if exists.is_some() {
                    skipped += 1;
                    continue;
                }
            }

            let saved_at = entry.saved_at.unwrap_or_else(now_ms);
            let record = SkillRecord {
                id: format!("local:{}", entry.key),
                name,
                description: entry.description.chars().take(240).collect(),
                source: "local".to_string(),
                repo: entry.source.clone(),
                path: entry.path,
                url: entry.url,
                body_sha: None,
                grade: entry.grade,
                grade_scope: Some("prototype".to_string()),
                license: None,
                stars: entry.stars,
                installs: entry.installs.and_then(|value| value.as_i64()),
                tokens: None,
                capabilities: Vec::new(),
                simhash: None,
            };

            self.write_skill(&record).await?;
            if entry.saved_at.is_some() {
                // Keep the prototype's timestamp: "last seen" should not reset just because
                // the row was migrated.
                sqlx::query("UPDATE skills SET first_seen_at = ?, last_seen_at = ? WHERE id = ?")
                    .bind(saved_at)
                    .bind(saved_at)
                    .bind(&record.id)
                    .execute(self.store.pool())
                    .await?;
            }
            imported += 1;
        }

        Ok(ImportReport {
            imported,
            skipped,
            invalid,
        })
    }
}

const COLUMNS: &str = "s.id, s.name, s.description, s.source, s.repo, s.path, s.url,
                       s.grade, s.grade_scope, s.stars, s.installs, s.tokens, s.cluster_id,
                       s.last_seen_at";

/// `unmeasured` is per metric, not per skill: a third-party benchmark can cover one claim
/// while the description's other numbers stay unverified.
const EXTRA: &str = "COALESCE(c.n, 1) AS variants,
                     EXISTS(SELECT 1 FROM evidence e
                             WHERE e.skill_id = s.id AND e.measured IS NULL
                               AND e.advertised IS NOT NULL
                               AND NOT EXISTS (SELECT 1 FROM evidence m
                                                WHERE m.skill_id = e.skill_id
                                                  AND m.metric = e.metric
                                                  AND m.measured IS NOT NULL)) AS unmeasured,
                     (s.body_sha IS NOT NULL) AS has_body";

/// Nullable columns are decoded as `Option<T>`, never as `T` with `.ok()`. Decoding a NULL
/// into `i64` succeeds with 0 in sqlx's SQLite backend, which would report a repository with
/// no star count as having zero stars instead of "unknown".
fn row_to_hit(row: &sqlx::sqlite::SqliteRow) -> SearchHit {
    SearchHit {
        id: row.get("id"),
        name: row.get("name"),
        description: row
            .try_get::<Option<String>, _>("description")
            .unwrap_or(None)
            .unwrap_or_default(),
        source: row.get("source"),
        repo: row.try_get::<Option<String>, _>("repo").unwrap_or(None),
        path: row.try_get::<Option<String>, _>("path").unwrap_or(None),
        url: row.try_get::<Option<String>, _>("url").unwrap_or(None),
        grade: row.try_get::<Option<String>, _>("grade").unwrap_or(None),
        grade_scope: row
            .try_get::<Option<String>, _>("grade_scope")
            .unwrap_or(None),
        stars: row.try_get::<Option<i64>, _>("stars").unwrap_or(None),
        installs: row.try_get::<Option<i64>, _>("installs").unwrap_or(None),
        tokens: row.try_get::<Option<i64>, _>("tokens").unwrap_or(None),
        cluster_id: row
            .try_get::<Option<String>, _>("cluster_id")
            .unwrap_or(None),
        variants: row.try_get::<i64, _>("variants").unwrap_or(1),
        last_seen_at: row
            .try_get::<Option<i64>, _>("last_seen_at")
            .unwrap_or(None),
        unmeasured: row.try_get::<i64, _>("unmeasured").unwrap_or(0) != 0,
        has_body: row.try_get::<i64, _>("has_body").unwrap_or(0) != 0,
    }
}

fn describe_delta(
    own_sha: Option<&str>,
    sibling_sha: Option<&str>,
    own_simhash: Option<&str>,
    sibling_simhash: Option<&str>,
    own_license: Option<&str>,
    sibling_license: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    if own_sha.is_some() && own_sha == sibling_sha {
        parts.push("identical content".to_string());
    } else {
        match (
            own_simhash.and_then(parse_hex),
            sibling_simhash.and_then(parse_hex),
        ) {
            (Some(a), Some(b)) => {
                parts.push(format!("{} bits from this one", cluster::hamming(a, b)))
            }
            _ => parts.push("not fingerprinted".to_string()),
        }
    }

    match (own_license.is_some(), sibling_license.is_some()) {
        (true, false) => parts.push("no license".to_string()),
        (false, true) => parts.push("no license here".to_string()),
        _ => {}
    }

    parts.join(" · ")
}

fn parse_hex(value: &str) -> Option<u64> {
    u64::from_str_radix(value, 16).ok()
}

#[derive(Debug, Deserialize)]
struct PrototypeEntry {
    key: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(rename = "source", default)]
    source: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    grade: Option<String>,
    #[serde(default)]
    stars: Option<i64>,
    #[serde(default)]
    installs: Option<serde_json::Value>,
    #[serde(default)]
    path: Option<String>,
    #[serde(rename = "savedAt", default)]
    saved_at: Option<i64>,
}

//! One HTTP client for every outbound index request, with an ETag cache behind it.
//!
//! Three properties matter and each comes from a specific failure the prototype had:
//!
//! * **Bodies are content-addressed.** A response body is hashed into [`BlobStore`] and the
//!   URL row keeps only the hash, so re-indexing the same repo rewrites nothing.
//! * **Revalidation is conditional.** A cached URL is re-fetched with `If-None-Match` (and
//!   `If-Modified-Since`); a `304` costs no body bytes.
//! * **Freshness is a window, not a guess.** Within `ttl`, a cached URL is not contacted at
//!   all. That is what makes `rigel skills index` on an unchanged repo cost **zero** requests
//!   rather than one conditional request per file (P2.11), and it is the honest reason the
//!   shelf still works with the network off.
//!
//! Nothing here retries silently. A rate limit surfaces with its reset time because the
//! unauthenticated GitHub budget is 60/hour and a user who hits it needs to know why.

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, IF_MODIFIED_SINCE, IF_NONE_MATCH};
use sqlx::SqlitePool;

use super::blob::BlobStore;
use crate::migrate::now_ms;

const USER_AGENT_VALUE: &str = "Rigel/0.1 (+https://github.com/; local skill index)";

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("could not reach {url}: {reason}")]
    Network { url: String, reason: String },

    #[error("{url} responded {status}")]
    Status { url: String, status: u16 },

    #[error("{url} is rate limited")]
    RateLimited { url: String, reset_at: Option<i64> },

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("blob store error: {0}")]
    Io(#[from] io::Error),

    #[error("invalid JSON from {url}: {source}")]
    Json {
        url: String,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Clone)]
pub struct Fetched {
    pub text: String,
    pub status: u16,
    /// True when the body came from the local cache, whether because it was fresh or
    /// because the server answered `304`.
    pub from_cache: bool,
}

struct CacheRow {
    etag: Option<String>,
    last_modified: Option<String>,
    sha256: Option<String>,
    status: Option<i64>,
    checked_at: i64,
}

#[derive(Clone)]
pub struct Http {
    client: reqwest::Client,
    pool: SqlitePool,
    blobs: BlobStore,
    ttl_ms: i64,
    requests: std::sync::Arc<AtomicUsize>,
}

impl Http {
    pub fn new(pool: SqlitePool, blobs: BlobStore, ttl: Duration) -> Result<Self, HttpError> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT_VALUE)
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .build()
            .map_err(|error| HttpError::Network {
                url: "<client>".to_string(),
                reason: error.to_string(),
            })?;

        Ok(Self {
            client,
            pool,
            blobs,
            ttl_ms: ttl.as_millis() as i64,
            requests: std::sync::Arc::new(AtomicUsize::new(0)),
        })
    }

    /// Requests actually put on the wire, including conditional ones. The exit test asserts
    /// this is `0` for a re-index inside the freshness window.
    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::Relaxed)
    }

    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    /// Never contact the network; answer only from what is already cached. Used by
    /// `rigel skills search` paths and by the shelf's offline mode.
    pub async fn cached_text(&self, url: &str) -> Result<Option<Fetched>, HttpError> {
        let Some(row) = self.cache_row(url).await? else {
            return Ok(None);
        };
        let Some(sha) = row.sha256 else {
            return Ok(None);
        };
        let Some(text) = self.blobs.get_text(&sha)? else {
            return Ok(None);
        };
        Ok(Some(Fetched {
            text,
            status: row.status.unwrap_or(200) as u16,
            from_cache: true,
        }))
    }

    pub async fn get_text(&self, url: &str) -> Result<Fetched, HttpError> {
        self.get_text_with(url, "text/plain", false).await
    }

    /// Skip the freshness window but still revalidate conditionally. This is what `--force`
    /// means: "ask the network", not "download everything again".
    pub async fn get_text_fresh(&self, url: &str) -> Result<Fetched, HttpError> {
        self.get_text_with(url, "text/plain", true).await
    }

    pub async fn get_text_with(
        &self,
        url: &str,
        accept: &str,
        force: bool,
    ) -> Result<Fetched, HttpError> {
        // Two attempts at most. The second one is the recovery path for a cache row whose
        // blob is gone: `304` is useless when there is nothing to serve, so the validators are
        // dropped and the file is fetched properly. Without this, losing a blob turned every
        // later re-index of that repository into a hard error.
        let mut conditional = true;

        loop {
            let cached = if conditional {
                self.cache_row(url).await?
            } else {
                None
            };

            if let Some(row) = &cached {
                let fresh = !force && self.ttl_ms > 0 && row.checked_at + self.ttl_ms > now_ms();
                if fresh {
                    if let Some(sha) = &row.sha256 {
                        if let Some(text) = self.blobs.get_text(sha)? {
                            return Ok(Fetched {
                                text,
                                status: row.status.unwrap_or(200) as u16,
                                from_cache: true,
                            });
                        }
                    }
                }
            }

            let mut request = self.client.get(url).header(ACCEPT, accept);
            if let Some(token) = github_token() {
                request = request.header(AUTHORIZATION, format!("Bearer {token}"));
            }
            if let Some(row) = &cached {
                if let Some(etag) = &row.etag {
                    request = request.header(IF_NONE_MATCH, etag);
                }
                if let Some(modified) = &row.last_modified {
                    request = request.header(IF_MODIFIED_SINCE, modified);
                }
            }

            self.requests.fetch_add(1, Ordering::Relaxed);
            let response = request.send().await.map_err(|error| HttpError::Network {
                url: url.to_string(),
                reason: error.to_string(),
            })?;
            let status = response.status();

            if status == reqwest::StatusCode::NOT_MODIFIED {
                if let Some(row) = &cached {
                    if let Some(sha) = &row.sha256 {
                        if let Some(text) = self.blobs.get_text(sha)? {
                            self.touch(url).await?;
                            return Ok(Fetched {
                                text,
                                status: 304,
                                from_cache: true,
                            });
                        }
                    }
                }

                if conditional {
                    sqlx::query("DELETE FROM http_cache WHERE url = ?")
                        .bind(url)
                        .execute(&self.pool)
                        .await?;
                    conditional = false;
                    continue;
                }

                return Err(HttpError::Status {
                    url: url.to_string(),
                    status: 304,
                });
            }

            if !status.is_success() {
                let reset_at = response
                    .headers()
                    .get("x-ratelimit-reset")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<i64>().ok())
                    .map(|seconds| seconds * 1000);
                if status == reqwest::StatusCode::FORBIDDEN
                    || status == reqwest::StatusCode::TOO_MANY_REQUESTS
                {
                    return Err(HttpError::RateLimited {
                        url: url.to_string(),
                        reset_at,
                    });
                }
                return Err(HttpError::Status {
                    url: url.to_string(),
                    status: status.as_u16(),
                });
            }

            let etag = header(&response, "etag");
            let last_modified = header(&response, "last-modified");
            let text = response.text().await.map_err(|error| HttpError::Network {
                url: url.to_string(),
                reason: error.to_string(),
            })?;

            let sha = self.blobs.put(text.as_bytes())?;
            self.store_cache(
                url,
                etag.as_deref(),
                last_modified.as_deref(),
                &sha,
                status.as_u16(),
            )
            .await?;

            return Ok(Fetched {
                text,
                status: status.as_u16(),
                from_cache: false,
            });
        }
    }

    pub async fn get_json(&self, url: &str, force: bool) -> Result<serde_json::Value, HttpError> {
        let fetched = self
            .get_text_with(
                url,
                "application/vnd.github+json, application/json;q=0.9",
                force,
            )
            .await?;
        let value = serde_json::from_str(&fetched.text).map_err(|source| HttpError::Json {
            url: url.to_string(),
            source,
        })?;
        Ok(value)
    }

    async fn cache_row(&self, url: &str) -> Result<Option<CacheRow>, HttpError> {
        // etag, last_modified, sha256, status, checked_at
        type Row = (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            i64,
        );
        let row: Option<Row> = sqlx::query_as(
            "SELECT etag, last_modified, sha256, status, checked_at FROM http_cache WHERE url = ?",
        )
        .bind(url)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(etag, last_modified, sha256, status, checked_at)| CacheRow {
                etag,
                last_modified,
                sha256,
                status,
                checked_at,
            },
        ))
    }

    async fn touch(&self, url: &str) -> Result<(), HttpError> {
        sqlx::query("UPDATE http_cache SET checked_at = ? WHERE url = ?")
            .bind(now_ms())
            .bind(url)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn store_cache(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
        sha: &str,
        status: u16,
    ) -> Result<(), HttpError> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO http_cache (url, etag, last_modified, sha256, status, fetched_at, checked_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(url) DO UPDATE SET
               etag = excluded.etag,
               last_modified = excluded.last_modified,
               sha256 = excluded.sha256,
               status = excluded.status,
               fetched_at = excluded.fetched_at,
               checked_at = excluded.checked_at",
        )
        .bind(url)
        .bind(etag)
        .bind(last_modified)
        .bind(sha)
        .bind(status as i64)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn header(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

/// GitHub allows 60 requests/hour unauthenticated. Rigel never ships a token, but a user who
/// has one in the environment gets their own 5,000/hour budget instead of ours.
pub fn github_token() -> Option<String> {
    std::env::var("RIGEL_GITHUB_TOKEN")
        .or_else(|_| std::env::var("GITHUB_TOKEN"))
        .ok()
        .filter(|token| !token.trim().is_empty())
}

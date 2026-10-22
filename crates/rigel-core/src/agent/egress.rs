//! Network egress with per-origin consent (D33).
//!
//! `fetch_url` is the only way the native agent reaches the network. A fetch is approved per
//! **origin** — scheme, host and port — not per URL, so one approval covers the pages of a site
//! without letting a redirect carry that consent somewhere else. The answer can cover one
//! request, the rest of the session, or every session on this machine; only the last one is
//! written to `<index dir>/approved-origins.json`.
//!
//! The fetcher itself is deliberately narrow: `http`/`https` only, redirects followed by hand so
//! each new origin is approved, redirects capped, a 256 KiB body cap, and a content-type
//! allowlist of text-like types. Everything it returns is untrusted input, labelled as such.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Url;
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION};
use serde::{Deserialize, Serialize};

/// Per response. The model only ever sees [`crate::agent::tools`]' result cap on top of this.
pub const MAX_FETCH_BYTES: usize = 256 * 1024;
const MAX_REDIRECTS: usize = 5;
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const APPROVED_FILE: &str = "approved-origins.json";

/// What is being approved: one URL, belonging to one origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginRequest {
    pub url: String,
    /// `scheme://host[:port]`, lowercased and without a trailing slash.
    pub origin: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginDecision {
    AllowOnce,
    /// Until this process exits. A CLI turn is one process.
    AllowSession,
    /// Persisted under the index dir, so it survives restarts.
    AllowAlways,
    Deny,
}

/// Who answers an [`OriginRequest`]. Implementations must fail closed: an error, EOF or an
/// unreadable answer denies.
#[async_trait]
pub trait OriginApprover: Send + Sync {
    async fn approve(&self, request: &OriginRequest) -> OriginDecision;
}

/// Refuses every origin. The default for a headless or not-yet-wired caller.
pub struct DenyOrigins;

#[async_trait]
impl OriginApprover for DenyOrigins {
    async fn approve(&self, _request: &OriginRequest) -> OriginDecision {
        OriginDecision::Deny
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressError {
    /// A stable code the tool returns: `BAD_URL`, `UNSUPPORTED_SCHEME`, `DENIED`, ...
    pub code: &'static str,
    pub message: String,
}

impl EgressError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// One fetched page. `text` is decoded lossily as UTF-8; `truncated` says the body cap cut it.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub url: String,
    pub origin: String,
    pub status: u16,
    pub content_type: String,
    pub bytes: usize,
    pub truncated: bool,
    pub text: String,
}

impl Fetched {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct StoredOrigins {
    #[serde(default)]
    origins: Vec<String>,
}

/// Per-process consent cache plus the persisted `always` list. Cloneable so a subagent's tool
/// set shares the same grants.
#[derive(Clone)]
pub struct Egress {
    client: reqwest::Client,
    approver: Arc<dyn OriginApprover>,
    store: std::path::PathBuf,
    session: Arc<Mutex<HashSet<String>>>,
    always: Arc<Mutex<Option<HashSet<String>>>>,
}

impl Egress {
    pub fn new(
        approver: Arc<dyn OriginApprover>,
        index_dir: impl Into<std::path::PathBuf>,
    ) -> Self {
        let client = reqwest::Client::builder()
            // Redirects are followed by hand: a 302 to another origin must not inherit the
            // first origin's approval.
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .user_agent("rigel/0.1")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let index_dir = index_dir.into();
        Self {
            client,
            approver,
            store: index_dir.join(APPROVED_FILE),
            session: Arc::new(Mutex::new(HashSet::new())),
            always: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_approver(mut self, approver: Arc<dyn OriginApprover>) -> Self {
        self.approver = approver;
        self
    }

    /// Fetch one `http(s)` URL after its origin is approved. The whole operation is bounded, so
    /// a server that trickles bytes cannot hold the turn open indefinitely.
    pub async fn fetch(&self, url: &str) -> Result<Fetched, EgressError> {
        let start = Url::parse(url.trim())
            .map_err(|error| EgressError::new("BAD_URL", format!("{url} is not a URL: {error}")))?;
        match start.scheme() {
            "http" | "https" => {}
            other => {
                return Err(EgressError::new(
                    "UNSUPPORTED_SCHEME",
                    format!("{other} is not allowed; fetch_url only uses http and https"),
                ));
            }
        }

        match tokio::time::timeout(FETCH_TIMEOUT, self.fetch_inner(start)).await {
            Ok(result) => result,
            Err(_) => Err(EgressError::new(
                "TIMEOUT",
                format!(
                    "the fetch did not finish within {}s",
                    FETCH_TIMEOUT.as_secs()
                ),
            )),
        }
    }

    async fn fetch_inner(&self, start: Url) -> Result<Fetched, EgressError> {
        let mut current = start.clone();
        // An `AllowOnce` holds for this fetch, including redirects inside the same origin.
        let mut once = HashSet::new();

        for _ in 0..=MAX_REDIRECTS {
            let origin = current.origin().ascii_serialization();
            if !once.contains(&origin) {
                self.authorize(&origin, current.as_str()).await?;
                once.insert(origin.clone());
            }

            let response = self
                .client
                .get(current.clone())
                .send()
                .await
                .map_err(|error| EgressError::new("NETWORK", format!("{origin}: {error}")))?;
            let status = response.status();

            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_string);
                let Some(location) = location else {
                    return Err(EgressError::new(
                        "BAD_REDIRECT",
                        format!("{origin} redirected with no Location header"),
                    ));
                };
                let next = current.join(&location).map_err(|error| {
                    EgressError::new(
                        "BAD_REDIRECT",
                        format!("{origin} redirected to {location}, which is not a URL: {error}"),
                    )
                })?;
                if !matches!(next.scheme(), "http" | "https") {
                    return Err(EgressError::new(
                        "UNSUPPORTED_SCHEME",
                        format!(
                            "{origin} redirected to {}, which is not http or https",
                            next
                        ),
                    ));
                }
                current = next;
                continue;
            }

            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            if !text_like(&content_type) {
                return Err(EgressError::new(
                    "UNSUPPORTED_TYPE",
                    format!(
                        "{origin} returned {:?}; fetch_url only reads text-like content",
                        if content_type.is_empty() {
                            "no content-type"
                        } else {
                            &content_type
                        }
                    ),
                ));
            }

            let declared = response
                .headers()
                .get(CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<usize>().ok());
            let mut truncated = declared.is_some_and(|length| length > MAX_FETCH_BYTES);
            let mut body = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk
                    .map_err(|error| EgressError::new("NETWORK", format!("{origin}: {error}")))?;
                if body.len() >= MAX_FETCH_BYTES {
                    truncated = true;
                    break;
                }
                let take = (MAX_FETCH_BYTES - body.len()).min(chunk.len());
                body.extend_from_slice(&chunk[..take]);
                if take < chunk.len() {
                    truncated = true;
                    break;
                }
            }

            let text = String::from_utf8_lossy(&body).to_string();
            return Ok(Fetched {
                url: current.to_string(),
                origin,
                status: status.as_u16(),
                content_type,
                bytes: body.len(),
                truncated,
                text,
            });
        }

        Err(EgressError::new(
            "TOO_MANY_REDIRECTS",
            format!("more than {MAX_REDIRECTS} redirects from {start}"),
        ))
    }

    async fn authorize(&self, origin: &str, url: &str) -> Result<(), EgressError> {
        if self.granted(origin) {
            return Ok(());
        }
        let decision = self
            .approver
            .approve(&OriginRequest {
                url: url.to_string(),
                origin: origin.to_string(),
            })
            .await;
        match decision {
            OriginDecision::AllowOnce => Ok(()),
            OriginDecision::AllowSession => {
                self.remember_session(origin);
                Ok(())
            }
            OriginDecision::AllowAlways => {
                self.remember_session(origin);
                self.remember_always(origin);
                Ok(())
            }
            OriginDecision::Deny => Err(EgressError::new(
                "DENIED",
                format!("the user did not approve fetching {origin}; nothing was sent"),
            )),
        }
    }

    fn granted(&self, origin: &str) -> bool {
        self.session
            .lock()
            .map(|session| session.contains(origin))
            .unwrap_or(false)
            || self.always_has(origin)
    }

    fn remember_session(&self, origin: &str) {
        if let Ok(mut session) = self.session.lock() {
            session.insert(origin.to_string());
        }
    }

    fn always_has(&self, origin: &str) -> bool {
        let mut always = match self.always.lock() {
            Ok(always) => always,
            Err(_) => return false,
        };
        if always.is_none() {
            *always = Some(load_always(&self.store));
        }
        always
            .as_ref()
            .is_some_and(|origins| origins.contains(origin))
    }

    fn remember_always(&self, origin: &str) {
        let mut always = match self.always.lock() {
            Ok(always) => always,
            Err(_) => return,
        };
        let origins = always.get_or_insert_with(|| load_always(&self.store));
        origins.insert(origin.to_string());
        save_always(&self.store, origins);
    }
}

fn load_always(path: &std::path::Path) -> HashSet<String> {
    // A missing or unreadable file is an empty list, never a panic and never a grant.
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<StoredOrigins>(&text).ok())
        .map(|stored| stored.origins.into_iter().collect())
        .unwrap_or_default()
}

fn save_always(path: &std::path::Path, origins: &HashSet<String>) {
    let mut sorted: Vec<&String> = origins.iter().collect();
    sorted.sort();
    let stored = StoredOrigins {
        origins: sorted.into_iter().cloned().collect(),
    };
    let Ok(text) = serde_json::to_string_pretty(&stored) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let temporary = path.with_extension("json.tmp");
    if std::fs::write(&temporary, text).is_err() {
        return;
    }
    // Windows cannot rename over an existing file; the window between remove and rename only
    // means the next process asks again.
    let _ = std::fs::remove_file(path);
    let _ = std::fs::rename(&temporary, path);
}

/// The content types the tool will read. Anything else is refused rather than decoded as text.
pub fn text_like(content_type: &str) -> bool {
    let base = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    base.starts_with("text/")
        || matches!(
            base.as_str(),
            "application/json"
                | "application/ld+json"
                | "application/xml"
                | "application/xhtml+xml"
                | "application/rss+xml"
                | "application/atom+xml"
                | "application/yaml"
                | "application/x-yaml"
                | "application/toml"
        )
        || base.ends_with("+json")
        || base.ends_with("+xml")
}

#[cfg(test)]
mod tests {
    use super::text_like;

    #[test]
    fn only_text_like_types_are_read() {
        for allowed in [
            "text/html; charset=utf-8",
            "text/plain",
            "application/json",
            "application/json; charset=utf-8",
            "application/ld+json",
            "application/xhtml+xml",
            "application/atom+xml",
            "application/problem+json",
            "application/soap+xml",
        ] {
            assert!(text_like(allowed), "{allowed} should be readable");
        }
        for refused in [
            "",
            "image/png",
            "application/pdf",
            "application/octet-stream",
            "video/mp4",
            "application/zip",
        ] {
            assert!(!text_like(refused), "{refused} should be refused");
        }
    }
}

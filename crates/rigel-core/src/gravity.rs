//! Gravity contextual ads, opt-in (https://trygravity.com).
//!
//! Gravity is not an inference provider: it auctions text ads against the recent conversation and
//! pays the publisher for impressions and clicks. The renderer never holds the API key and never
//! calls Gravity directly. The desktop and the website ask one Firebase broker for the same slot,
//! so the publisher key lives only in Secret Manager:
//!
//! * [`SharedGravity::get_ads`] — `POST /api/gravity-ad`, the broker both surfaces share.
//! * [`Gravity::get_ads`] — the direct engine call, kept for a signed direct integration.
//! * [`track`] — loads an impression URL once, when the card becomes visible.
//! * [`click_through`] — records the click and returns the landing page, which the window then
//!   opens in the system browser.
//!
//! The app's webview CSP allows neither third-party images nor third-party connections, so both
//! tracking calls must run in Rust. That is also the honest split: the key and the publisher's
//! signed tracking URLs stay out of the renderer entirely.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::credentials::{self, CredentialStore};

/// Overridable so a test (or a local proxy) needs no code change.
pub const BASE_ENV: &str = "RIGEL_GRAVITY_BASE_URL";
pub const DEFAULT_BASE: &str = "https://server.trygravity.com";
/// The shared broker. Override for a local Functions emulator or a self-hosted deployment.
pub const ENDPOINT_ENV: &str = "RIGEL_GRAVITY_ENDPOINT";
pub const DEFAULT_ENDPOINT: &str = "https://rigel-25128.web.app/api/gravity-ad";
/// The slot Rigel shows on the paper. Registered in the Gravity dashboard under this id.
pub const PLACEMENT: &str = "below_response";
pub const PLACEMENT_ID: &str = "rigel-sponsor";
/// A stable anonymous install id, so frequency caps and reports work without an account.
pub const INSTALL_FILE: &str = "gravity-user-id";

const USER_AGENT: &str = "Rigel (desktop; Gravity contextual ads)";

#[derive(Debug, thiserror::Error)]
pub enum GravityError {
    #[error("no Gravity API key is stored")]
    MissingKey,

    #[error("Gravity request is incomplete: {0}")]
    Invalid(String),

    #[error("Gravity did not answer: {0}")]
    Offline(String),

    #[error("Gravity refused the request ({status}): {message}")]
    Refused { status: u16, message: String },

    #[error("Gravity returned unreadable data: {0}")]
    Unreadable(String),

    #[error("the tracking URL is not a Gravity URL: {0}")]
    UntrustedUrl(String),

    #[error("the click did not resolve to a landing page")]
    NoLandingPage,
}

/// One message of the recent conversation. Gravity matches the ad against these, and the SDKs
/// send the last two; Rigel sends the same window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    /// The end user's browser or app, not Rigel's HTTP client.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ua: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ip: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub os: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub locale: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    pub id: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub tags: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Placement {
    pub placement: String,
    pub placement_id: String,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            placement: PLACEMENT.to_string(),
            placement_id: PLACEMENT_ID.to_string(),
        }
    }
}

/// The exact request body the ad engine accepts. Field names follow its JSON, not Rust.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdRequest {
    pub messages: Vec<Message>,
    pub session_id: String,
    pub placements: Vec<Placement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<Device>,
    /// Test ads are not billed and are not counted in reports.
    #[serde(default)]
    pub test_ad: bool,
    /// With `test_ad`, return a sample ad instead of `204 No Content`, so the UI can be built.
    #[serde(default)]
    pub always_return: bool,
}

impl AdRequest {
    pub fn new(
        session_id: impl Into<String>,
        messages: Vec<Message>,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            messages,
            session_id: session_id.into(),
            placements: vec![Placement::default()],
            user: Some(User {
                id: user_id.into(),
                tags: [("plan".to_string(), "free".to_string())]
                    .into_iter()
                    .collect(),
            }),
            device: None,
            test_ad: false,
            always_return: false,
        }
    }

    pub fn with_device(mut self, device: Device) -> Self {
        self.device = Some(device);
        self
    }

    pub fn as_test(mut self) -> Self {
        self.test_ad = true;
        self.always_return = true;
        self
    }
}

/// One ad, as the engine returns it. `click_url` is the tracked link; `url` is the raw landing
/// page and must not be used for a link.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ad {
    #[serde(default)]
    pub ad_text: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub brand_name: String,
    #[serde(default)]
    pub cta: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub favicon: String,
    #[serde(default)]
    pub brand_image: String,
    #[serde(default)]
    pub click_url: String,
    /// Gravity spells this `impUrl`; the explicit rename keeps the odd capitalization honest.
    #[serde(default, rename = "impUrl")]
    pub imp_url: String,
    #[serde(default)]
    pub placement: String,
    /// The engine spells this `placement_id`, unlike the camelCase fields around it.
    #[serde(default, rename = "placement_id")]
    pub placement_id: String,
}

impl Ad {
    /// The ad has enough to draw a card. A row with no text at all is not shown.
    pub fn drawable(&self) -> bool {
        !(self.ad_text.trim().is_empty() && self.title.trim().is_empty())
    }

    /// The impression URL, with the usual capitalization. Kept as an explicit accessor so a
    /// typo in a field name cannot silently disable tracking.
    pub fn impression_url(&self) -> &str {
        &self.imp_url
    }
}

/// The base URL, overridable for tests and a proxy.
pub fn base_url() -> String {
    std::env::var(BASE_ENV)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE.to_string())
}

/// The broker both surfaces call. `RIGEL_GRAVITY_ENDPOINT` exists for a local emulator.
pub fn shared_endpoint() -> String {
    std::env::var(ENDPOINT_ENV)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string())
}

/// A stable anonymous user id for this installation. Written once into the index directory;
/// when the directory is not writable the process still gets an id for this run.
pub fn install_id(dir: &Path) -> String {
    let path = dir.join(INSTALL_FILE);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let value = existing.trim();
        if !value.is_empty() && value.len() <= 128 {
            return value.to_string();
        }
    }
    let value = uuid::Uuid::new_v4().to_string();
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(&path, &value);
    value
}

/// The publisher client. One instance per call is cheap: the HTTP client is the only state.
pub struct Gravity {
    key: String,
    base: String,
    http: reqwest::Client,
}

impl Gravity {
    pub fn new(key: impl Into<String>, base: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(6))
            .connect_timeout(Duration::from_secs(3))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_default();
        Self {
            key: key.into(),
            base: base.into().trim_end_matches('/').to_string(),
            http,
        }
    }

    /// The stored key, or `None` when the integration is off/no key is stored. No network call
    /// happens for the `None` case: an unconfigured Rigel must be silent, not fail slowly.
    pub fn from_store(store: &dyn CredentialStore) -> Option<Self> {
        let key = credentials::get_from(store, credentials::GRAVITY).ok()?;
        let value = key.expose().trim();
        if value.is_empty() {
            return None;
        }
        Some(Self::new(value, base_url()))
    }

    /// Request ads. `204 No Content` is an ordinary "no ad matched", not an error, and comes
    /// back as an empty vector. A `401`/`403`/`429` is reported with its status so Settings can
    /// tell the user whether the key or the publisher approval is at fault.
    pub async fn get_ads(&self, request: &AdRequest) -> Result<Vec<Ad>, GravityError> {
        validate_request(request)?;

        let response = self
            .http
            .post(format!("{}/api/v1/ad", self.base))
            .bearer_auth(&self.key)
            .json(request)
            .send()
            .await
            .map_err(|error| GravityError::Offline(error.to_string()))?;

        let status = response.status();
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(Vec::new());
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(GravityError::Refused {
                status: status.as_u16(),
                message: brief(&body),
            });
        }
        let ads: Vec<Ad> = response
            .json()
            .await
            .map_err(|error| GravityError::Unreadable(error.to_string()))?;
        Ok(ads.into_iter().filter(Ad::drawable).collect())
    }
}

/// One answer from the shared broker. `test` is true while the publisher is still in review, so
/// the card can say so and the desktop switch can hide non-billed samples.
#[derive(Debug, Clone, Default)]
pub struct SharedAds {
    pub ads: Vec<Ad>,
    pub test: bool,
}

#[derive(Debug, Deserialize)]
struct SharedResponse {
    #[serde(default)]
    ads: Vec<Ad>,
    #[serde(default)]
    test: bool,
}

/// The Firebase-hosted broker. It holds the publisher key; this client holds only its URL.
pub struct SharedGravity {
    endpoint: String,
    http: reqwest::Client,
}

impl SharedGravity {
    pub fn new(endpoint: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .connect_timeout(Duration::from_secs(3))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_default();
        Self {
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            http,
        }
    }

    pub fn configured() -> Self {
        Self::new(shared_endpoint())
    }

    /// Ask the broker for one ad. `204` and a no-fill both mean an empty list; an unreachable or
    /// misconfigured broker is an error the UI can hide without pretending an ad was served.
    pub async fn get_ads(&self, request: &AdRequest) -> Result<SharedAds, GravityError> {
        validate_request(request)?;
        if !trusted_endpoint(&self.endpoint) {
            return Err(GravityError::Invalid(format!(
                "the shared endpoint is not https: {}",
                self.endpoint
            )));
        }

        let response = self
            .http
            .post(&self.endpoint)
            .json(request)
            .send()
            .await
            .map_err(|error| GravityError::Offline(error.to_string()))?;

        let status = response.status();
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(SharedAds::default());
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(GravityError::Refused {
                status: status.as_u16(),
                message: brief(&body),
            });
        }
        let answer: SharedResponse = response
            .json()
            .await
            .map_err(|error| GravityError::Unreadable(error.to_string()))?;
        Ok(SharedAds {
            ads: answer
                .ads
                .into_iter()
                .filter(|ad| ad.drawable() && is_https(&ad.click_url))
                .collect(),
            test: answer.test,
        })
    }
}

fn validate_request(request: &AdRequest) -> Result<(), GravityError> {
    if request.messages.is_empty() {
        return Err(GravityError::Invalid(
            "no conversation messages".to_string(),
        ));
    }
    if request.session_id.trim().is_empty() {
        return Err(GravityError::Invalid("no session id".to_string()));
    }
    if request.placements.is_empty() || request.placements.len() > 10 {
        return Err(GravityError::Invalid(
            "one to ten placements are required".to_string(),
        ));
    }
    Ok(())
}

fn is_https(value: &str) -> bool {
    reqwest::Url::parse(value.trim())
        .map(|url| url.scheme() == "https")
        .unwrap_or(false)
}

/// https anywhere, or plain http only on this machine for a Functions emulator.
fn trusted_endpoint(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value.trim()) else {
        return false;
    };
    if url.scheme() == "https" {
        return true;
    }
    if url.scheme() != "http" {
        return false;
    }
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

fn brief(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "no body".to_string();
    }
    let clipped: String = trimmed.chars().take(240).collect();
    if trimmed.chars().count() > 240 {
        format!("{clipped}…")
    } else {
        clipped
    }
}

/// Whether a URL is one of Gravity's signed tracking endpoints. Anything else is refused before
/// it reaches the network, so a compromised reply cannot turn the app into an open proxy.
pub fn trusted_tracking_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value.trim()) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    host == "trygravity.ai"
        || host.ends_with(".trygravity.ai")
        || host == "trygravity.com"
        || host.ends_with(".trygravity.com")
}

/// Load an impression URL once. A `3xx` counts as delivered: Gravity's tracking host may
/// redirect a pixel to a CDN, and the impression has already been recorded by then.
pub async fn track(url: &str) -> Result<(), GravityError> {
    if !trusted_tracking_url(url) {
        return Err(GravityError::UntrustedUrl(url.to_string()));
    }
    let http = tracking_client();
    let response = http
        .get(url.trim())
        .send()
        .await
        .map_err(|error| GravityError::Offline(error.to_string()))?;
    let status = response.status();
    if status.is_success() || status.is_redirection() {
        Ok(())
    } else {
        Err(GravityError::Refused {
            status: status.as_u16(),
            message: "the impression URL was refused".to_string(),
        })
    }
}

/// Record a click and return the advertiser's landing page. Gravity answers with a `302`; the
/// webview cannot follow a cross-origin navigation safely, so the window opens this URL in the
/// system browser after Rust has recorded the click.
pub async fn click_through(url: &str) -> Result<String, GravityError> {
    if !trusted_tracking_url(url) {
        return Err(GravityError::UntrustedUrl(url.to_string()));
    }
    let http = tracking_client();
    let response = http
        .get(url.trim())
        .send()
        .await
        .map_err(|error| GravityError::Offline(error.to_string()))?;
    if let Some(location) = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
    {
        let landing = reqwest::Url::parse(location).map_err(|_| GravityError::NoLandingPage)?;
        if matches!(landing.scheme(), "http" | "https") {
            return Ok(landing.to_string());
        }
        return Err(GravityError::NoLandingPage);
    }
    // Some test fills and a few direct campaigns answer 200 with the landing page itself rather
    // than a 302. Opening the trusted click URL is the honest result; the click was recorded.
    if response.status().is_success() && is_https(url) {
        return Ok(url.trim().to_string());
    }
    Err(GravityError::NoLandingPage)
}

fn tracking_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .connect_timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(USER_AGENT)
        .build()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::credentials::MemoryCredentialStore;

    /// One HTTP request, read fully, answered with `status` and `body`.
    async fn serve(status: &'static str, body: &'static str) -> (String, Arc<Mutex<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(String::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if let Some(end) = find_headers_end(&request) {
                    let head = String::from_utf8_lossy(&request[..end]).to_string();
                    let length = content_length(&head).unwrap_or(0);
                    if request.len() >= end + length {
                        break;
                    }
                }
            }
            *log.lock().unwrap() = String::from_utf8_lossy(&request).to_string();
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(header.as_bytes()).await;
            let _ = socket.write_all(body.as_bytes()).await;
            let _ = socket.shutdown().await;
        });
        (format!("http://{address}"), seen)
    }

    fn find_headers_end(request: &[u8]) -> Option<usize> {
        request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|position| position + 4)
    }

    fn content_length(head: &str) -> Option<usize> {
        head.lines()
            .find_map(|line| {
                line.strip_prefix("content-length: ")
                    .or_else(|| line.strip_prefix("Content-Length: "))
            })
            .and_then(|value| value.trim().parse().ok())
    }

    fn request() -> AdRequest {
        AdRequest::new(
            "sess-1",
            vec![
                Message::new("user", "How do I deploy a serverless Postgres DB?"),
                Message::new("assistant", "Start with a managed Postgres…"),
            ],
            "install-7",
        )
        .with_device(Device {
            ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)".to_string(),
            ip: "203.0.113.42".to_string(),
            ..Device::default()
        })
    }

    #[test]
    fn no_key_means_no_client_and_no_request() {
        let store = MemoryCredentialStore::new();
        assert!(Gravity::from_store(&store).is_none());
        let store = MemoryCredentialStore::with(credentials::GRAVITY, "   ");
        assert!(Gravity::from_store(&store).is_none());
        let store = MemoryCredentialStore::with(credentials::GRAVITY, "gr-test");
        assert!(Gravity::from_store(&store).is_some());
    }

    #[test]
    fn install_id_is_stable_and_written_once() {
        let dir = tempfile::tempdir().unwrap();
        let first = install_id(dir.path());
        let second = install_id(dir.path());
        assert_eq!(first, second);
        assert!(!first.is_empty());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(INSTALL_FILE))
                .unwrap()
                .trim(),
            first
        );
    }

    #[test]
    fn only_gravity_hosts_are_tracked() {
        assert!(trusted_tracking_url("https://api.trygravity.ai/ack?p=1"));
        assert!(trusted_tracking_url(
            "https://server.trygravity.com/track/click?p=1"
        ));
        assert!(!trusted_tracking_url("https://evil.example/ack"));
        assert!(!trusted_tracking_url("http://api.trygravity.ai/ack"));
        assert!(!trusted_tracking_url("not a url"));
    }

    #[tokio::test]
    async fn a_200_returns_the_ad_and_sends_the_documented_body() {
        let (origin, seen) = serve(
            "200 OK",
            r#"[{"adText":"Serverless Postgres that scales to zero.","title":"Neon","brandName":"Neon","cta":"Try Neon Free","url":"https://neon.tech","clickUrl":"https://api.trygravity.ai/track/click?p=abc","impUrl":"https://api.trygravity.ai/ack?p=abc","placement":"below_response","placement_id":"rigel-sponsor"}]"#,
        )
        .await;
        let gravity = Gravity::new("gr-test", origin);
        let ads = gravity.get_ads(&request()).await.unwrap();
        assert_eq!(ads.len(), 1);
        assert_eq!(ads[0].brand_name, "Neon");
        assert_eq!(
            ads[0].click_url,
            "https://api.trygravity.ai/track/click?p=abc"
        );
        assert_eq!(
            ads[0].impression_url(),
            "https://api.trygravity.ai/ack?p=abc"
        );

        let request = seen.lock().unwrap().clone();
        assert!(request.starts_with("POST /api/v1/ad HTTP/1.1"), "{request}");
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer gr-test")
        );
        assert!(request.contains("\"sessionId\":\"sess-1\""), "{request}");
        assert!(
            request.contains("\"placement\":\"below_response\""),
            "{request}"
        );
        assert!(
            request.contains("\"placement_id\":\"rigel-sponsor\""),
            "{request}"
        );
        assert_eq!(ads[0].placement_id, "rigel-sponsor");
        assert!(
            request.contains("\"user\":{\"id\":\"install-7\""),
            "{request}"
        );
        assert!(request.contains("\"ip\":\"203.0.113.42\""), "{request}");
    }

    #[tokio::test]
    async fn the_shared_broker_returns_test_ads_without_sending_a_key() {
        let (origin, seen) = serve(
            "200 OK",
            r#"{"ads":[{"adText":"Ship a database this afternoon.","title":"Neon","brandName":"Neon","cta":"Try Neon Free","clickUrl":"https://api.trygravity.ai/track/click?p=abc","impUrl":"https://api.trygravity.ai/ack?p=abc","placement":"below_response","placement_id":"rigel-sponsor"}],"test":true}"#,
        )
        .await;
        let broker = SharedGravity::new(format!("{origin}/api/gravity-ad"));
        let answer = broker.get_ads(&request()).await.unwrap();
        assert!(answer.test);
        assert_eq!(answer.ads.len(), 1);
        assert_eq!(answer.ads[0].placement_id, "rigel-sponsor");

        let sent = seen.lock().unwrap().clone();
        assert!(sent.starts_with("POST /api/gravity-ad HTTP/1.1"), "{sent}");
        assert!(
            !sent.to_ascii_lowercase().contains("authorization:"),
            "{sent}"
        );
        assert!(
            sent.contains("\"placement_id\":\"rigel-sponsor\""),
            "{sent}"
        );
    }

    #[tokio::test]
    async fn a_shared_broker_error_is_reported_with_its_status() {
        let (origin, _) = serve("503 Service Unavailable", r#"{"error":"ads_unavailable"}"#).await;
        let error = SharedGravity::new(format!("{origin}/api/gravity-ad"))
            .get_ads(&request())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("503"), "{error}");
    }

    #[tokio::test]
    async fn a_204_is_no_ad_rather_than_a_failure() {
        let (origin, _) = serve("204 No Content", "").await;
        let ads = Gravity::new("gr-test", origin)
            .get_ads(&request())
            .await
            .unwrap();
        assert!(ads.is_empty());
    }

    #[tokio::test]
    async fn a_403_says_the_publisher_is_not_approved() {
        let (origin, _) = serve("403 Forbidden", r#"{"error":"publisher_not_approved"}"#).await;
        let error = Gravity::new("gr-test", origin)
            .get_ads(&request())
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("403"), "{message}");
        assert!(message.contains("publisher_not_approved"), "{message}");
    }

    #[tokio::test]
    async fn an_incomplete_request_never_reaches_the_network() {
        let mut incomplete = request();
        incomplete.messages.clear();
        let error = Gravity::new("gr-test", "http://127.0.0.1:1")
            .get_ads(&incomplete)
            .await
            .unwrap_err();
        assert!(matches!(error, GravityError::Invalid(_)));
    }

    #[tokio::test]
    async fn a_click_resolves_to_the_landing_page() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = [0_u8; 4096];
            let _ = socket.read(&mut buffer).await;
            let _ = socket
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: https://example.com/pricing\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
        });
        // The tracking-url guard is host-based, so a loopback server cannot be used here; a
        // click on anything but Gravity is refused before any socket is opened.
        let error = click_through(&format!("http://{address}/track"))
            .await
            .unwrap_err();
        assert!(matches!(error, GravityError::UntrustedUrl(_)));
    }
}

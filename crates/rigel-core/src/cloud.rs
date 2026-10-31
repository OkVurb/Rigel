//! Rigel Cloud: the user's own Firebase project, and nothing else.
//!
//! The app is offline by default. This module is the one place that can make it online, and it
//! only does so when the user configures a project and signs in. There is no Rigel-operated
//! server, no telemetry and no anonymous identifier: the project id and web API key name a
//! Firebase project **the user owns**, and every request carries the user's own ID token.
//!
//! The pieces:
//!
//! * [`CloudConfig`] — project id and web API key, read from `<data dir>/cloud.json` or the pair
//!   `RIGEL_FIREBASE_PROJECT_ID` / `RIGEL_FIREBASE_API_KEY`. The web API key is public by design;
//!   the Firestore rules, not the key, decide who may read a document.
//! * [`Cloud::sign_in`] — Firebase Authentication's email/password endpoint. The password is used
//!   once and never stored; the refresh token goes to the OS keychain under
//!   `rigel/firebase-session`, and the ID token is refreshed on demand.
//! * [`Cloud::push`] — writes the local shelf's metadata to
//!   `users/{uid}/skills/{document}`, one document per skill, batched through Firestore's
//!   `documents:commit`. Bodies are **not** uploaded; the blob store stays local.
//! * [`Cloud::pull`] — lists those documents and upserts any row the local index does not have.
//!   An existing body is never replaced (`write_skill` keeps the local one).
//! * [`Cloud::publish_session`] — writes one session's header and its turns to
//!   `users/{uid}/sessions/{id}/turns`, for the iPhone client. Only the prompt and the assistant's
//!   own words are sent; tool calls, command output, file changes and the project path stay local.
//! * App Check — an optional, per-project attestation. A desktop binary has no built-in Firebase
//!   provider, so Rigel asks a service the user runs for a short-lived App Check token and
//!   attaches it to every Firebase request. `docs/cloud.md` has the endpoint contract.
//!
//! The Firestore document shape is documented in `docs/cloud.md`, so another client — the phone
//! app, a script — can read the same collection with the same rules.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Mutex as AsyncMutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::migrate::now_ms;
use crate::session::{
    Agent, Item, Session as ProjectSession, SessionError, SessionService, TurnTranscript,
};
use crate::skills::{SkillIndex, SkillRecord, SkillsError};

/// Where the project id and web API key live when they are not in the environment.
pub const CONFIG_FILE: &str = "cloud.json";
/// What this machine last mirrored: the ids it is responsible for, and when. Local only.
const STATE_FILE: &str = "cloud-state.json";
/// The keychain entry holding the refresh token and the account it belongs to.
const KEYCHAIN_SERVICE: &str = "rigel";
const KEYCHAIN_USER: &str = "firebase-session";

pub const DEFAULT_IDENTITY_BASE: &str = "https://identitytoolkit.googleapis.com";
pub const DEFAULT_TOKEN_BASE: &str = "https://securetoken.googleapis.com";
pub const DEFAULT_FIRESTORE_BASE: &str = "https://firestore.googleapis.com";

/// How many shelf rows one push mirrors. The shelf is a catalogue, not a corpus; a cap keeps a
/// mistaken 100 000-row index from becoming a bill.
const CATALOGUE_LIMIT: usize = 5_000;
/// Firestore accepts 500 writes per commit. Stay under it so a retry after a rate limit is cheap.
const WRITE_BATCH: usize = 400;
/// One list page. Firestore's ceiling is 1 000; 300 keeps a response comfortably small.
const PAGE_SIZE: usize = 300;
/// Stop a pull after this many pages rather than walking a collection a bug could make endless.
const MAX_PAGES: usize = 20;
/// Refresh before the token actually expires, so a request does not race the clock.
const TOKEN_MARGIN_MS: i64 = 30_000;
/// A prompt and a reply are bounded before they become a Firestore document. A turn is a step,
/// not a transcript of everything an agent read; tool output is never published at all.
const MAX_PROMPT_CHARS: usize = 8_000;
const MAX_REPLY_CHARS: usize = 40_000;
/// Firebase's own floor for an email/password account.
const MIN_PASSWORD_CHARS: usize = 6;
/// The sentence Identity Toolkit's `EMAIL_NOT_FOUND`, `INVALID_PASSWORD` and
/// `INVALID_LOGIN_CREDENTIALS` all become. Named because the reset path has to recognise it
/// again to answer an unknown address the same way it answers a known one.
const BAD_CREDENTIALS: &str = "the email or password is wrong";
/// Fetch a new App Check token before the old one expires, so a request does not race it.
const APP_CHECK_MARGIN_MS: i64 = 60_000;

#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error(
        "no Firebase project configured: run `rigel cloud configure --project <project-id>` or set RIGEL_FIREBASE_PROJECT_ID and RIGEL_FIREBASE_API_KEY"
    )]
    NotConfigured,

    #[error("`{0}` is not a Firebase project id (lowercase letters, digits and hyphens)")]
    InvalidProject(String),

    #[error(
        "the Firebase web API key is empty; pass one to `rigel cloud configure` or set RIGEL_FIREBASE_API_KEY"
    )]
    InvalidApiKey,

    #[error("not signed in: run `rigel cloud login --email <address>`")]
    NotSignedIn,

    #[error("the App Check attestation URL `{0}` is not an http(s) URL")]
    InvalidAppCheck(String),

    #[error("App Check could not attest this client: {0}")]
    AppCheck(String),

    #[error("{0}")]
    Input(String),

    #[error("the OS keychain refused the request: {0}")]
    Keychain(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("the cloud config is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Firebase rejected the request ({status}): {message}")]
    Rejected { status: u16, message: String },

    #[error("Firebase answered with something Rigel does not understand: {0}")]
    Malformed(String),

    #[error(transparent)]
    Skills(#[from] SkillsError),

    #[error(transparent)]
    Session(#[from] SessionError),
}

impl CloudError {
    /// 1 is something the user can fix (a missing setting, a wrong password); 2 is a failed
    /// precondition or a network fault. The CLI maps these straight through.
    pub fn exit_code(&self) -> u8 {
        match self {
            CloudError::NotConfigured
            | CloudError::InvalidProject(_)
            | CloudError::InvalidApiKey
            | CloudError::NotSignedIn
            | CloudError::InvalidAppCheck(_)
            | CloudError::AppCheck(_)
            | CloudError::Input(_) => 1,
            CloudError::Rejected { status, .. } if (400..500).contains(status) => 1,
            _ => 2,
        }
    }
}

/* -------------------------------- configuration -------------------------------- */

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudConfig {
    /// The Firebase project id, e.g. `rigel-cloud-4f2a1`.
    pub project_id: String,
    /// The project's web API key. Public by design; Firestore rules enforce ownership.
    pub api_key: String,
    /// App Check attestation, when this machine should send App Check tokens. Absent means the
    /// requests go out as they did before; an old `cloud.json` reads as absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_check: Option<AppCheckConfig>,
}

/// The client half of a Firebase App Check custom provider.
///
/// Firebase has no attestation provider for a desktop binary, so the token comes from a service
/// the user runs. The endpoint answers with a token the Firebase Admin SDK minted for this
/// project's app and the Unix second at which it expires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppCheckConfig {
    pub attest_url: String,
}

impl CloudConfig {
    pub fn new(
        project_id: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, CloudError> {
        let config = Self {
            project_id: project_id.into().trim().to_string(),
            api_key: api_key.into().trim().to_string(),
            app_check: None,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn path(dir: &Path) -> PathBuf {
        dir.join(CONFIG_FILE)
    }

    /// The file first, then the environment. A project that configures the window by hand and a
    /// CI job that exports two variables both work without one overwriting the other.
    pub fn load(dir: &Path) -> Result<Option<Self>, CloudError> {
        let path = Self::path(dir);
        if path.is_file() {
            let text = std::fs::read_to_string(&path)?;
            let config: CloudConfig = serde_json::from_str(&text)?;
            config.validate()?;
            return Ok(Some(config));
        }
        Ok(Self::from_env())
    }

    fn from_env() -> Option<Self> {
        let project = std::env::var("RIGEL_FIREBASE_PROJECT_ID").ok()?;
        let key = std::env::var("RIGEL_FIREBASE_API_KEY").ok()?;
        let mut config = Self::new(project, key).ok()?;
        config.app_check = std::env::var("RIGEL_FIREBASE_APP_CHECK_URL")
            .ok()
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty())
            .map(|attest_url| AppCheckConfig { attest_url });
        config.validate().ok()?;
        Some(config)
    }

    /// Write the project id and key beside the index. Returns the path written.
    pub fn save(dir: &Path, config: &Self) -> Result<PathBuf, CloudError> {
        config.validate()?;
        std::fs::create_dir_all(dir)?;
        let path = Self::path(dir);
        std::fs::write(&path, serde_json::to_string_pretty(config)?)?;
        Ok(path)
    }

    fn validate(&self) -> Result<(), CloudError> {
        let project = self.project_id.trim();
        if project.is_empty() {
            return Err(CloudError::NotConfigured);
        }
        let valid = project.len() <= 64
            && project
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !valid {
            return Err(CloudError::InvalidProject(project.to_string()));
        }
        if self.api_key.trim().is_empty() {
            return Err(CloudError::InvalidApiKey);
        }
        if let Some(check) = &self.app_check {
            let url = check.attest_url.trim();
            let parsed = reqwest::Url::parse(url)
                .ok()
                .filter(|url| matches!(url.scheme(), "http" | "https"));
            if parsed.is_none() || url.len() > 2_048 {
                return Err(CloudError::InvalidAppCheck(url.to_string()));
            }
        }
        Ok(())
    }
}

/// The three Firebase hosts, separated so a test can point all of them at one local server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub identity: String,
    pub token: String,
    pub firestore: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self::from_env()
    }
}

impl Endpoints {
    pub fn from_env() -> Self {
        Self {
            identity: base_url("RIGEL_FIREBASE_IDENTITY_BASE", DEFAULT_IDENTITY_BASE),
            token: base_url("RIGEL_FIREBASE_TOKEN_BASE", DEFAULT_TOKEN_BASE),
            firestore: base_url("RIGEL_FIREBASE_FIRESTORE_BASE", DEFAULT_FIRESTORE_BASE),
        }
    }

    /// One origin for all three, which is what the test server offers.
    pub fn same(origin: impl Into<String>) -> Self {
        let origin = origin.into().trim_end_matches('/').to_string();
        Self {
            identity: origin.clone(),
            token: origin.clone(),
            firestore: origin,
        }
    }
}

fn base_url(variable: &str, fallback: &str) -> String {
    std::env::var(variable)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

/* ---------------------------------- the session ---------------------------------- */

/// A signed-in account. The two tokens never reach the renderer or a log line: [`CloudStatus`] is
/// what the window and the CLI see.
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub uid: String,
    pub email: String,
    /// The project that minted this session. A stored session for another project is ignored
    /// rather than sent to a project it was not issued by.
    pub project_id: String,
    #[serde(rename = "idToken")]
    id_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    pub expires_at: i64,
    /// What Firebase last said about the address. Kept so the account page can show it without
    /// a request. [`Cloud::check_email_verification`] re-reads it from Identity Toolkit; a
    /// session written before this field existed reads as unverified until it does.
    #[serde(default)]
    pub email_verified: bool,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("uid", &self.uid)
            .field("email", &self.email)
            .field("project_id", &self.project_id)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// What the window may know about App Check: the endpoint, never the token it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppCheckStatus {
    pub attest_url: String,
}

/// What the window and the CLI may know: who is signed in, never how.
#[derive(Debug, Clone, Serialize)]
pub struct CloudStatus {
    pub configured: bool,
    pub project_id: Option<String>,
    pub signed_in: bool,
    pub email: Option<String>,
    pub uid: Option<String>,
    pub expires_at: Option<i64>,
    /// Whether Firebase has confirmed the account's address. False when signed out; an old
    /// stored session also reads false until a check re-reads the account.
    pub email_verified: bool,
    /// The App Check attestation endpoint, when this machine sends App Check tokens. Never the
    /// token itself: it is a bearer credential and stays in memory.
    pub app_check: Option<AppCheckStatus>,
    /// When this machine last pushed the shelf, and last pulled it. Read from the local
    /// manifest, not the network; `last_sync_at` is the later of the two.
    pub last_push_at: Option<i64>,
    pub last_pull_at: Option<i64>,
    pub last_sync_at: Option<i64>,
}

impl CloudStatus {
    pub fn unconfigured() -> Self {
        Self {
            configured: false,
            project_id: None,
            signed_in: false,
            email: None,
            uid: None,
            expires_at: None,
            email_verified: false,
            app_check: None,
            last_push_at: None,
            last_pull_at: None,
            last_sync_at: None,
        }
    }
}

/// The local side of the mirror: which ids this machine last pushed or pulled, and when.
///
/// It is what makes deletion safe. A missing row is only tombstoned when it was in this
/// machine's manifest and is no longer in the shelf; a fresh machine with an empty shelf has
/// no manifest, so its first push cannot delete the account's cloud rows. The file is scoped
/// to one project and one uid and is ignored when either changes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CloudState {
    #[serde(default)]
    project_id: String,
    #[serde(default)]
    uid: String,
    #[serde(default)]
    skills: BTreeSet<String>,
    #[serde(default)]
    last_push_at: Option<i64>,
    #[serde(default)]
    last_pull_at: Option<i64>,
}

/// The refresh token's home. The keychain is the real store; the memory one is for tests, so a
/// test never reads or writes a developer's Firebase account.
pub trait SessionStore: Send + Sync {
    fn load(&self) -> Result<Option<Session>, CloudError>;
    fn save(&self, session: &Session) -> Result<(), CloudError>;
    fn clear(&self) -> Result<(), CloudError>;
}

pub struct KeychainSessionStore;

impl SessionStore for KeychainSessionStore {
    fn load(&self) -> Result<Option<Session>, CloudError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
            .map_err(|error| CloudError::Keychain(error.to_string()))?;
        match entry.get_password() {
            Ok(text) => serde_json::from_str(&text).map(Some).map_err(|_| {
                CloudError::Malformed(
                    "the stored Firebase session is unreadable; sign in again".to_string(),
                )
            }),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(CloudError::Keychain(error.to_string())),
        }
    }

    fn save(&self, session: &Session) -> Result<(), CloudError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
            .map_err(|error| CloudError::Keychain(error.to_string()))?;
        let text = serde_json::to_string(session)?;
        entry
            .set_password(&text)
            .map_err(|error| CloudError::Keychain(error.to_string()))
    }

    fn clear(&self) -> Result<(), CloudError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
            .map_err(|error| CloudError::Keychain(error.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(CloudError::Keychain(error.to_string())),
        }
    }
}

/// A session kept in one process. Tests use it; nothing else should.
#[derive(Default)]
pub struct MemorySessionStore {
    session: Mutex<Option<Session>>,
}

impl MemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The signed-in account, without either token — what a test can assert on.
    pub fn account(&self) -> Option<(String, String, i64)> {
        self.session
            .lock()
            .expect("session lock")
            .as_ref()
            .map(|s| (s.uid.clone(), s.email.clone(), s.expires_at))
    }
}

impl SessionStore for MemorySessionStore {
    fn load(&self) -> Result<Option<Session>, CloudError> {
        Ok(self.session.lock().expect("session lock").clone())
    }

    fn save(&self, session: &Session) -> Result<(), CloudError> {
        *self.session.lock().expect("session lock") = Some(session.clone());
        Ok(())
    }

    fn clear(&self) -> Result<(), CloudError> {
        *self.session.lock().expect("session lock") = None;
        Ok(())
    }
}

/* ------------------------------------ the client ------------------------------------ */

pub struct Cloud {
    config: CloudConfig,
    endpoints: Endpoints,
    http: reqwest::Client,
    /// `None` when App Check is off. A token cache and its endpoint, never serialized.
    app_check: Option<AppCheck>,
    store: Arc<dyn SessionStore>,
    /// `None` in a client that only talks to the network (the older constructor); the window,
    /// the CLI and the tests that exercise tombstones give it a directory.
    state_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PushReport {
    pub project_id: String,
    pub uid: String,
    pub pushed: usize,
    pub batches: usize,
    /// Local rows that were in the manifest and are gone; each became a tombstone.
    pub deleted: usize,
    /// The shelf is larger than the mirror's cap, so deletions were **not** propagated.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishReport {
    pub project_id: String,
    pub uid: String,
    pub session_id: String,
    pub title: String,
    pub turns: usize,
    pub batches: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PullReport {
    pub project_id: String,
    pub uid: String,
    pub received: usize,
    pub written: usize,
    pub skipped: usize,
    pub pages: usize,
    /// Tombstone documents seen.
    pub tombstones: usize,
    /// Local rows a tombstone actually removed.
    pub deleted: usize,
}

/// The client half of a custom App Check provider.
///
/// It is deliberately not a Firebase SDK: Rigel asks the endpoint the user configured for an
/// App Check token the Firebase Admin SDK minted for this project's app, then attaches it to
/// every Firebase request. The token is a bearer credential, so it lives in memory only and is
/// never serialized, logged or sent to the renderer.
struct AppCheck {
    attest_url: String,
    http: reqwest::Client,
    cached: AsyncMutex<Option<CachedAppCheckToken>>,
}

struct CachedAppCheckToken {
    token: String,
    expires_at: i64,
}

impl AppCheck {
    fn new(attest_url: String, http: reqwest::Client) -> Self {
        Self {
            attest_url,
            http,
            cached: AsyncMutex::new(None),
        }
    }

    /// A token good for at least another minute, fetching one when the cache is empty or close
    /// to expiry. The lock spans the fetch on purpose: a burst of Firestore commits should cost
    /// one attestation, not one per request.
    async fn token(&self) -> Result<String, CloudError> {
        let mut cached = self.cached.lock().await;
        if let Some(current) = cached.as_ref() {
            if current.expires_at - now_ms() > APP_CHECK_MARGIN_MS {
                return Ok(current.token.clone());
            }
        }

        let answer = self.fetch().await?;
        let token = answer.token.trim().to_string();
        if token.is_empty() {
            return Err(CloudError::AppCheck(
                "the attestation endpoint returned an empty token".into(),
            ));
        }

        let now = now_ms();
        let expires_at = answer.expires_at.saturating_mul(1_000);
        if expires_at <= now {
            return Err(CloudError::AppCheck(format!(
                "the attestation endpoint returned a token that expired at {}",
                rfc3339(expires_at)
            )));
        }
        // Firebase's own ceiling for a custom provider is seven days. A larger number is almost
        // certainly milliseconds, which would otherwise be cached as if it never expired.
        if expires_at > now + 7 * 24 * 60 * 60 * 1_000 {
            return Err(CloudError::AppCheck(
                "`expires_at` is more than seven days away; it must be Unix seconds, not milliseconds"
                    .into(),
            ));
        }

        *cached = Some(CachedAppCheckToken {
            token: token.clone(),
            expires_at,
        });
        Ok(token)
    }

    async fn fetch(&self) -> Result<AppCheckAnswer, CloudError> {
        let response = self
            .http
            .get(&self.attest_url)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|error| {
                CloudError::AppCheck(format!(
                    "the attestation endpoint is not reachable: {error}"
                ))
            })?;
        let status = response.status();
        let text = response.text().await.map_err(|error| {
            CloudError::AppCheck(format!(
                "the attestation endpoint's answer could not be read: {error}"
            ))
        })?;
        if !status.is_success() {
            let detail = text.trim();
            let message = if detail.is_empty() {
                format!("the attestation endpoint answered {status}")
            } else {
                format!(
                    "the attestation endpoint answered {status}: {}",
                    truncate(detail, 200)
                )
            };
            return Err(CloudError::AppCheck(message));
        }
        serde_json::from_str(&text).map_err(|error| {
            CloudError::AppCheck(format!(
                "the attestation endpoint must answer with {{\"token\": \"…\", \"expires_at\": <unix seconds>}}: {error}"
            ))
        })
    }
}

impl Cloud {
    /// Build a client over an explicit store. The window and the CLI call [`Cloud::open`]; this
    /// constructor is what the tests use to point at a local server and a memory session.
    pub fn new(
        config: CloudConfig,
        endpoints: Endpoints,
        store: Arc<dyn SessionStore>,
    ) -> Result<Self, CloudError> {
        Self::build(None, config, endpoints, store)
    }

    /// A client that also keeps its manifest in `dir/cloud-state.json`. `open` uses this; tests
    /// point it at a temporary directory so a tombstone can be seen across two pushes.
    pub fn new_at(
        dir: &Path,
        config: CloudConfig,
        endpoints: Endpoints,
        store: Arc<dyn SessionStore>,
    ) -> Result<Self, CloudError> {
        Self::build(Some(dir.to_path_buf()), config, endpoints, store)
    }

    fn build(
        state_dir: Option<PathBuf>,
        config: CloudConfig,
        endpoints: Endpoints,
        store: Arc<dyn SessionStore>,
    ) -> Result<Self, CloudError> {
        config.validate()?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        let app_check = config
            .app_check
            .as_ref()
            .map(|check| AppCheck::new(check.attest_url.clone(), http.clone()));
        Ok(Self {
            config,
            endpoints,
            http,
            app_check,
            store,
            state_dir,
        })
    }

    pub fn open(dir: &Path) -> Result<Self, CloudError> {
        let config = CloudConfig::load(dir)?.ok_or(CloudError::NotConfigured)?;
        Self::new_at(
            dir,
            config,
            Endpoints::from_env(),
            Arc::new(KeychainSessionStore),
        )
    }

    /// Read the stored account without a network call. An unconfigured app answers instead of
    /// failing, because the settings page has to render that state.
    pub fn status_at(dir: &Path) -> Result<CloudStatus, CloudError> {
        match CloudConfig::load(dir)? {
            Some(config) => Self::new_at(
                dir,
                config,
                Endpoints::from_env(),
                Arc::new(KeychainSessionStore),
            )?
            .status(),
            None => Ok(CloudStatus::unconfigured()),
        }
    }

    pub fn config(&self) -> &CloudConfig {
        &self.config
    }

    pub fn status(&self) -> Result<CloudStatus, CloudError> {
        let session = self
            .store
            .load()?
            .filter(|session| session.project_id == self.config.project_id);
        // A missing or corrupt manifest is not worth failing the settings page over; push and
        // pull report it in full. Without a signed-in account the timestamps have no owner, so
        // they are hidden rather than shown against the next person to sign in.
        let (last_push_at, last_pull_at) = match session.as_ref() {
            Some(session) => match self.load_state(&session.uid) {
                Ok(state) => (state.last_push_at, state.last_pull_at),
                Err(_) => (None, None),
            },
            None => (None, None),
        };
        let last_sync_at = match (last_push_at, last_pull_at) {
            (Some(pushed), Some(pulled)) => Some(pushed.max(pulled)),
            (pushed, pulled) => pushed.or(pulled),
        };
        Ok(CloudStatus {
            configured: true,
            project_id: Some(self.config.project_id.clone()),
            signed_in: session.is_some(),
            email: session.as_ref().map(|session| session.email.clone()),
            uid: session.as_ref().map(|session| session.uid.clone()),
            expires_at: session.as_ref().map(|session| session.expires_at),
            email_verified: session
                .as_ref()
                .is_some_and(|session| session.email_verified),
            app_check: self.config.app_check.as_ref().map(|check| AppCheckStatus {
                attest_url: check.attest_url.clone(),
            }),
            last_push_at,
            last_pull_at,
            last_sync_at,
        })
    }

    /* ------------------------------------ manifest ------------------------------------ */

    fn state_path(&self) -> Option<PathBuf> {
        self.state_dir.as_ref().map(|dir| dir.join(STATE_FILE))
    }

    /// The manifest for this account, or an empty one when there is no file, it belongs to
    /// another project/uid, or this client has no directory to keep one in.
    fn load_state(&self, uid: &str) -> Result<CloudState, CloudError> {
        let Some(path) = self.state_path() else {
            return Ok(CloudState::default());
        };
        if !path.is_file() {
            return Ok(CloudState::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let state: CloudState = serde_json::from_str(&text).map_err(|error| {
            CloudError::Malformed(format!(
                "{STATE_FILE} is unreadable ({error}); delete it to reset the mirror's manifest"
            ))
        })?;
        if state.project_id != self.config.project_id || state.uid != uid {
            // Another account's manifest must never decide what this account deletes.
            return Ok(CloudState::default());
        }
        Ok(state)
    }

    fn save_state(&self, state: &CloudState) -> Result<(), CloudError> {
        let Some(path) = self.state_path() else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let staging = path.with_extension("json.tmp");
        std::fs::write(&staging, serde_json::to_string_pretty(state)?)?;
        // `rename` over an existing file fails on Windows; the manifest is small, so the
        // remove-then-rename window costs nothing that a retry cannot rebuild.
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        std::fs::rename(&staging, &path)?;
        Ok(())
    }

    /// Sign in through Identity Toolkit and keep the refresh token in the OS keychain. The
    /// password is not stored anywhere and is not part of the returned value.
    pub async fn sign_in(&self, email: &str, password: &str) -> Result<CloudStatus, CloudError> {
        let email = check_credentials(email, password)?;
        let answer: SignInAnswer = self
            .post_json(
                &self.identity_url("accounts:signInWithPassword"),
                &json!({
                    "email": email,
                    "password": password,
                    "returnSecureToken": true,
                }),
            )
            .await?;
        self.keep_session(answer, email)
    }

    /// Create the account, then keep the session Firebase returns for it — a new account is
    /// signed in, not told to sign in again.
    ///
    /// Identity Toolkit refuses an address that already has an account (`EMAIL_EXISTS`), which is
    /// the only duplicate check that cannot be raced. Enabling the provider means the project's
    /// public web API key can create accounts, so a public project should restrict sign-up with
    /// App Check or a blocking function; `docs/cloud.md` spells that out.
    pub async fn sign_up(&self, email: &str, password: &str) -> Result<CloudStatus, CloudError> {
        let email = check_credentials(email, password)?;
        if password.chars().count() < MIN_PASSWORD_CHARS {
            return Err(CloudError::Input(format!(
                "Firebase needs a password of at least {MIN_PASSWORD_CHARS} characters"
            )));
        }
        let answer: SignInAnswer = self
            .post_json(
                &self.identity_url("accounts:signUp"),
                &json!({
                    "email": email,
                    "password": password,
                    "returnSecureToken": true,
                }),
            )
            .await?;
        self.keep_session(answer, email)
    }

    /// Ask Firebase to email a reset link.
    ///
    /// The answer is deliberately the same whether or not the address has an account. Firebase
    /// answers `EMAIL_NOT_FOUND` when its email-enumeration protection is off, and this client
    /// must not become the account oracle that protection removes, so an unknown address is
    /// reported exactly like a known one. The returned string is the address the link went to
    /// (or would have).
    pub async fn send_password_reset(&self, email: &str) -> Result<String, CloudError> {
        let email = email.trim();
        if email.is_empty() {
            return Err(CloudError::Input("an email address is required".into()));
        }
        let answer: Result<ResetAnswer, CloudError> = self
            .post_json(
                &self.identity_url("accounts:sendOobCode"),
                &json!({ "requestType": "PASSWORD_RESET", "email": email }),
            )
            .await;
        match answer {
            Ok(answer) => Ok(answer.email.unwrap_or_else(|| email.to_string())),
            Err(CloudError::Rejected {
                status: 400,
                message,
            }) if message == BAD_CREDENTIALS => Ok(email.to_string()),
            Err(error) => Err(error),
        }
    }

    /// Ask Firebase to email a verification link to the signed-in account.
    ///
    /// The link, its template and its expiry belong to Firebase; the answer is the address the
    /// link went to. Sending again is safe and is what the resend control does: a new link is
    /// minted, and the previous one still works until it expires.
    pub async fn send_verification_email(&self) -> Result<String, CloudError> {
        let session = self.token().await?;
        if session.email_verified {
            return Err(CloudError::Input(
                "this account's email is already verified".into(),
            ));
        }
        let answer: ResetAnswer = self
            .post_json(
                &self.identity_url("accounts:sendOobCode"),
                &json!({ "requestType": "VERIFY_EMAIL", "idToken": session.id_token }),
            )
            .await?;
        Ok(answer.email.unwrap_or(session.email))
    }

    /// Re-read the account's verification state from Identity Toolkit and keep it.
    ///
    /// Reading the stored ID token would not do: its `email_verified` claim stays false until a
    /// new token is minted, so a link clicked a minute ago would still look unverified. The
    /// lookup answers from the user record instead. An unchanged answer writes nothing, so a
    /// check costs one request and no keychain write. The ID token itself is not refreshed here;
    /// nothing in Rigel's rules reads the claim, and it updates with the next token anyway.
    pub async fn check_email_verification(&self) -> Result<CloudStatus, CloudError> {
        let session = self.token().await?;
        let answer: LookupAnswer = self
            .post_json(
                &self.identity_url("accounts:lookup"),
                &json!({ "idToken": session.id_token }),
            )
            .await?;
        // Absent means this project did not say; keep what this machine already knew rather
        // than turning a verified account unverified because a field was omitted.
        let verified = answer
            .users
            .first()
            .and_then(|user| user.email_verified)
            .unwrap_or(session.email_verified);
        if verified != session.email_verified {
            let mut next = session;
            next.email_verified = verified;
            self.store.save(&next)?;
        }
        self.status()
    }

    fn identity_url(&self, method: &str) -> String {
        format!(
            "{}/v1/{method}?key={}",
            self.endpoints.identity, self.config.api_key
        )
    }

    /// Store what sign-in, sign-up and refresh all agree on, then report the account.
    fn keep_session(&self, answer: SignInAnswer, email: &str) -> Result<CloudStatus, CloudError> {
        let lifetime = answer.lifetime_ms();
        let session = Session {
            uid: answer.local_id,
            email: answer.email.unwrap_or_else(|| email.to_string()),
            project_id: self.config.project_id.clone(),
            id_token: answer.id_token,
            refresh_token: answer.refresh_token,
            expires_at: now_ms() + lifetime - TOKEN_MARGIN_MS,
            email_verified: answer.email_verified,
        };
        self.store.save(&session)?;
        self.status()
    }

    /// Forget the local refresh token. Firebase keeps the account; this is a sign-out, not a
    /// deletion, and it makes no request.
    pub fn sign_out(&self) -> Result<CloudStatus, CloudError> {
        self.store.clear()?;
        self.status()
    }

    /// Mirror the local shelf's metadata into the user's Firestore project.
    ///
    /// Rows that were in this machine's manifest but are no longer in the shelf become
    /// tombstones. A fresh machine has no manifest, so its first push never deletes cloud rows
    /// it has not seen. If the shelf is larger than [`CATALOGUE_LIMIT`], deletion propagation is
    /// paused and reported rather than guessing at what the truncated tail contained.
    pub async fn push(&self, index: &SkillIndex) -> Result<PushReport, CloudError> {
        let session = self.token().await?;
        let records = index.catalogue(CATALOGUE_LIMIT).await?;
        let total = index.count("").await? as usize;
        let truncated = total > CATALOGUE_LIMIT;
        let mut state = self.load_state(&session.uid)?;
        let live: BTreeSet<String> = records.iter().map(|record| record.id.clone()).collect();
        let removed: Vec<String> = if truncated {
            Vec::new()
        } else {
            state.skills.difference(&live).cloned().collect()
        };

        // One document for the account itself: who last synced, from which build and platform.
        // It also proves the rules allow a write before the skill batches start.
        self.commit(
            &session,
            vec![profile_write(&self.config.project_id, &session)],
        )
        .await?;

        let mut pushed = 0;
        let mut batches = 0;
        for chunk in records.chunks(WRITE_BATCH) {
            let writes = chunk
                .iter()
                .map(|record| skill_write(&self.config.project_id, &session.uid, record))
                .collect::<Vec<_>>();
            self.commit(&session, writes).await?;
            pushed += chunk.len();
            batches += 1;
        }

        let mut deleted = 0;
        for chunk in removed.chunks(WRITE_BATCH) {
            let writes = chunk
                .iter()
                .map(|id| skill_tombstone_write(&self.config.project_id, &session.uid, id))
                .collect::<Vec<_>>();
            self.commit(&session, writes).await?;
            deleted += chunk.len();
            batches += 1;
        }

        if !truncated {
            state.skills = live;
        }
        state.project_id = self.config.project_id.clone();
        state.uid = session.uid.clone();
        state.last_push_at = Some(now_ms());
        self.save_state(&state)?;

        Ok(PushReport {
            project_id: self.config.project_id.clone(),
            uid: session.uid,
            pushed,
            batches,
            deleted,
            truncated,
        })
    }

    /// Bring down rows this machine does not have yet. Local bodies win: a pulled record carries
    /// no `body_sha`, so `write_skill` keeps whatever blob the index already points at.
    ///
    /// A tombstone is authoritative: the row and its evidence are removed locally, and the id
    /// leaves the manifest so a later push cannot resurrect it.
    pub async fn pull(&self, index: &SkillIndex) -> Result<PullReport, CloudError> {
        let session = self.token().await?;
        let mut records = Vec::new();
        let mut tombstones = BTreeSet::new();
        let mut skipped = 0;
        let mut pages = 0;
        let mut page_token: Option<String> = None;

        loop {
            let page = self.list_skills(&session, page_token.as_deref()).await?;
            pages += 1;
            for document in &page.documents {
                if let Some(id) = tombstone_id(document) {
                    tombstones.insert(id);
                    continue;
                }
                match decode_skill(document) {
                    Some(record) => records.push(record),
                    None => skipped += 1,
                }
            }
            match page.next_page_token {
                Some(token) if pages < MAX_PAGES => page_token = Some(token),
                _ => break,
            }
        }

        let received = records.len();
        let written = index.ingest_records(&records).await?;
        let mut deleted = 0;
        for id in &tombstones {
            if index.remove(id).await? {
                deleted += 1;
            }
        }

        let mut state = self.load_state(&session.uid)?;
        state.project_id = self.config.project_id.clone();
        state.uid = session.uid.clone();
        for record in &records {
            state.skills.insert(record.id.clone());
        }
        for id in &tombstones {
            state.skills.remove(id);
        }
        state.last_pull_at = Some(now_ms());
        self.save_state(&state)?;

        Ok(PullReport {
            project_id: self.config.project_id.clone(),
            uid: session.uid,
            received,
            written,
            skipped,
            pages,
            tombstones: tombstones.len(),
            deleted,
        })
    }

    /// Publish one session to `users/{uid}/sessions/{session_id}` and its turns to the `turns`
    /// subcollection, where the phone reads them.
    ///
    /// The header is written **last**, after every turn batch: a phone that lists sessions never
    /// shows a header whose turns did not land. Only the prompt and the assistant's own text
    /// move — tool calls, command output, file changes and the local project path stay here.
    pub async fn publish_session(
        &self,
        service: &SessionService,
        session_id: &str,
    ) -> Result<PublishReport, CloudError> {
        if session_id.contains('/') {
            return Err(CloudError::Malformed(
                "a session id cannot contain a slash".to_string(),
            ));
        }
        let token = self.token().await?;
        let session = service.get(session_id).await?;
        let agents = service.agents(session_id).await?;
        let transcripts = service.transcript(session_id).await?;

        let mut published = 0;
        let mut batches = 0;
        for chunk in transcripts.chunks(WRITE_BATCH) {
            let writes = chunk
                .iter()
                .enumerate()
                .map(|(offset, transcript)| {
                    let agent = agents
                        .iter()
                        .find(|agent| agent.id == transcript.turn.agent_id);
                    turn_write(
                        &self.config.project_id,
                        &token.uid,
                        session_id,
                        published + offset + 1,
                        transcript,
                        agent,
                    )
                })
                .collect::<Vec<_>>();
            self.commit(&token, writes).await?;
            published += chunk.len();
            batches += 1;
        }

        self.commit(
            &token,
            vec![session_write(
                &self.config.project_id,
                &token.uid,
                &session,
                transcripts.len(),
                agents.len(),
            )],
        )
        .await?;

        let title = session_title(&session);
        Ok(PublishReport {
            project_id: self.config.project_id.clone(),
            uid: token.uid,
            session_id: session.id,
            title,
            turns: published,
            batches,
        })
    }

    /* ------------------------------------ auth ------------------------------------ */

    /// The current ID token, refreshed when it is within the margin. `None` is a sign-out; an
    /// expired token is not, because the refresh token is what the user actually holds.
    async fn token(&self) -> Result<Session, CloudError> {
        let Some(session) = self.store.load()? else {
            return Err(CloudError::NotSignedIn);
        };
        if session.project_id != self.config.project_id {
            return Err(CloudError::NotSignedIn);
        }
        if session.expires_at > now_ms() + TOKEN_MARGIN_MS {
            return Ok(session);
        }

        let answer: RefreshAnswer = self
            .post_form(
                &format!(
                    "{}/v1/token?key={}",
                    self.endpoints.token, self.config.api_key
                ),
                &[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", session.refresh_token.as_str()),
                ],
            )
            .await?;

        let lifetime = answer.lifetime_ms();
        let next = Session {
            uid: answer.user_id,
            email: session.email,
            project_id: self.config.project_id.clone(),
            id_token: answer.id_token,
            refresh_token: answer.refresh_token,
            expires_at: now_ms() + lifetime - TOKEN_MARGIN_MS,
            email_verified: session.email_verified,
        };
        self.store.save(&next)?;
        Ok(next)
    }

    /* ---------------------------------- Firestore ---------------------------------- */

    async fn commit(&self, session: &Session, writes: Vec<Value>) -> Result<(), CloudError> {
        if writes.is_empty() {
            return Ok(());
        }
        let url = format!(
            "{}/v1/projects/{}/databases/(default)/documents:commit",
            self.endpoints.firestore, self.config.project_id
        );
        let _: Value = self
            .post_json_authorized(&url, &json!({ "writes": writes }), &session.id_token)
            .await?;
        Ok(())
    }

    async fn list_skills(
        &self,
        session: &Session,
        page_token: Option<&str>,
    ) -> Result<ListAnswer, CloudError> {
        let url = format!(
            "{}/v1/projects/{}/databases/(default)/documents/users/{}/skills",
            self.endpoints.firestore, self.config.project_id, session.uid
        );
        let mut request = self
            .http
            .get(&url)
            .bearer_auth(&session.id_token)
            .query(&[("pageSize", PAGE_SIZE.to_string())]);
        if let Some(token) = page_token {
            request = request.query(&[("pageToken", token)]);
        }
        if let Some(token) = self.app_check_token().await? {
            request = request.header("X-Firebase-AppCheck", token);
        }
        let response = request.send().await?;
        self.decode(response).await
    }

    /* ------------------------------------ HTTP ------------------------------------ */

    /// The value of `X-Firebase-AppCheck`, or `None` when App Check is off.
    ///
    /// A configured App Check that cannot produce a token fails the request instead of sending
    /// it un-attested: with enforcement on, Firebase would reject it anyway, and a local error
    /// names the endpoint that needs fixing.
    async fn app_check_token(&self) -> Result<Option<String>, CloudError> {
        match &self.app_check {
            Some(check) => Ok(Some(check.token().await?)),
            None => Ok(None),
        }
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &Value,
    ) -> Result<T, CloudError> {
        let mut request = self.http.post(url).json(body);
        if let Some(token) = self.app_check_token().await? {
            request = request.header("X-Firebase-AppCheck", token);
        }
        let response = request.send().await?;
        self.decode(response).await
    }

    async fn post_json_authorized<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &Value,
        token: &str,
    ) -> Result<T, CloudError> {
        let mut request = self.http.post(url).bearer_auth(token).json(body);
        if let Some(app_check) = self.app_check_token().await? {
            request = request.header("X-Firebase-AppCheck", app_check);
        }
        let response = request.send().await?;
        self.decode(response).await
    }

    async fn post_form<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        pairs: &[(&str, &str)],
    ) -> Result<T, CloudError> {
        let mut request = self.http.post(url).form(pairs);
        if let Some(token) = self.app_check_token().await? {
            request = request.header("X-Firebase-AppCheck", token);
        }
        let response = request.send().await?;
        self.decode(response).await
    }

    async fn decode<T: serde::de::DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> Result<T, CloudError> {
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(CloudError::Rejected {
                status: status.as_u16(),
                message: firebase_message(&text),
            });
        }
        serde_json::from_str(&text)
            .map_err(|error| CloudError::Malformed(format!("{error}: {}", truncate(&text, 240))))
    }
}

/* ---------------------------------- wire shapes ---------------------------------- */

#[derive(Deserialize)]
struct SignInAnswer {
    #[serde(rename = "localId")]
    local_id: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(rename = "idToken")]
    id_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    /// Seconds, as a string — Firebase's own choice, not ours.
    #[serde(rename = "expiresIn", default)]
    expires_in: Option<String>,
    /// Present on sign-in and sign-up; absent in some project configurations, hence the default.
    #[serde(rename = "emailVerified", default)]
    email_verified: bool,
}

impl SignInAnswer {
    fn lifetime_ms(&self) -> i64 {
        self.expires_in
            .as_deref()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(3_600)
            .saturating_mul(1_000)
    }
}

#[derive(Deserialize)]
struct RefreshAnswer {
    #[serde(rename = "id_token")]
    id_token: String,
    #[serde(rename = "refresh_token")]
    refresh_token: String,
    #[serde(rename = "expires_in", default)]
    expires_in: Option<String>,
    #[serde(rename = "user_id")]
    user_id: String,
}

impl RefreshAnswer {
    fn lifetime_ms(&self) -> i64 {
        self.expires_in
            .as_deref()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(3_600)
            .saturating_mul(1_000)
    }
}

#[derive(Deserialize)]
struct ResetAnswer {
    #[serde(default)]
    email: Option<String>,
}

#[derive(Deserialize)]
struct AppCheckAnswer {
    token: String,
    /// Unix seconds. Milliseconds are refused by the caller, not silently reinterpreted.
    expires_at: i64,
}

#[derive(Deserialize)]
struct LookupAnswer {
    #[serde(default)]
    users: Vec<LookupUser>,
}

#[derive(Deserialize)]
struct LookupUser {
    /// `None` when the response omitted it, which is not the same as `false`.
    #[serde(rename = "emailVerified", default)]
    email_verified: Option<bool>,
}

#[derive(Deserialize, Default)]
struct ListAnswer {
    #[serde(default)]
    documents: Vec<Value>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
}

/* -------------------------------- document encoding -------------------------------- */

fn session_write(
    project_id: &str,
    uid: &str,
    session: &ProjectSession,
    turn_count: usize,
    agent_count: usize,
) -> Value {
    let mut fields = Map::new();
    let project = project_label(&session.project_root);
    insert_string(&mut fields, "id", Some(&session.id));
    insert_string(&mut fields, "title", Some(&session_title(session)));
    insert_string(&mut fields, "project", Some(&project));
    insert_string(&mut fields, "status", Some(session.status.as_str()));
    insert_int(&mut fields, "created_at", Some(session.created_at));
    insert_int(&mut fields, "ended_at", session.ended_at);
    insert_int(&mut fields, "spend_micros", Some(session.spend_micros));
    insert_int(&mut fields, "budget_cents", session.budget_cents);
    insert_int(&mut fields, "tasks_done", Some(session.tasks_done));
    insert_int(&mut fields, "turn_count", Some(turn_count as i64));
    insert_int(&mut fields, "agent_count", Some(agent_count as i64));
    insert_timestamp(&mut fields, "published_at", now_ms());
    fields.insert("schema".into(), json!({ "integerValue": "1" }));
    fields.insert("app".into(), json!({ "stringValue": "Rigel" }));

    json!({
        "update": {
            "name": session_document(project_id, uid, &session.id),
            "fields": Value::Object(fields),
        }
    })
}

fn turn_write(
    project_id: &str,
    uid: &str,
    session_id: &str,
    order: usize,
    transcript: &TurnTranscript,
    agent: Option<&Agent>,
) -> Value {
    let turn = &transcript.turn;
    let (prompt, prompt_cut) = clipped(turn.prompt.as_deref().unwrap_or(""), MAX_PROMPT_CHARS);
    let (reply, reply_cut) = clipped(&reply_text(&transcript.items), MAX_REPLY_CHARS);

    let mut fields = Map::new();
    insert_string(&mut fields, "id", Some(&turn.id));
    insert_int(&mut fields, "order", Some(order as i64));
    insert_int(&mut fields, "seq", Some(turn.seq));
    insert_string(&mut fields, "status", Some(&turn.status));
    insert_string(&mut fields, "prompt", Some(&prompt));
    insert_string(&mut fields, "reply", Some(&reply));
    insert_int(&mut fields, "started_at", Some(turn.started_at));
    insert_int(&mut fields, "completed_at", turn.completed_at);
    insert_string(&mut fields, "stop_reason", turn.stop_reason.as_deref());
    if let Some(agent) = agent {
        insert_string(&mut fields, "harness", Some(&agent.harness));
        insert_string(&mut fields, "role", agent.role.as_deref());
        insert_string(&mut fields, "model", agent.model.as_deref());
    }
    if let Some(usage) = &turn.usage {
        insert_int(&mut fields, "cost_micros", Some(usage.cost_micros));
        insert_int(&mut fields, "input_tokens", Some(usage.input_tokens as i64));
        insert_int(
            &mut fields,
            "output_tokens",
            Some(usage.output_tokens as i64),
        );
    }
    fields.insert(
        "subagent".into(),
        json!({ "booleanValue": turn.parent_turn_id.is_some() }),
    );
    fields.insert(
        "truncated".into(),
        json!({ "booleanValue": prompt_cut || reply_cut }),
    );
    insert_timestamp(&mut fields, "published_at", now_ms());
    fields.insert("schema".into(), json!({ "integerValue": "1" }));
    fields.insert("app".into(), json!({ "stringValue": "Rigel" }));

    json!({
        "update": {
            "name": turn_document(project_id, uid, session_id, &turn.id),
            "fields": Value::Object(fields),
        }
    })
}

/// The assistant's own words, in order. Tool results, reasoning, commands and file changes are
/// not part of a published turn; they are where a transcript would leak a secret or a diff.
fn reply_text(items: &[Item]) -> String {
    let mut parts = Vec::new();
    for item in items {
        if item.kind != "message" {
            continue;
        }
        if item.body.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if let Some(text) = item.body.get("text").and_then(Value::as_str) {
            let text = text.trim();
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }
    parts.join("\n\n")
}

fn clipped(text: &str, limit: usize) -> (String, bool) {
    if text.chars().count() <= limit {
        return (text.to_string(), false);
    }
    (text.chars().take(limit).collect(), true)
}

/// The phone only needs the folder's name; publishing the absolute path would put a username on
/// every document for no benefit.
fn project_label(root: &str) -> String {
    let trimmed = root.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(root)
        .to_string()
}

fn insert_timestamp(fields: &mut Map<String, Value>, key: &str, ms: i64) {
    fields.insert(key.to_string(), json!({ "timestampValue": rfc3339(ms) }));
}

/// The header the phone requires. A session renamed to nothing is still a
/// session, and Firestore does not write an empty string field.
fn session_title(session: &ProjectSession) -> String {
    session
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("Untitled session")
        .to_string()
}

fn session_document(project_id: &str, uid: &str, session_id: &str) -> String {
    format!("projects/{project_id}/databases/(default)/documents/users/{uid}/sessions/{session_id}")
}

fn turn_document(project_id: &str, uid: &str, session_id: &str, turn_id: &str) -> String {
    format!(
        "{}/turns/{turn_id}",
        session_document(project_id, uid, session_id)
    )
}

fn profile_write(project_id: &str, session: &Session) -> Value {
    json!({
        "update": {
            "name": document_name(project_id, &session.uid, None),
            "fields": {
                "email": { "stringValue": session.email },
                "app": { "stringValue": "Rigel" },
                "version": { "stringValue": env!("CARGO_PKG_VERSION") },
                "platform": { "stringValue": std::env::consts::OS },
                "schema": { "integerValue": "1" },
                "updated_at": { "timestampValue": rfc3339(now_ms()) },
            }
        }
    })
}

fn skill_write(project_id: &str, uid: &str, record: &SkillRecord) -> Value {
    let mut fields = Map::new();
    fields.insert("id".into(), json!({ "stringValue": record.id }));
    fields.insert("name".into(), json!({ "stringValue": record.name }));
    fields.insert(
        "description".into(),
        json!({ "stringValue": record.description }),
    );
    fields.insert("source".into(), json!({ "stringValue": record.source }));
    insert_string(&mut fields, "repo", record.repo.as_deref());
    insert_string(&mut fields, "path", record.path.as_deref());
    insert_string(&mut fields, "url", record.url.as_deref());
    insert_string(&mut fields, "grade", record.grade.as_deref());
    insert_string(&mut fields, "grade_scope", record.grade_scope.as_deref());
    insert_string(&mut fields, "license", record.license.as_deref());
    insert_int(&mut fields, "stars", record.stars);
    insert_int(&mut fields, "installs", record.installs);
    insert_int(&mut fields, "tokens", record.tokens);

    if let Some(capabilities) = record.capabilities_json() {
        fields.insert(
            "capabilities".into(),
            json!({ "stringValue": capabilities }),
        );
    }
    if let Some(simhash) = record.simhash {
        fields.insert(
            "simhash".into(),
            json!({ "stringValue": format!("{:016x}", simhash as u64) }),
        );
    }
    fields.insert(
        "updated_at".into(),
        json!({ "timestampValue": rfc3339(now_ms()) }),
    );
    fields.insert("app".into(), json!({ "stringValue": "Rigel" }));
    fields.insert("schema".into(), json!({ "integerValue": "1" }));

    json!({
        "update": {
            "name": document_name(project_id, uid, Some(&document_id(&record.id))),
            "fields": Value::Object(fields),
        }
    })
}

/// A deletion marker at the skill's normal document path. It keeps the `id` so a pulling
/// client can find the local row, and deliberately carries no name, description or grade: a
/// tombstone must not read as a skill to a client that does not know the field.
fn skill_tombstone_write(project_id: &str, uid: &str, skill_id: &str) -> Value {
    let now = now_ms();
    json!({
        "update": {
            "name": document_name(project_id, uid, Some(&document_id(skill_id))),
            "fields": {
                "id": { "stringValue": skill_id },
                "deleted": { "booleanValue": true },
                "deleted_at": { "timestampValue": rfc3339(now) },
                "updated_at": { "timestampValue": rfc3339(now) },
                "app": { "stringValue": "Rigel" },
                "schema": { "integerValue": "1" },
            }
        }
    })
}

fn insert_string(fields: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        fields.insert(key.to_string(), json!({ "stringValue": value }));
    }
}

fn insert_int(fields: &mut Map<String, Value>, key: &str, value: Option<i64>) {
    if let Some(value) = value {
        fields.insert(
            key.to_string(),
            json!({ "integerValue": value.to_string() }),
        );
    }
}

fn decode_skill(document: &Value) -> Option<SkillRecord> {
    let fields = document.get("fields")?.as_object()?;
    let id = field_string(fields, "id")?;
    let name = field_string(fields, "name")?;
    let capabilities = field_string(fields, "capabilities")
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let simhash = field_string(fields, "simhash")
        .and_then(|text| u64::from_str_radix(&text, 16).ok())
        .map(|value| value as i64);

    Some(SkillRecord {
        id,
        name,
        description: field_string(fields, "description").unwrap_or_default(),
        source: field_string(fields, "source").unwrap_or_else(|| "cloud".to_string()),
        repo: field_string(fields, "repo"),
        path: field_string(fields, "path"),
        url: field_string(fields, "url"),
        body_sha: None,
        grade: field_string(fields, "grade"),
        grade_scope: field_string(fields, "grade_scope"),
        license: field_string(fields, "license"),
        stars: field_int(fields, "stars"),
        installs: field_int(fields, "installs"),
        tokens: field_int(fields, "tokens"),
        capabilities,
        simhash,
    })
}

/// The id of a deleted row, when the document is a tombstone. A live document has `deleted`
/// absent or false; a malformed tombstone without an id is skipped, not guessed at.
fn tombstone_id(document: &Value) -> Option<String> {
    let fields = document.get("fields")?.as_object()?;
    if !fields.get("deleted")?.get("booleanValue")?.as_bool()? {
        return None;
    }
    field_string(fields, "id")
}

fn field_string(fields: &Map<String, Value>, key: &str) -> Option<String> {
    fields
        .get(key)?
        .get("stringValue")?
        .as_str()
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn field_int(fields: &Map<String, Value>, key: &str) -> Option<i64> {
    let value = fields.get(key)?;
    if let Some(text) = value.get("integerValue").and_then(Value::as_str) {
        return text.parse().ok();
    }
    value
        .get("doubleValue")
        .and_then(Value::as_f64)
        .map(|value| value as i64)
}

/// A Firestore document's fully qualified name. Firestore requires the name's project to match
/// the request's, which is why the id is passed in rather than assumed. `None` is the account
/// document at `users/{uid}`; a document id nests it under `skills`.
fn document_name(project_id: &str, uid: &str, document: Option<&str>) -> String {
    match document {
        None => format!("projects/{project_id}/databases/(default)/documents/users/{uid}"),
        Some(document) => format!(
            "projects/{project_id}/databases/(default)/documents/users/{uid}/skills/{document}"
        ),
    }
}

/// The document id for a skill id. Skill ids contain `/` (`github:owner/repo:path`), which a
/// Firestore document id cannot; a hash is stable across machines and path-safe.
pub fn document_id(skill_id: &str) -> String {
    hex(&Sha256::digest(skill_id.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// RFC 3339 in UTC. Firestore's `timestampValue` is a string; a date crate for one format is not
/// worth the dependency, so this is Howard Hinnant's civil-date arithmetic again.
fn rfc3339(ms: i64) -> String {
    let seconds = ms.div_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let rem = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Firebase's error bodies are a JSON envelope around a machine code. Translate the ones a user
/// can act on and keep the rest; a wrong password must not read like a network failure.
fn firebase_message(body: &str) -> String {
    let raw = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| truncate(body.trim(), 240));

    match raw.as_str() {
        "EMAIL_NOT_FOUND" | "INVALID_PASSWORD" | "INVALID_LOGIN_CREDENTIALS" => {
            BAD_CREDENTIALS.to_string()
        }
        "OPERATION_NOT_ALLOWED" => {
            "email/password sign-in is disabled in this Firebase project".to_string()
        }
        "INVALID_EMAIL" => "that is not an email address Firebase accepts".to_string(),
        "MISSING_PASSWORD" => "a password is required".to_string(),
        "EMAIL_EXISTS" => "that email already has an account".to_string(),
        "USER_DISABLED" => "that account is disabled in the Firebase console".to_string(),
        "USER_NOT_FOUND" => "that account no longer exists in this Firebase project".to_string(),
        "TOO_MANY_ATTEMPTS_TRY_LATER" => "too many attempts; try again later".to_string(),
        "INVALID_API_KEY" | "API_KEY_INVALID" => {
            "the Firebase web API key is not valid".to_string()
        }
        "CONFIGURATION_NOT_FOUND" => {
            "this project has no email/password sign-in: enable it in Firebase Authentication"
                .to_string()
        }
        other if other.starts_with("WEAK_PASSWORD") => {
            format!("Firebase needs a password of at least {MIN_PASSWORD_CHARS} characters")
        }
        other => {
            let lower = other.to_ascii_lowercase();
            if lower.contains("app check") || lower.contains("appcheck") {
                "App Check rejected this request: check the attestation endpoint and the project's App Check settings (see docs/cloud.md)".to_string()
            } else if lower.contains("has not been used in project")
                || lower.contains("firestore api")
            {
                "Firestore is not enabled in this project: create the database in the Firebase console".to_string()
            } else if lower.contains("permission") {
                "permission denied: deploy firestore.rules to this project (see docs/cloud.md)"
                    .to_string()
            } else {
                other.to_string()
            }
        }
    }
}

/// The two fields every account call needs, checked before a request is made so a typo does not
/// cost a round trip. A password that is merely short is Firebase's to refuse, not this function's.
fn check_credentials<'a>(email: &'a str, password: &str) -> Result<&'a str, CloudError> {
    let email = email.trim();
    if email.is_empty() {
        return Err(CloudError::Input("an email address is required".into()));
    }
    if password.is_empty() {
        return Err(CloudError::Input("a password is required".into()));
    }
    Ok(email)
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Usage;
    use crate::session::{SessionStatus, Turn};

    fn record() -> SkillRecord {
        SkillRecord {
            id: "github:acme/skills:skills/pdf/SKILL.md".to_string(),
            name: "pdf".to_string(),
            description: "Read and split PDFs.".to_string(),
            source: "github".to_string(),
            repo: Some("acme/skills".to_string()),
            path: Some("skills/pdf/SKILL.md".to_string()),
            url: Some("https://example.com/pdf".to_string()),
            body_sha: Some("deadbeef".to_string()),
            grade: Some("B".to_string()),
            grade_scope: Some("file-only".to_string()),
            license: Some("MIT".to_string()),
            stars: Some(12),
            installs: Some(3),
            tokens: Some(480),
            capabilities: Vec::new(),
            simhash: Some(0x0000_0000_0000_00ff),
        }
    }

    #[test]
    fn a_config_round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = CloudConfig::new("rigel-cloud-4f2a1", "key-123").unwrap();
        config.app_check = Some(AppCheckConfig {
            attest_url: "https://example.com/app-check".into(),
        });
        let path = CloudConfig::save(dir.path(), &config).unwrap();
        assert!(path.ends_with(CONFIG_FILE));
        assert_eq!(CloudConfig::load(dir.path()).unwrap(), Some(config));
    }

    #[test]
    fn an_app_check_endpoint_must_be_an_http_url() {
        let mut config = CloudConfig::new("rigel-cloud", "key").unwrap();
        config.app_check = Some(AppCheckConfig {
            attest_url: "ftp://example.com/token".into(),
        });
        assert!(matches!(
            config.validate(),
            Err(CloudError::InvalidAppCheck(_))
        ));
        config.app_check = Some(AppCheckConfig {
            attest_url: "not a url".into(),
        });
        assert!(matches!(
            config.validate(),
            Err(CloudError::InvalidAppCheck(_))
        ));
        config.app_check = Some(AppCheckConfig {
            attest_url: "https://example.com/app-check".into(),
        });
        assert!(config.validate().is_ok());
    }

    #[test]
    fn an_old_config_file_reads_as_app_check_off() {
        let config: CloudConfig =
            serde_json::from_str(r#"{"project_id":"rigel-cloud","api_key":"key"}"#).unwrap();
        assert!(config.app_check.is_none());
    }

    #[test]
    fn an_attestation_answer_needs_a_token_and_unix_seconds() {
        let answer: AppCheckAnswer =
            serde_json::from_str(r#"{"token":"t","expires_at":1760961600}"#).unwrap();
        assert_eq!(answer.token, "t");
        assert_eq!(answer.expires_at, 1_760_961_600);
        assert!(serde_json::from_str::<AppCheckAnswer>(r#"{"token":"t"}"#).is_err());
        assert!(
            serde_json::from_str::<AppCheckAnswer>(r#"{"token":"t","expires_at":"1760961600"}"#)
                .is_err(),
            "a string is not the number the contract asks for"
        );
    }

    #[test]
    fn a_project_id_with_a_slash_is_refused_before_it_reaches_a_url() {
        assert!(matches!(
            CloudConfig::new("rigel/cloud", "key"),
            Err(CloudError::InvalidProject(_))
        ));
        assert!(matches!(
            CloudConfig::new("rigel-cloud", "  "),
            Err(CloudError::InvalidApiKey)
        ));
    }

    #[test]
    fn document_ids_are_stable_and_safe_in_a_path() {
        let id = document_id(&record().id);
        assert_eq!(id, document_id(&record().id));
        assert_eq!(id.len(), 64);
        assert!(!id.contains('/'), "{id}");
        assert_ne!(id, document_id("github:acme/skills:skills/other/SKILL.md"));
    }

    #[test]
    fn a_skill_document_decodes_without_a_body() {
        let write = skill_write("rigel-cloud", "uid-1", &record());
        let document = json!({
            "fields": write["update"]["fields"].clone(),
        });
        let decoded = decode_skill(&document).expect("the document should decode");
        assert_eq!(decoded.id, record().id);
        assert_eq!(decoded.name, "pdf");
        assert_eq!(decoded.grade.as_deref(), Some("B"));
        assert_eq!(decoded.tokens, Some(480));
        // The body is never part of the cloud document, so a pull cannot overwrite a local blob.
        assert_eq!(decoded.body_sha, None);
        assert_eq!(decoded.simhash, record().simhash);
    }

    #[test]
    fn a_document_without_an_id_is_skipped_rather_than_guessed() {
        assert!(decode_skill(&json!({ "fields": { "name": { "stringValue": "pdf" } } })).is_none());
        assert!(decode_skill(&json!({})).is_none());
    }

    fn transcript(prompt: &str, reply: &str) -> TurnTranscript {
        TurnTranscript {
            turn: Turn {
                id: "turn-1".to_string(),
                agent_id: "agent-1".to_string(),
                parent_turn_id: None,
                seq: 2,
                status: "completed".to_string(),
                prompt: Some(prompt.to_string()),
                started_at: 1_760_961_600_000,
                completed_at: Some(1_760_961_601_000),
                usage: Some(Usage {
                    input_tokens: 120,
                    output_tokens: 30,
                    cost_micros: 7_500,
                    ..Usage::default()
                }),
                stop_reason: Some("end_turn".to_string()),
            },
            items: vec![
                Item {
                    id: "item-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    seq: 1,
                    kind: "message".to_string(),
                    status: "complete".to_string(),
                    body: json!({ "role": "assistant", "text": reply }),
                    created_at: 1_760_961_600_500,
                    raw: None,
                },
                Item {
                    id: "item-2".to_string(),
                    turn_id: "turn-1".to_string(),
                    seq: 2,
                    kind: "command".to_string(),
                    status: "complete".to_string(),
                    body: json!({ "command": "cargo check --secret-token hunter2" }),
                    created_at: 1_760_961_600_600,
                    raw: None,
                },
            ],
        }
    }

    #[test]
    fn a_turn_document_carries_the_conversation_and_nothing_else() {
        let write = turn_write(
            "rigel-cloud",
            "uid-1",
            "sess-1",
            3,
            &transcript("why is the build slow?", "It compiles twice."),
            None,
        );
        let fields = &write["update"]["fields"];
        assert_eq!(
            write["update"]["name"],
            "projects/rigel-cloud/databases/(default)/documents/users/uid-1/sessions/sess-1/turns/turn-1"
        );
        assert_eq!(fields["prompt"]["stringValue"], "why is the build slow?");
        assert_eq!(fields["reply"]["stringValue"], "It compiles twice.");
        assert_eq!(fields["order"]["integerValue"], "3");
        assert_eq!(fields["cost_micros"]["integerValue"], "7500");
        assert_eq!(fields["started_at"]["integerValue"], "1760961600000");
        assert_eq!(fields["truncated"]["booleanValue"], false);
        assert_eq!(fields["subagent"]["booleanValue"], false);
        // A command's arguments can carry a token, so only assistant prose is published.
        assert!(
            !write.to_string().contains("hunter2"),
            "tool bodies must not reach the cloud"
        );
    }

    #[test]
    fn long_text_is_clipped_and_says_so() {
        let prompt = "p".repeat(MAX_PROMPT_CHARS + 1);
        let reply = "r".repeat(MAX_REPLY_CHARS + 1);
        let write = turn_write("p", "u", "s", 0, &transcript(&prompt, &reply), None);
        let fields = &write["update"]["fields"];
        assert_eq!(
            fields["prompt"]["stringValue"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            MAX_PROMPT_CHARS
        );
        assert_eq!(
            fields["reply"]["stringValue"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            MAX_REPLY_CHARS
        );
        assert_eq!(fields["truncated"]["booleanValue"], true);
    }

    #[test]
    fn a_session_document_names_the_project_but_not_its_path() {
        let session = ProjectSession {
            id: "sess-1".to_string(),
            project_root: "C:\\Users\\someone\\work\\rigel".to_string(),
            title: Some("Fix the build".to_string()),
            status: SessionStatus::Idle,
            budget_cents: Some(500),
            spend_cents: 3,
            spend_micros: 32_000,
            interventions: 0,
            tasks_done: 2,
            created_at: 1_760_961_600_000,
            ended_at: None,
            pinned_at: None,
            archived_at: None,
        };
        let write = session_write("rigel-cloud", "uid-1", &session, 4, 2);
        let fields = &write["update"]["fields"];
        assert_eq!(fields["project"]["stringValue"], "rigel");
        assert_eq!(fields["turn_count"]["integerValue"], "4");
        assert_eq!(fields["agent_count"]["integerValue"], "2");
        assert!(fields.get("project_root").is_none());
        assert!(!write.to_string().contains("Users"));
    }

    #[test]
    fn a_tombstone_is_a_marker_not_a_skill() {
        let write = skill_tombstone_write("rigel-cloud", "uid-1", &record().id);
        let fields = &write["update"]["fields"];
        assert_eq!(fields["deleted"]["booleanValue"], true);
        assert_eq!(fields["id"]["stringValue"], record().id);
        assert!(
            fields.get("name").is_none(),
            "a tombstone must not read as a skill"
        );

        let document = json!({ "fields": fields.clone() });
        assert!(
            decode_skill(&document).is_none(),
            "an older reader skips the marker rather than inventing a row"
        );
        assert_eq!(
            tombstone_id(&document).as_deref(),
            Some(record().id.as_str())
        );

        let live = skill_write("rigel-cloud", "uid-1", &record());
        let live_document = json!({ "fields": live["update"]["fields"].clone() });
        assert_eq!(tombstone_id(&live_document), None);
    }

    #[tokio::test]
    async fn the_manifest_is_scoped_to_one_project_and_account() {
        let dir = tempfile::tempdir().unwrap();
        let config = CloudConfig::new("rigel-cloud", "key").unwrap();
        let cloud = Cloud::new_at(
            dir.path(),
            config,
            Endpoints::same("http://127.0.0.1:1"),
            Arc::new(MemorySessionStore::new()),
        )
        .unwrap();
        assert!(cloud.load_state("uid-1").unwrap().skills.is_empty());

        let mut state = CloudState {
            project_id: "rigel-cloud".to_string(),
            uid: "uid-1".to_string(),
            ..CloudState::default()
        };
        state.skills.insert(record().id);
        state.last_push_at = Some(1_760_961_600_000);
        cloud.save_state(&state).unwrap();

        let loaded = cloud.load_state("uid-1").unwrap();
        assert_eq!(loaded.skills.len(), 1);
        assert_eq!(loaded.last_push_at, Some(1_760_961_600_000));
        assert!(
            cloud.load_state("uid-2").unwrap().skills.is_empty(),
            "another account's manifest must not decide what this one deletes"
        );
    }

    #[test]
    fn timestamps_are_rfc3339_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_760_961_600_000), "2025-10-20T12:00:00Z");
        assert_eq!(rfc3339(1_792_454_400_000), "2026-10-20T00:00:00Z");
    }

    #[test]
    fn firebase_codes_become_sentences_a_user_can_act_on() {
        assert_eq!(
            firebase_message(r#"{"error":{"code":400,"message":"INVALID_LOGIN_CREDENTIALS"}}"#),
            "the email or password is wrong"
        );
        assert_eq!(
            firebase_message(r#"{"error":{"message":"PERMISSION_DENIED"}}"#),
            "permission denied: deploy firestore.rules to this project (see docs/cloud.md)"
        );
        assert_eq!(
            firebase_message(r#"{"error":{"message":"EMAIL_EXISTS"}}"#),
            "that email already has an account"
        );
        assert_eq!(
            firebase_message(
                r#"{"error":{"message":"WEAK_PASSWORD : Password should be at least 6 characters"}}"#
            ),
            "Firebase needs a password of at least 6 characters"
        );
        assert_eq!(firebase_message("not json"), "not json");
    }

    #[test]
    fn a_memory_session_never_prints_its_tokens() {
        let store = MemorySessionStore::new();
        store
            .save(&Session {
                uid: "uid-1".into(),
                email: "ada@example.com".into(),
                project_id: "rigel-cloud".into(),
                id_token: "id-secret".into(),
                refresh_token: "refresh-secret".into(),
                expires_at: 1,
                email_verified: false,
            })
            .unwrap();
        let dump = format!("{:?}", store.load().unwrap().unwrap());
        assert!(dump.contains("ada@example.com"));
        assert!(!dump.contains("id-secret"));
        assert!(!dump.contains("refresh-secret"));
    }

    #[test]
    fn a_sign_in_answer_says_whether_the_address_is_verified() {
        let answer: SignInAnswer = serde_json::from_str(
            r#"{"localId":"uid-1","email":"ada@example.com","idToken":"i","refreshToken":"r"}"#,
        )
        .unwrap();
        assert!(
            !answer.email_verified,
            "an omitted claim must not read as verified"
        );

        let answer: SignInAnswer = serde_json::from_str(
            r#"{"localId":"uid-1","email":"ada@example.com","idToken":"i","refreshToken":"r","emailVerified":true}"#,
        )
        .unwrap();
        assert!(answer.email_verified);
    }

    #[test]
    fn a_session_from_before_verification_existed_reads_as_unverified() {
        let session: Session = serde_json::from_str(
            r#"{"uid":"uid-1","email":"ada@example.com","project_id":"rigel-cloud","idToken":"i","refreshToken":"r","expires_at":1}"#,
        )
        .unwrap();
        assert!(!session.email_verified);
        assert!(!CloudStatus::unconfigured().email_verified);
        assert!(CloudStatus::unconfigured().app_check.is_none());
    }
}

//! agentchattr: the chat server behind the Chat tab (P6).
//!
//! After OmniRoute, this is the second local service Rigel starts itself and fronts through
//! the core. The installer ships a sidecar (`$RESOURCE/sidecar`: the pinned agentchattr source
//! and an embeddable Python runtime), so a clean machine needs no Python on PATH; a development
//! checkout can still point at `vendor/agentchattr` (gitignored) instead. Rigel holds *one*
//! relationship to the server: it spawns `run.py`, reads the session token from its stdout, and
//! forwards a narrow slice of its API to the window. The renderer never talks to the chat server
//! directly and never sees the token — the same boundary that keeps the keychain out of the
//! window (§3.2, S2).
//!
//! `RIGEL_AGENTCHAT_URL` overrides the origin; `RIGEL_AGENTCHAT_DIR` points at a checkout when
//! it does not sit next to the open project. `install_sidecar` is how the desktop shell hands
//! the bundled copy over: the source is copied to app data, where the room may write.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;

use super::{obscura, roblox};

/// Where agentchattr listens by default (vendor config.toml `[server] port`).
pub const DEFAULT_ORIGIN: &str = "http://127.0.0.1:8300";

/// How long `ensure_server` waits for a freshly launched chat server to answer.
const STARTUP_ATTEMPTS: u32 = 120;
const STARTUP_INTERVAL: Duration = Duration::from_millis(250);

/// The session token `run.py` prints on startup (in-memory on the server side too —
/// every restart mints a new one, so callers must never cache it across restarts).
static SESSION_TOKEN: Mutex<Option<String>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize)]
pub struct ChatStatus {
    pub running: bool,
    /// True when Rigel holds a working session token for the server. The tab needs it for
    /// everything past the landing page; a server Rigel can neither spawn nor take over reports
    /// `false`.
    pub authed: bool,
    pub origin: String,
    /// The checkout the server would be spawned from, when it is on this machine.
    pub dir: Option<String>,
    /// Present when there is no checkout: exactly what to install and where. The Chat tab shows
    /// this instead of letting Start fail at spawn — a fresh clone has no `vendor/agentchattr`.
    pub setup: Option<String>,
}

/// One harness process Rigel started for the room, and therefore can stop.
///
/// The chat server only knows names, not pids, so the roster cannot tell an agent Rigel owns from
/// one a user started by hand. This is that bookkeeping: the pid to kill, and the configured name
/// the room's `@name` (or `@name-2`) belongs to.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChatProcess {
    pub name: String,
    pub label: String,
    pub pid: u32,
}

/// Live children this process started. A wrapper that exits on its own is removed by its waiter
/// thread, so a stale entry means "Rigel started it and it is still running", which is exactly the
/// condition for offering a Stop button.
static LAUNCHED: Mutex<Vec<ChatProcess>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub id: u64,
    pub sender: String,
    pub text: String,
    /// `HH:MM:SS` as the server stores it; agentchattr owns the clock.
    pub time: String,
    /// `chat` / `join` / `leave` / `system`.
    pub kind: String,
    pub channel: String,
    /// Room uploads the message carries. `url` is the room's own `/uploads/<name>` path; the
    /// renderer never receives an absolute URL or the server origin.
    pub attachments: Vec<ChatAttachment>,
}

/// One file a message carries.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChatAttachment {
    pub name: String,
    pub url: String,
}

fn origin() -> String {
    std::env::var("RIGEL_AGENTCHAT_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_ORIGIN.to_string())
}

/// User-facing name from case 2 of `.mcp.json`: the field is the Rust struct verbatim.
fn token() -> Option<String> {
    SESSION_TOKEN
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .filter(|value| !value.is_empty())
}

fn set_token(new_value: String) {
    if let Ok(mut guard) = SESSION_TOKEN.lock() {
        *guard = Some(new_value);
    }
}

fn clear_token() {
    if let Ok(mut guard) = SESSION_TOKEN.lock() {
        *guard = None;
    }
}

/// The token `run.py` injects into the landing page for the browser tab, taken from that same
/// response. Kept as a function so the shape is testable without a server.
fn landing_token(html: &str) -> Option<String> {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        regex::Regex::new(r#"__SESSION_TOKEN__\s*=\s*"([0-9a-fA-F]{32,128})""#)
            .expect("the landing token pattern is valid")
    });
    pattern.captures(html).map(|capture| capture[1].to_string())
}

/// Take over a room this launch did not spawn: a chat server left behind by a previous window
/// (its child survives the app on Windows) or one started by hand from the same checkout. Its
/// landing page carries the token for the browser tab, so reading it keeps the token in Rust and
/// makes the existing room usable instead of asking the user to kill a working server.
///
/// The token is only accepted when `/api/settings` answers through it, and the page must look
/// like agentchattr; anything else leaves the room unauthenticated.
async fn adopt_token(client: &reqwest::Client) -> bool {
    if token().is_some() {
        return true;
    }
    let Ok(response) = client.get(format!("{}/", origin())).send().await else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    let Ok(html) = response.text().await else {
        return false;
    };
    if !html.contains("agentchattr") {
        return false;
    }
    let Some(found) = landing_token(&html) else {
        return false;
    };
    set_token(found);
    if api_call(reqwest::Method::GET, "/api/settings", None)
        .await
        .is_ok()
    {
        return true;
    }
    clear_token();
    false
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_millis(2_500))
        .connect_timeout(Duration::from_millis(800))
        .build()
        .unwrap_or_default()
}

/// GET anything; the landing page and `/static/*` are the only token-less surface.
async fn healthy(client: &reqwest::Client) -> bool {
    client
        .get(format!("{}/", origin()))
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

/// One authenticated API call. The token replaces the browser cookie the vendor UI uses.
async fn api_call(
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let url = format!("{}{}", origin(), path);
    let token = token().ok_or({
        "the chat server is up but exposed no session token — stop it and start the room from the Chat tab"
    })?;
    let mut request = http_client()
        .request(method, &url)
        .header("x-session-token", token);
    if let Some(value) = body
        && value != Value::Null
    {
        request = request.json(&value);
    }
    request
        .send()
        .await
        .map_err(|error| format!("chat server call failed: {error}"))?
        .json::<Value>()
        .await
        .map_err(|error| format!("chat server returned unreadable data: {error}"))
}

fn parse_message(value: &Value) -> Option<ChatMessage> {
    Some(ChatMessage {
        id: value.get("id")?.as_u64()?,
        sender: value.get("sender")?.as_str()?.to_string(),
        text: value
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        time: value
            .get("time")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        kind: value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("chat")
            .to_string(),
        channel: value
            .get("channel")
            .and_then(Value::as_str)
            .unwrap_or("general")
            .to_string(),
        attachments: parse_attachments(value.get("attachments")),
    })
}

fn parse_messages(value: Value) -> Vec<ChatMessage> {
    match value {
        Value::Array(rows) => rows.iter().filter_map(parse_message).collect(),
        _ => Vec::new(),
    }
}

/// The largest attachment the window will draw, before base64 inflates it.
const ATTACHMENT_CAP: usize = 8 * 1024 * 1024;

/// A room upload path, or a refusal. Attachments are plain files under `/uploads/`: no scheme, no
/// traversal, no nested path, so the core can never be asked to fetch somewhere else.
fn attachment_path(url: &str) -> Result<&str, String> {
    let path = url.trim();
    let Some(name) = path.strip_prefix("/uploads/") else {
        return Err("an attachment must be a room upload, not an arbitrary URL".to_string());
    };
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("the attachment name is not a plain file".to_string());
    }
    Ok(path)
}

/// Attachments as the room stores them (`name`, `url`), dropping anything that is not a plain
/// upload path. A message with no files is the common case and costs one `None` check.
fn parse_attachments(value: Option<&Value>) -> Vec<ChatAttachment> {
    let Some(rows) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let path = attachment_path(row.get("url")?.as_str()?).ok()?;
            let name = row
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path));
            Some(ChatAttachment {
                name: name.to_string(),
                url: path.to_string(),
            })
        })
        .collect()
}

/// The content types an `<img>` can draw here. A PDF or a zip stays a named chip: there is no
/// renderer for it, and guessing a type would widen what the webview trusts.
fn image_mime(name: &str) -> Option<&'static str> {
    match name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "avif" => Some("image/avif"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

/// Standard base64 with padding. Small enough to keep here rather than add a dependency for one
/// data URL.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        let packed = ((chunk[0] as u32) << 16) | ((second as u32) << 8) | third as u32;
        out.push(TABLE[(packed >> 18) as usize & 63] as char);
        out.push(TABLE[(packed >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(packed >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[packed as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// One room upload as a `data:` URL, so the window can draw it without seeing the session token
/// or the server's origin. Images only; the size cap is checked before and after the body.
pub async fn attachment(url: &str) -> Result<String, String> {
    let path = attachment_path(url)?;
    let mime =
        image_mime(path).ok_or_else(|| "only images are drawn in a chat message".to_string())?;
    let token = token().ok_or({
        "the chat server is up but exposed no session token — stop it and start the room from the Chat tab"
    })?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_millis(800))
        .build()
        .map_err(|error| format!("could not build the attachment client: {error}"))?;
    let response = client
        .get(format!("{}{}", origin(), path))
        .header("x-session-token", token)
        .send()
        .await
        .map_err(|error| format!("attachment fetch failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "the room answered {} for that attachment",
            response.status()
        ));
    }
    if response.content_length().unwrap_or(0) > ATTACHMENT_CAP as u64 {
        return Err("that attachment is larger than 8 MB".to_string());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("attachment body failed: {error}"))?;
    if bytes.len() > ATTACHMENT_CAP {
        return Err("that attachment is larger than 8 MB".to_string());
    }
    Ok(format!("data:{mime};base64,{}", base64(&bytes)))
}

/* -------------------------------- sidecar -------------------------------- */

/// A built sidecar: the read-only install copy and the writable copy the room runs from.
///
/// `resources/sidecar/` ships in the installer and never changes on disk. agentchattr writes
/// `data/`, `config.local.toml` and its JSONL beside its own source, so the shell copies the
/// source into app data once and leaves `python/` where the installer put it.
#[derive(Debug, Clone)]
struct Sidecar {
    /// `$RESOURCE/sidecar`: `agentchattr/` and `python/`.
    source: PathBuf,
    /// `<app data>/agentchat`: the copy `run.py`, the wrappers and the room read and write.
    dir: PathBuf,
}

static SIDECAR: Mutex<Option<Sidecar>> = Mutex::new(None);
/// The project root last written into the copied `config.toml`; a poll loop must not rewrite it.
static SIDECAR_CWD: Mutex<Option<String>> = Mutex::new(None);

fn sidecar() -> Option<Sidecar> {
    SIDECAR.lock().ok().and_then(|guard| guard.clone())
}

/// The build marker the sidecar script writes, falling back to the vendor `VERSION`. A change
/// here is what makes the app-data copy refresh.
fn source_version(source: &Path) -> String {
    ["agentchattr/.rigel-sidecar", "agentchattr/VERSION"]
        .iter()
        .find_map(|file| {
            std::fs::read_to_string(source.join(file))
                .ok()
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
        })
        .unwrap_or_else(|| "unversioned".to_string())
}

/// Copy a tree, leaving the room's own state alone, then remove files the source no longer has.
fn copy_tree(source: &Path, target: &Path, keep: &[&str]) -> Result<(), String> {
    std::fs::create_dir_all(target)
        .map_err(|error| format!("could not create {}: {error}", target.display()))?;
    let mut seen = Vec::new();
    copy_dir(source, target, "", keep, &mut seen)?;
    prune_dir(target, "", &seen, keep)?;
    Ok(())
}

fn preserved(relative: &str, keep: &[&str]) -> bool {
    relative == "data" || relative.starts_with("data/") || keep.contains(&relative)
}

fn copy_dir(
    source: &Path,
    target: &Path,
    prefix: &str,
    keep: &[&str],
    seen: &mut Vec<String>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(source)
        .map_err(|error| format!("could not read {}: {error}", source.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("could not read {}: {error}", source.display()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if preserved(&relative, keep) {
            continue;
        }
        let from = entry.path();
        let to = target.join(&name);
        if from.is_dir() {
            std::fs::create_dir_all(&to)
                .map_err(|error| format!("could not create {}: {error}", to.display()))?;
            copy_dir(&from, &to, &relative, keep, seen)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|error| format!("could not copy {}: {error}", from.display()))?;
            seen.push(relative);
        }
    }
    Ok(())
}

fn prune_dir(target: &Path, prefix: &str, seen: &[String], keep: &[&str]) -> Result<(), String> {
    let entries = match std::fs::read_dir(target) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if preserved(&relative, keep) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            prune_dir(&path, &relative, seen, keep)?;
            let empty = std::fs::read_dir(&path)
                .map(|mut rows| rows.next().is_none())
                .unwrap_or(false);
            if empty {
                let _ = std::fs::remove_dir(&path);
            }
        } else if !seen.iter().any(|known| known == &relative) {
            std::fs::remove_file(&path)
                .map_err(|error| format!("could not remove stale {}: {error}", path.display()))?;
        }
    }
    Ok(())
}

/// Copy the bundled sidecar into app data, refreshing it only when the pinned build changed.
///
/// Called once by the desktop shell at startup. `data/` and `config.local.toml` are the room's
/// own files and survive an upgrade; everything else is source and is replaced.
pub fn install_sidecar(
    source: impl Into<PathBuf>,
    data: impl Into<PathBuf>,
) -> Result<String, String> {
    let source = source.into();
    let dir = data.into();
    // `copy_tree` prunes anything the source does not have. An index root contains `index.db`,
    // `api.json`, settings and blob directories, so installing there would delete user data.
    if dir.join("index.db").exists() || dir.join("api.json").exists() {
        return Err(format!(
            "refusing to install the chat sidecar into {}: that is Rigel's data root, not its agentchat directory",
            dir.display()
        ));
    }
    if !source.join("agentchattr/run.py").is_file() {
        return Err(format!(
            "the sidecar at {} is incomplete (no agentchattr/run.py); build it with \
             `python scripts/sidecar/build-agentchat.py`",
            source.display()
        ));
    }

    let version = source_version(&source);
    let marker = dir.join(".rigel-sidecar");
    let installed = std::fs::read_to_string(&marker)
        .unwrap_or_default()
        .trim()
        .to_string();
    if installed != version {
        copy_tree(
            &source.join("agentchattr"),
            &dir,
            &["config.local.toml", ".rigel-sidecar"],
        )?;
        std::fs::create_dir_all(dir.join("data"))
            .map_err(|error| format!("could not create {}: {error}", dir.join("data").display()))?;
        std::fs::write(&marker, format!("{version}\n"))
            .map_err(|error| format!("could not write {}: {error}", marker.display()))?;
    }

    if let Ok(mut guard) = SIDECAR.lock() {
        *guard = Some(Sidecar {
            source,
            dir: dir.clone(),
        });
    }
    if let Ok(mut guard) = SIDECAR_CWD.lock() {
        *guard = None;
    }
    Ok(dir.display().to_string())
}

/// The sidecar shipped beside this executable. The Windows installer puts it at `sidecar/`; a
/// Tauri bundle can also preserve it under `resources/sidecar`.
fn packaged_sidecar_dir(root: &Path) -> Option<PathBuf> {
    ["sidecar", "resources/sidecar"]
        .iter()
        .map(|relative| root.join(relative))
        .find(|candidate| candidate.join("agentchattr/run.py").is_file())
}

/// Adopt the sidecar installed beside this executable into `<index_dir>/agentchat`.
///
/// The API can start before the desktop shell has installed its sidecar. If that happened and it
/// kept using `vendor/agentchattr`, the room would write `*_queue.jsonl` under the app-data copy
/// while the wrappers watched the checkout's `data/` — the room would show every agent online and
/// none of them would ever receive an assignment. Pointing both at the same app-data copy is what
/// makes `POST /v1/room/agents/start` actually wake the crew.
///
/// The join is deliberate: `install_sidecar` replaces source files and prunes everything else,
/// so handing it the index root would delete Rigel's database and settings.
pub fn use_installed_sidecar(index_dir: impl Into<PathBuf>) -> Option<String> {
    let root = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let source = packaged_sidecar_dir(&root)?;
    install_sidecar(source, index_dir.into().join("agentchat")).ok()
}

/// The crew inherits `cwd` from `config.toml`. The vendor checkout sits inside the project, so
/// its `..` is the project root; the app-data copy is not, so Rigel writes the open project into
/// the copied file. `config.local.toml` still adds agents; only `cwd` is managed here.
fn point_agents_at(dir: &Path, project_root: Option<&str>) {
    let Some(root) = project_root.map(str::trim).filter(|root| !root.is_empty()) else {
        return;
    };
    if SIDECAR_CWD
        .lock()
        .map(|guard| guard.as_deref() == Some(root))
        .unwrap_or(false)
    {
        return;
    }
    if rewrite_agent_cwd(&dir.join("config.toml"), Path::new(root)).is_ok()
        && let Ok(mut guard) = SIDECAR_CWD.lock()
    {
        *guard = Some(root.to_string());
    }
}

/// Rewrite every `[agents.*] cwd` to the project root, adding the key where the vendor table had
/// none. Line-based like the reader: the file is a flat table list, and comments must survive.
fn rewrite_agent_cwd(path: &Path, root: &Path) -> Result<bool, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let value = format!(
        "cwd = \"{}\"",
        root.display()
            .to_string()
            .replace('\\', "/")
            .replace('"', "\\\"")
    );

    let mut out = String::with_capacity(text.len() + 64);
    let mut in_agents = false;
    let mut has_cwd = true;
    let mut changed = false;
    for line in text.lines() {
        let trimmed = line.trim();
        let header = trimmed
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            .map(str::trim);
        if header.is_some_and(|name| name.starts_with("agents.")) {
            if in_agents && !has_cwd {
                out.push_str(&value);
                out.push('\n');
                changed = true;
            }
            in_agents = true;
            has_cwd = false;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if header.is_some() {
            if in_agents && !has_cwd {
                out.push_str(&value);
                out.push('\n');
                changed = true;
            }
            in_agents = false;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let is_cwd = trimmed
            .strip_prefix("cwd")
            .map(str::trim_start)
            .is_some_and(|rest| rest.starts_with('='));
        if in_agents && is_cwd {
            out.push_str(&value);
            out.push('\n');
            has_cwd = true;
            changed |= trimmed != value;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if in_agents && !has_cwd {
        out.push_str(&value);
        out.push('\n');
        changed = true;
    }
    if !changed {
        return Ok(false);
    }
    std::fs::write(path, out)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    Ok(true)
}

/// Where the chat source sits. A built sidecar wins; the project guess covers a dev checkout.
pub fn dir_for(project_root: Option<&str>) -> Option<PathBuf> {
    if let Ok(from_env) = std::env::var("RIGEL_AGENTCHAT_DIR") {
        let candidate = PathBuf::from(from_env.trim());
        if candidate.join("run.py").exists() {
            return Some(candidate);
        }
    }
    if let Some(sidecar) = sidecar() {
        point_agents_at(&sidecar.dir, project_root);
        return Some(sidecar.dir);
    }
    let project = project_root?;
    for candidate in ["vendor/agentchattr", "agentchattr"] {
        let candidate = Path::new(project).join(candidate);
        if candidate.join("run.py").exists() {
            return Some(candidate);
        }
    }
    None
}

pub async fn status(project_root: Option<&str>) -> ChatStatus {
    let origin = origin();
    let client = http_client();
    let running = healthy(&client).await;

    let mut authed = false;
    if running {
        if token().is_none() {
            adopt_token(&client).await;
        }
        authed = api_call(reqwest::Method::GET, "/api/settings", None)
            .await
            .is_ok();
        // A room restarted outside Rigel keeps its data but mints a new token; an old token that
        // no longer answers is replaced by the one the new landing page carries.
        if !authed {
            clear_token();
            authed = adopt_token(&client).await;
        }
    }

    let dir = dir_for(project_root);
    ChatStatus {
        running,
        authed,
        origin,
        setup: dir.is_none().then(|| setup_help(project_root)),
        dir: dir.map(|path| path.display().to_string()),
    }
}

/// A fresh clone has no `vendor/agentchattr` (it is gitignored), so the tab teaches the install
/// instead of failing at spawn. Kept beside `dir_for` so every path named here is one the lookup
/// actually uses.
fn setup_help(project_root: Option<&str>) -> String {
    let checkout = project_root
        .map(|root| {
            format!(
                "{}/vendor/agentchattr",
                root.trim_end_matches(['/', std::path::MAIN_SEPARATOR])
            )
        })
        .unwrap_or_else(|| "<project>/vendor/agentchattr".to_string());
    format!(
        "no chat sidecar in this build and no agentchattr checkout found. Developers: run
         `python scripts/sidecar/build-agentchat.py` once — it vendors the pinned source and an
         embeddable Python runtime, and the app copies that to its data directory. Or clone
         https://github.com/bcurts/agentchattr into \"{checkout}\" and set RIGEL_AGENTCHAT_DIR;
         the env override wins over both."
    )
}

/// Serialises cold starts: the window's boot and a tab action can both see a dead server,
/// and only one of them should spawn a process.
fn start_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// Health first, launch second — the OmniRoute contract. Spawning is what earns Rigel the
/// session token; a room that is already up is taken over through its landing page, so a server
/// orphaned by a previous window does not force the user to kill it by hand.
pub async fn ensure_server(project_root: Option<&str>) -> Result<ChatStatus, String> {
    let _starting = start_lock().lock().await;
    let client = http_client();
    if healthy(&client).await {
        return Ok(status(project_root).await);
    }

    let dir = dir_for(project_root).ok_or_else(missing_checkout)?;
    ensure_project_mcps(project_root)?;

    spawn_server(&dir)?;

    for _ in 0..STARTUP_ATTEMPTS {
        if healthy(&client).await {
            return Ok(status(project_root).await);
        }
        tokio::time::sleep(STARTUP_INTERVAL).await;
    }

    Err(format!(
        "the chat server was spawned at {} but did not answer after boot. \
         Run `python run.py` there to see the failure, and check that port 8300 is free.",
        dir.display()
    ))
}

fn bundled_python(source: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    let candidate = source.join("python/python.exe");
    #[cfg(not(windows))]
    let candidate = source.join("python/bin/python3");
    candidate.is_file().then_some(candidate)
}

/// `pythonw.exe` is the same interpreter with a GUI subsystem: Windows cannot give it a console
/// window, even if the parent's launch flags would allow one. The chat server never needs a
/// console; the harness wrappers still use `python.exe` because they inject keystrokes through
/// their hidden console.
#[cfg(windows)]
fn bundled_python_windowless(source: &Path) -> Option<PathBuf> {
    let candidate = source.join("python/pythonw.exe");
    candidate.is_file().then_some(candidate)
}

#[cfg(not(windows))]
fn bundled_python_windowless(source: &Path) -> Option<PathBuf> {
    bundled_python(source)
}

/// The interpreter for the server itself. A bundled sidecar has `pythonw.exe`; a developer
/// checkout falls back to the ordinary interpreter.
fn server_python(dir: &Path) -> Option<PathBuf> {
    if let Some(sidecar) = sidecar().filter(|sidecar| sidecar.dir == dir)
        && let Some(python) = bundled_python_windowless(&sidecar.source)
    {
        return Some(python);
    }
    chat_python(dir)
}

/// A developer checkout's `.venv` first, then the runtime that ships with the app, then PATH.
/// The bundled interpreter only runs the bundled copy: a foreign checkout has its own pins.
fn chat_python(dir: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    let venv_python = dir.join(".venv/Scripts/python.exe");
    #[cfg(not(windows))]
    let venv_python = dir.join(".venv/bin/python");

    if venv_python.exists() {
        return Some(venv_python);
    }
    if let Some(sidecar) = sidecar().filter(|sidecar| sidecar.dir == dir)
        && let Some(python) = bundled_python(&sidecar.source)
    {
        return Some(python);
    }
    which("python").or_else(|| which("python3"))
}

/// PATH lookup that keeps the `.exe` detail out of the caller (Windows never bare-executes).
fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        #[cfg(windows)]
        // npm installs `muse` as `muse.cmd`; a `.cmd` shim is still a runnable CLI here because
        // the wrapper launches it through `cmd.exe`.
        for suffix in [".exe", ".cmd", ".bat", ".com", ""] {
            let candidate = dir.join(if program.ends_with(".exe") {
                program.to_string()
            } else {
                format!("{program}{suffix}")
            });
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        #[cfg(not(windows))]
        {
            let candidate = dir.join(program);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn spawn_server(dir: &std::path::Path) -> Result<(), String> {
    let python = server_python(dir).ok_or("no python found on PATH to boot the chat server")?;
    let mut command = std::process::Command::new(python);
    command
        .arg("run.py")
        .current_dir(dir)
        // The token is one printed line. pythonw is a GUI process, so keep stdout unbuffered
        // long enough for the reader thread to see that line.
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    crate::process::hide(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start the chat server ({error})"))?;

    // The banner (and with it the session token) is a one-time print on stdout. A thread
    // reaps every line so the pipe never fills, and the token lands in the static when seen.
    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(token) = line.trim().strip_prefix("Session token: ") {
                    set_token(token.trim().to_string());
                }
            }
        });
    }
    // On a clean shutdown the OS reaps; a stopped service is detected by health, not pid.
    std::mem::forget(child);
    Ok(())
}

/// The channel list the room_settings carry. `general` always exists server-side.
pub async fn channels() -> Result<Vec<String>, String> {
    let settings = api_call(reqwest::Method::GET, "/api/settings", None).await?;
    Ok(settings
        .get("channels")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| vec!["general".to_string()]))
}

/// Per-agent presence for the roster pills (`available`, `busy`, `label`, `color`, `role`).
/// Kept as raw JSON: the vendor server owns the shape, and the tab renders it read-only.
pub async fn agents() -> Result<Value, String> {
    api_call(reqwest::Method::GET, "/api/status", None).await
}

/// The names currently online in the room. Used to tell a native agent who else it can address,
/// not just the agents that happen to have an assignment on the work board.
pub async fn online() -> Result<Vec<String>, String> {
    Ok(online_names(&agents().await?))
}

/// Recent history, or everything newer than `since_id` for the poller.
pub async fn messages(
    channel: Option<&str>,
    since_id: u64,
    limit: u64,
) -> Result<Vec<ChatMessage>, String> {
    let mut path = if since_id > 0 {
        format!("/api/messages?since_id={since_id}&limit={limit}")
    } else {
        format!("/api/messages?limit={limit}")
    };
    if let Some(name) = channel
        && !name.is_empty()
    {
        path.push_str(&format!("&channel={}", urlencoded(name)?));
    }
    Ok(parse_messages(
        api_call(reqwest::Method::GET, &path, None).await?,
    ))
}

fn urlencoded(value: &str) -> Result<String, String> {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(encoded)
}

/// The room name a native agent posts under: its role plus the first characters of its agent
/// id, e.g. `reader-9b1c`. The id suffix is what keeps two `reader` children in one session
/// distinguishable; a room agent keeps its own registered name.
pub fn native_name(agent: &crate::session::Agent) -> String {
    native_name_from(agent.role.as_deref(), &agent.id)
}

pub fn native_name_from(role: Option<&str>, id: &str) -> String {
    let role = role.unwrap_or("agent").trim().to_ascii_lowercase();
    let role: String = role
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .take(24)
        .collect();
    let role = if role.is_empty() {
        "agent".to_string()
    } else {
        role
    };
    let short: String = id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(4)
        .collect();
    if short.is_empty() {
        role
    } else {
        format!("{role}-{short}")
    }
}

/// One websocket frame: the vendor browser sends user messages over `/ws`, there is no REST
/// path for them (the REST send is per-agent-token only), so the core owns a short-lived
/// socket per message. `sender: "user"` matches the vendor UI's default identity; native agents
/// use [`send_as`] so the room shows who spoke.
pub async fn send(text: &str, channel: &str) -> Result<(), String> {
    send_as(text, channel, "user").await
}

/// Post as a native agent. The room accepts any sender string; the name is still validated here
/// because a blank or `system` sender would be indistinguishable from the server's own notices.
pub async fn send_as(text: &str, channel: &str, sender: &str) -> Result<(), String> {
    let sender = sender.trim();
    if sender.is_empty() || sender.eq_ignore_ascii_case("system") {
        return Err("a chat sender must be a real name, not blank or `system`".to_string());
    }
    if sender.chars().count() > 40 {
        return Err("a chat sender name is limited to 40 characters".to_string());
    }
    let token = token().ok_or({
        "the chat server is up but exposed no session token — stop it and start the room from the Chat tab"
    })?;
    let url = origin().replacen("http", "ws", 1);
    let ws_url = format!("{url}/ws?token={token}");

    let (mut socket, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(|error| format!("chat websocket refused: {error}"))?;

    let frame = serde_json::json!({
        "type": "message",
        "sender": sender,
        "channel": channel,
        "text": text,
    });
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            frame.to_string(),
        ))
        .await
        .map_err(|error| format!("chat send failed: {error}"))?;

    // Wait for the broadcast echo so a refused socket cannot fake a success.
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(Ok(message)) = socket.next().await {
            if message.is_text() || message.is_binary() {
                break;
            }
        }
    })
    .await;

    let _ = socket.close(None).await;
    Ok(())
}

/* ------------------------- agents, without windows ------------------------ */

/// What a harness needs before it can answer a room prompt.
///
/// Installation alone is not enough: Kimi, Kilo, CodeBuddy and Copilot all install cleanly while
/// refusing a prompt until they are signed in. The readiness surfaces report this state from a
/// real, bounded headless probe instead of calling every installed binary ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessStatus {
    /// Configured and installed, but not probed yet. Internal launch paths use this state; the
    /// readiness surfaces replace it with the result of [`probed_harnesses`].
    Unknown,
    /// A real headless prompt completed and returned the probe token.
    Usable,
    /// The configured command is not on PATH.
    Missing,
    /// An API agent's configured provider key is not in the environment.
    MissingKey,
    /// The CLI ran but asked for a login, subscription or API key.
    Unauthenticated,
    /// The probe could not reach a verdict: spawn failure, a non-zero exit without a reply, or
    /// no probe reply at all. A timeout is deliberately *not* this state: see [`ProbeTimeout`].
    ProbeFailed,
    /// The CLI was still starting when the probe's full budget (and one retry) ran out. This says
    /// nothing about the account: reporting it as a credential failure would turn a slow cold
    /// start into a false sign-in requirement.
    ProbeTimeout,
    /// The provider refused the turn because its account cannot bill it (billing disabled,
    /// prepayment credits depleted, HTTP 402). The CLI itself is installed and authenticated;
    /// the fix is a billing action on the provider's site, not a Rigel login.
    ProviderBlocked,
}

/// One harness the vendor checkout can wrap, read from its own `config.toml`.
///
/// Both start scripts and this module exist because agentchattr's Windows wrapper drives the
/// CLI's TUI through the console it was started on (`WriteConsoleInput` against
/// `GetStdHandle(STD_INPUT_HANDLE)`), so an agent always needs *a* console — it just does not
/// need a window on screen.
#[derive(Debug, Clone, Serialize)]
pub struct ChatHarness {
    pub name: String,
    pub label: String,
    /// `type = "api"`: an HTTP agent on a local model (`wrapper_api.py`), no CLI and no TUI.
    pub api: bool,
    /// Kept for clients of the old shape: true only when this harness is currently usable. The
    /// raw configuration view can be true before probing; `configured` and the API probe first.
    pub ready: bool,
    pub status: HarnessStatus,
    /// The exact CLI line (or probe outcome) behind `status`, never a generic guess.
    pub evidence: String,
    /// The command a user must run, copied from the CLI's own diagnostic where possible.
    pub remediation: Option<String>,
    /// The credential source the CLI said it accepts.
    pub credential: Option<String>,
}

/// The room name Rigel manages for Free mode. It is written to `config.local.toml` on every
/// swarm start so the roster always sees the model Free mode actually chose.
pub const FREE_AGENT_NAME: &str = "rigel-free";

/// The environment variable the wrapper reads; the value is passed only to the spawned child.
pub const FREE_AGENT_API_KEY_ENV: &str = "RIGEL_FREE_API_KEY";

/// What to launch for Free mode. `api_key` is never serialized or logged.
#[derive(Clone)]
pub struct FreeAgent {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl std::fmt::Debug for FreeAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FreeAgent")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

/// Strip a `#` comment, leaving quoted values alone (`color = "#da7756"` is not a comment).
fn without_comment(line: &str) -> &str {
    let mut quoted = false;
    for (index, ch) in line.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..index],
            _ => {}
        }
    }
    line
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

/// `[agents.*]` tables as `(name, key → value)`. Line-based on purpose: three keys do not
/// justify a TOML dependency, and the vendor file is a flat table list.
type AgentTable = (String, Vec<(String, String)>);

fn parse_agents(text: &str) -> Vec<AgentTable> {
    let mut tables: Vec<AgentTable> = Vec::new();
    // `strip_env` is the one array-shaped value in the vendor file. Keep reading until its
    // closing bracket instead of dropping every continuation line, which is what made the
    // readiness probe inherit variables the room itself removes.
    let mut pending: Option<(String, String)> = None;
    for raw in text.lines() {
        let line = without_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some((key, mut value)) = pending.take() {
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(line);
            if line.contains(']') {
                if let Some((_, pairs)) = tables.last_mut() {
                    pairs.push((key, value));
                }
            } else {
                pending = Some((key, value));
            }
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            if let Some(name) = section.trim().strip_prefix("agents.") {
                tables.push((unquote(name), Vec::new()));
            } else {
                // Any other table ends the agent table above it.
                tables.push((String::new(), Vec::new()));
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = unquote(value);
        if let Some((_, pairs)) = tables.last_mut() {
            if value.trim_start().starts_with('[') && !value.contains(']') {
                pending = Some((key, value));
            } else {
                pairs.push((key, value));
            }
        }
    }
    if let Some((key, value)) = pending
        && let Some((_, pairs)) = tables.last_mut()
    {
        pairs.push((key, value));
    }
    tables.retain(|(name, _)| !name.is_empty());
    tables
}

/// The quoted members of a `strip_env = [...]` value.
fn env_list(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(unquote)
        .filter(|item| !item.is_empty())
        .collect()
}

/// Rewrite the managed Free-mode table in `config.local.toml`, leaving every user table and
/// comment alone. The key is not written here: the wrapper reads it from the child's environment.
pub fn install_free_agent(dir: &Path, free: &FreeAgent) -> Result<(), String> {
    let path = dir.join("config.local.toml");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut out = remove_agent_table(&text, FREE_AGENT_NAME);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!(
        "\n[agents.{FREE_AGENT_NAME}]\n\
         type = \"api\"\n\
         base_url = \"{}\"\n\
         model = \"{}\"\n\
         api_key_env = \"{FREE_AGENT_API_KEY_ENV}\"\n\
         label = \"Rigel Free\"\n\
         color = \"#6c8cff\"\n\
         system_prompt = \"You are Rigel Free, a careful coding collaborator in the project's \
         chat room. Read the recent messages, answer only what was asked, and keep replies short.\"\n",
        toml_escape(&free.base_url),
        toml_escape(&free.model),
    ));
    std::fs::write(&path, out)
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Remove one `[agents.*]` table without touching the sections around it.
fn remove_agent_table(text: &str, name: &str) -> String {
    let header = format!("[agents.{name}]");
    let mut out = String::with_capacity(text.len());
    let mut skipping = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            skipping = trimmed == header;
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The harnesses the checkout would wrap, in config order. `config.local.toml` is merged
/// over `config.toml` — the vendor's own precedence for machine-local agents.
pub fn harnesses(dir: &Path) -> Vec<ChatHarness> {
    harness_specs(dir)
        .into_iter()
        .map(|spec| spec.harness)
        .collect()
}

/// Configuration kept beside the serializable harness so the probe can mirror the room's own
/// environment handling without exposing `strip_env` through the API.
#[derive(Clone)]
struct HarnessSpec {
    harness: ChatHarness,
    command: String,
    strip_env: Vec<String>,
}

fn harness_specs(dir: &Path) -> Vec<HarnessSpec> {
    let mut merged: Vec<AgentTable> = Vec::new();
    for file in ["config.toml", "config.local.toml"] {
        let Ok(text) = std::fs::read_to_string(dir.join(file)) else {
            continue;
        };
        for (name, pairs) in parse_agents(&text) {
            match merged.iter_mut().find(|(known, _)| known == &name) {
                Some((_, existing)) => existing.extend(pairs),
                None => merged.push((name, pairs)),
            }
        }
    }

    merged
        .into_iter()
        .map(|(name, pairs)| {
            let field = |key: &str| {
                pairs
                    .iter()
                    .rev()
                    .find(|(known, _)| known == key)
                    .map(|(_, value)| value.clone())
            };
            let api = field("type").as_deref() == Some("api");
            let command = field("command").unwrap_or_else(|| name.clone());
            let api_key_env = field("api_key_env");
            let strip_env = field("strip_env")
                .map(|value| env_list(&value))
                .unwrap_or_default();
            let available = harness_ready(api, &command, api_key_env.as_deref());
            let (status, evidence, remediation, credential) = if api {
                match api_key_env.as_deref() {
                    Some(key) if !available => {
                        let (evidence, remediation) = if key == "MINIMAX_API_KEY" {
                            (
                                format!("{key} is not set and Rigel has no stored MiniMax key"),
                                Some(
                                    "Paste a MiniMax key in Settings → Harnesses, under Keys a harness receives"
                                        .to_string(),
                                ),
                            )
                        } else {
                            (
                                format!("{key} is not set in the environment that launches Rigel"),
                                Some(format!(
                                    "Set {key} in Rigel's environment, then restart the app"
                                )),
                            )
                        };
                        (
                            HarnessStatus::MissingKey,
                            evidence,
                            remediation,
                            Some(key.to_string()),
                        )
                    }
                    _ => (HarnessStatus::Unknown, String::new(), None, None),
                }
            } else if !available {
                (
                    HarnessStatus::Missing,
                    format!("`{command}` was not found on PATH"),
                    Some(format!("Install `{command}` so it resolves on PATH")),
                    None,
                )
            } else {
                (HarnessStatus::Unknown, String::new(), None, None)
            };
            let ready = !matches!(
                status,
                HarnessStatus::Missing | HarnessStatus::MissingKey
            );
            HarnessSpec {
                harness: ChatHarness {
                    label: field("label").unwrap_or_else(|| name.clone()),
                    ready,
                    api,
                    name,
                    status,
                    evidence,
                    remediation,
                    credential,
                },
                command,
                strip_env,
            }
        })
        .collect()
}

const PROBE_PROMPT: &str = "Reply with exactly OK";
/// A cold CLI start still has to load its runtime and reach a provider. Thirty seconds was tight
/// enough that Antigravity, Qwen and Kilo intermittently lost the race while answering normally
/// on the next attempt; the probe now gives every CLI a realistic budget and retries once.
const PROBE_TIMEOUT: Duration = Duration::from_secs(45);
/// The three CLIs observed starting slowly on Windows get a longer first attempt.
const SLOW_PROBE_TIMEOUT: Duration = Duration::from_secs(90);
/// `kilo auth list` only reads local credential state; it does not need a model's budget.
const PROBE_AUTH_TIMEOUT: Duration = Duration::from_secs(20);
/// Ten cold CLI starts at once starve each other on Windows: authenticated Claude, Codex, Qwen
/// and Antigravity probes timed out while Gemini and Copilot survived. Keep two in flight so a
/// real login is not reported as a failure just because the whole roster woke at once.
const PROBE_CONCURRENCY: usize = 2;

/// How long one attempt may take for this CLI. The probe still retries once after a timeout, so
/// the total wall clock can be twice this before the card reports `probe_timeout`.
fn probe_budget(name: &str) -> Duration {
    match name {
        "antigravity" | "qwen" | "kilo" => SLOW_PROBE_TIMEOUT,
        _ => PROBE_TIMEOUT,
    }
}
const READINESS_TTL: Duration = Duration::from_secs(60);

type ReadinessCache = HashMap<PathBuf, (Instant, Vec<ChatHarness>)>;
static READINESS_CACHE: OnceLock<Mutex<ReadinessCache>> = OnceLock::new();
/// Bumped when a keychain write or a forced re-check invalidates the cache. A probe that started
/// before the bump must not publish its answer, or a slow pre-sign-in pass would overwrite the
/// fresh verdict with the stale one it was already carrying.
static READINESS_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// One roster probe at a time. The Chat tab and Settings can both read readiness; without this
/// guard each reader launched its own ten-CLI roster, doubling process pressure and making a
/// healthy CLI look like a timeout. A second reader waits, then reuses the fresh cache entry.
static PROBE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// The cache itself. `OnceLock::get` alone never initializes the slot, so a cache that only ever
/// read through `get` silently missed every time and re-probed the whole roster on each request.
fn readiness_cache() -> &'static Mutex<ReadinessCache> {
    READINESS_CACHE.get_or_init(Mutex::default)
}

/// The cached verdict for one checkout, or `None` when it is absent or past its short lifetime.
fn cached_readiness(dir: &Path) -> Option<Vec<ChatHarness>> {
    let cache = readiness_cache().lock().ok()?;
    let (at, rows) = cache.get(dir)?;
    (at.elapsed() < READINESS_TTL).then(|| rows.clone())
}

fn remember_readiness(dir: &Path, rows: &[ChatHarness], generation: u64) {
    if generation != READINESS_GENERATION.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if let Ok(mut cache) = readiness_cache().lock() {
        cache.insert(dir.to_path_buf(), (Instant::now(), rows.to_vec()));
    }
}

/// Probe each configured harness once and cache the answer briefly. The Chat tab polls its
/// roster, so probing on every poll would spend a model request every few seconds; a minute is
/// short enough that signing in is reflected without restarting Rigel.
fn probed_harnesses(dir: &Path) -> Vec<ChatHarness> {
    if let Some(rows) = cached_readiness(dir) {
        return rows;
    }

    let _probe = PROBE_LOCK.get_or_init(Mutex::default).lock();
    // Another reader may have finished the roster while this one waited for the lock.
    if let Some(rows) = cached_readiness(dir) {
        return rows;
    }

    let generation = READINESS_GENERATION.load(std::sync::atomic::Ordering::SeqCst);
    let specs = harness_specs(dir);
    let mut rows = Vec::with_capacity(specs.len());
    for group in specs.chunks(PROBE_CONCURRENCY) {
        let handles: Vec<_> = group
            .iter()
            .cloned()
            .map(|spec| std::thread::spawn(move || probe_harness(spec)))
            .collect();
        rows.extend(handles.into_iter().filter_map(|handle| handle.join().ok()));
    }

    remember_readiness(dir, &rows, generation);
    rows
}

/// A headless prompt that matches the room's launch as closely as the CLI allows. The prompt is
/// one argv element on purpose: word-splitting it turns a real answer into a spurious failure.
fn probe_args(name: &str) -> Option<Vec<String>> {
    let prompt = PROBE_PROMPT.to_string();
    Some(match name {
        "claude" => vec!["-p".into(), prompt],
        // Codex's room launch clears a stale local proxy override. Probe the same path so a
        // working ChatGPT session is not misreported because an unrelated proxy is down.
        "codex" => vec![
            "exec".into(),
            "-c".into(),
            "openai_base_url=".into(),
            prompt,
        ],
        "gemini" => vec!["-p".into(), prompt, "--skip-trust".into()],
        // Print mode exits on its own; the interactive unattended flag can make older agy
        // builds wait, so the readiness probe uses the plain headless path.
        "antigravity" => vec!["-p".into(), prompt],
        "kimi" | "qwen" | "codebuddy" | "copilot" => vec!["-p".into(), prompt],
        "kilo" => vec!["run".into(), prompt],
        _ => return None,
    })
}

struct ProbeRun {
    output: String,
    code: Option<i32>,
    timed_out: bool,
    error: Option<String>,
}

/// Run one non-interactive probe with no stdin, a bounded wall clock, and the same env scrubbing
/// the room applies. Both streams go to a temp file so a chatty CLI such as Codex cannot fill a
/// pipe and deadlock while we wait for it.
fn run_probe(spec: &HarnessSpec, args: &[String], budget: Duration) -> ProbeRun {
    let program = which(&spec.command).unwrap_or_else(|| PathBuf::from(&spec.command));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let log = std::env::temp_dir().join(format!(
        "rigel-readiness-{}-{}-{stamp}.log",
        std::process::id(),
        spec.harness.name
    ));
    let Ok(writer) = std::fs::File::create(&log) else {
        return ProbeRun {
            output: String::new(),
            code: None,
            timed_out: false,
            error: Some("could not create the probe log".to_string()),
        };
    };
    let Ok(stderr) = writer.try_clone() else {
        let _ = std::fs::remove_file(&log);
        return ProbeRun {
            output: String::new(),
            code: None,
            timed_out: false,
            error: Some("could not clone the probe log handle".to_string()),
        };
    };

    let mut command = std::process::Command::new(&program);
    command
        .args(args)
        .current_dir(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::from(stderr))
        // Never let the readiness probe look like it was started by an interactive Claude.
        .env_remove("CLAUDECODE");
    for key in &spec.strip_env {
        command.env_remove(key);
    }
    let has_auth_token = std::env::var_os("ANTHROPIC_AUTH_TOKEN").is_some();
    for key in harness_env_removals(&spec.harness.name, has_auth_token) {
        command.env_remove(*key);
    }
    // Gemini is the one harness whose room launch injects a key Rigel already stores. A probe
    // that skipped this would report a working room agent as unauthenticated.
    if spec.harness.name == "gemini"
        && std::env::var_os("GEMINI_API_KEY").is_none()
        && let Ok(key) = crate::credentials::get(crate::credentials::GEMINI)
    {
        command.env("GEMINI_API_KEY", key);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = std::fs::remove_file(&log);
            return ProbeRun {
                output: String::new(),
                code: None,
                timed_out: false,
                error: Some(format!("could not start `{}`: {error}", spec.command)),
            };
        }
    };
    let pid = child.id();
    let started = Instant::now();
    let mut code = None;
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                code = status.code();
                break;
            }
            Ok(None) if started.elapsed() < budget => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                timed_out = true;
                let _ = kill_tree(pid);
                let _ = child.wait();
                break;
            }
            Err(error) => {
                let _ = kill_tree(pid);
                let _ = child.wait();
                return ProbeRun {
                    output: String::new(),
                    code: None,
                    timed_out: false,
                    error: Some(format!("could not wait for `{}`: {error}", spec.command)),
                };
            }
        }
    }
    let output = std::fs::read(&log)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    ProbeRun {
        output,
        code,
        timed_out,
        error: None,
    }
}

fn probe_harness(spec: HarnessSpec) -> ChatHarness {
    let mut harness = spec.harness.clone();
    if matches!(
        harness.status,
        HarnessStatus::Missing | HarnessStatus::MissingKey
    ) {
        return harness;
    }
    if harness.api {
        harness.status = HarnessStatus::Usable;
        harness.ready = true;
        harness.evidence = "API agent; no CLI login is required".to_string();
        return harness;
    }
    // Kilo's prompt path can sit on its model banner indefinitely when no provider credential
    // exists. Its own auth command is a fast, explicit signal, so prefer it before spending the
    // bounded prompt timeout.
    if harness.name == "kilo" {
        let check = run_probe(
            &spec,
            &["auth".to_string(), "list".to_string()],
            PROBE_AUTH_TIMEOUT,
        );
        let check_output = clean_output(&check.output);
        let missing = check_output.lines().map(clean_line).find(|line| {
            let line = line.to_ascii_lowercase();
            line.contains("0 credentials") || line.contains("no credentials")
        });
        if let Some(evidence) = missing {
            harness.status = HarnessStatus::Unauthenticated;
            harness.ready = false;
            harness.evidence = evidence;
            harness.remediation = Some("Run `kilo auth login`".to_string());
            harness.credential = Some("Kilo provider credential".to_string());
            return harness;
        }
    }
    let Some(args) = probe_args(&harness.name) else {
        harness.status = HarnessStatus::ProbeFailed;
        harness.ready = false;
        harness.evidence = format!(
            "`{}` is installed, but Rigel has no readiness probe for it",
            spec.command
        );
        return harness;
    };
    let budget = probe_budget(&harness.name);
    let mut run = run_probe(&spec, &args, budget);
    let mut output = clean_output(&run.output);

    // A CLI can print its login diagnostic before it exits or times out; that answer is still
    // the truth and outranks the clock.
    if apply_auth_diagnosis(&mut harness, &output) {
        return harness;
    }
    if apply_provider_block(&mut harness, &output) {
        return harness;
    }
    let answered = run.error.is_none() && run.code == Some(0) && output_has_ok(&output);
    if run.error.is_none() && !answered {
        // Cold starts and provider hiccups can lose one attempt while the CLI is healthy. Give
        // the same harness one more full attempt before publishing a timeout or a hard failure;
        // only an answer, an auth diagnostic, or a second failure becomes the card's state.
        run = run_probe(&spec, &args, budget);
        output = clean_output(&run.output);
    }
    if let Some(error) = run.error {
        harness.status = HarnessStatus::ProbeFailed;
        harness.ready = false;
        harness.evidence = error;
        return harness;
    }
    if apply_auth_diagnosis(&mut harness, &output) {
        return harness;
    }
    if apply_provider_block(&mut harness, &output) {
        return harness;
    }
    if run.timed_out {
        harness.status = HarnessStatus::ProbeTimeout;
        harness.ready = false;
        let tail = last_output_line(&output);
        harness.evidence = if tail.is_empty() {
            format!(
                "probe did not finish within {}s, after two attempts; the CLI printed no output",
                budget.as_secs()
            )
        } else {
            format!(
                "probe did not finish within {}s, after two attempts; last output: {tail}",
                budget.as_secs()
            )
        };
        harness.remediation = Some(format!(
            "Run `{} -p \"{PROBE_PROMPT}\"` in a terminal to see what it is waiting on, then press Check again",
            spec.command
        ));
        if harness.name == "kilo" {
            harness.remediation = Some(format!(
                "Run `kilo run \"{PROBE_PROMPT}\"` in a terminal to see what it is waiting on, fix any provider error it prints, then press Check again"
            ));
        }
        return harness;
    }
    if run.code == Some(0) && output_has_ok(&output) {
        harness.status = HarnessStatus::Usable;
        harness.ready = true;
        harness.evidence = format!("`{PROBE_PROMPT}` exited 0 and replied OK");
        harness.remediation = None;
        harness.credential = None;
        return harness;
    }

    harness.status = HarnessStatus::ProbeFailed;
    harness.ready = false;
    let tail = last_output_line(&output);
    harness.evidence = match run.code {
        Some(code) if tail.is_empty() => {
            format!("prompt probe exited {code} without a reply")
        }
        Some(code) => format!("prompt probe exited {code}: {tail}"),
        None => format!("prompt probe ended without an exit code: {tail}"),
    };
    harness
}

/// Apply a vendor's own sign-in diagnostic to the card, when the captured output contains one.
fn apply_auth_diagnosis(harness: &mut ChatHarness, output: &str) -> bool {
    let Some((evidence, remediation, credential)) = auth_diagnosis(&harness.name, output) else {
        return false;
    };
    harness.status = HarnessStatus::Unauthenticated;
    harness.ready = false;
    harness.evidence = evidence;
    harness.remediation = remediation;
    harness.credential = credential;
    true
}

/// Provider billing refusals are not probe failures. Gemini prints an opaque critical error with
/// `status: 402`; Kilo prints the same refusal as depleted AI Studio prepayment credits. Surface
/// the provider's own line and send the user to the billing page instead of retrying a login.
fn apply_provider_block(harness: &mut ChatHarness, output: &str) -> bool {
    let Some((evidence, remediation)) = provider_billing_diagnosis(output) else {
        return false;
    };
    harness.status = HarnessStatus::ProviderBlocked;
    harness.ready = false;
    harness.evidence = evidence;
    harness.remediation = Some(remediation);
    harness.credential = Some("Provider billing (Google AI Studio)".to_string());
    true
}

fn provider_billing_diagnosis(output: &str) -> Option<(String, String)> {
    let lower = output.to_ascii_lowercase();
    let blocked = lower.contains("prepayment credits are depleted")
        || lower.contains("credits are depleted")
        || lower.contains("status: 402")
        || lower.contains("payment required")
        || lower.contains("billing#prepay");
    if !blocked {
        return None;
    }
    let evidence = output
        .lines()
        .map(clean_line)
        .find(|line| {
            let line = line.to_ascii_lowercase();
            line.contains("402")
                || line.contains("credit")
                || line.contains("billing")
                || line.contains("payment")
        })
        .unwrap_or_else(|| last_output_line(output));
    let url = first_url(output).unwrap_or_else(|| "https://ai.studio/projects".to_string());
    Some((
        evidence,
        format!("Top up or restore billing at {url}, then press Check again"),
    ))
}

/// The CLI's own sign-in diagnostic, in the order the vendors print it. Evidence is the exact
/// line captured from this run; remediation names the command that line says will fix it.
fn auth_diagnosis(name: &str, output: &str) -> Option<(String, Option<String>, Option<String>)> {
    let lower = output.to_ascii_lowercase();
    let evidence = |needles: &[&str]| {
        output
            .lines()
            .map(clean_line)
            .find(|line| {
                let line = line.to_ascii_lowercase();
                needles.iter().any(|needle| line.contains(needle))
            })
            .unwrap_or_else(|| last_output_line(output))
    };

    if name == "kimi"
        && (lower.contains("subscription does not have access")
            || lower.contains("provider.auth_error"))
    {
        let evidence = evidence(&["subscription does not have access", "provider.auth_error"]);
        let url = first_url(output);
        let remediation = match url {
            Some(url) => format!(
                "Upgrade the Kimi Code plan at {url}, then run `kimi` and `/login` if the account is not signed in"
            ),
            None => "Upgrade the Kimi Code plan, then run `kimi` and `/login`".to_string(),
        };
        return Some((
            evidence,
            Some(remediation),
            Some("Kimi Code subscription".to_string()),
        ));
    }
    if name == "kimi" && lower.contains("no model configured") {
        return Some((
            evidence(&["no model configured"]),
            Some("Run `kimi`, then `/login`, and choose a model".to_string()),
            Some("Kimi Code account login".to_string()),
        ));
    }
    if name == "codebuddy"
        && (lower.contains("authentication required") || lower.contains("please use /login"))
    {
        return Some((
            evidence(&["authentication required", "please use /login"]),
            Some("Run `codebuddy`, then `/login`".to_string()),
            Some("CodeBuddy account login".to_string()),
        ));
    }
    if name == "kilo"
        && (lower.contains("you need to sign in")
            || lower.contains("not signed in")
            || lower.contains("run kilo auth login"))
    {
        return Some((
            evidence(&[
                "you need to sign in",
                "not signed in",
                "run kilo auth login",
            ]),
            Some("Run `kilo auth login`".to_string()),
            Some("Kilo provider credential".to_string()),
        ));
    }
    if name == "copilot"
        && (lower.contains("no authentication information found")
            || lower.contains("not logged in"))
    {
        return Some((
            evidence(&["no authentication information found", "not logged in"]),
            Some("Run `copilot login`, or set `COPILOT_GITHUB_TOKEN`".to_string()),
            Some("GitHub Copilot login or COPILOT_GITHUB_TOKEN".to_string()),
        ));
    }
    if lower.contains("gemini_api_key")
        && (lower.contains("must specify") || lower.contains("not set"))
    {
        return Some((
            evidence(&["gemini_api_key"]),
            Some(
                "Set GEMINI_API_KEY or run `rigel keys set gemini` to store it in Rigel"
                    .to_string(),
            ),
            Some("GEMINI_API_KEY".to_string()),
        ));
    }
    if lower.contains("not authenticated")
        || lower.contains("please login")
        || lower.contains("please log in")
        || lower.contains("invalid api key")
    {
        return Some((
            evidence(&[
                "not authenticated",
                "please login",
                "please log in",
                "invalid api key",
            ]),
            Some(format!("Run `{name}` and complete its login")),
            Some(format!("{name} account login")),
        ));
    }
    None
}

fn clean_output(text: &str) -> String {
    text.lines()
        .map(clean_line)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Strip ANSI control sequences from one captured line and keep evidence short enough for a
/// roster tooltip. CLI output is untrusted, so it is only ever displayed as text.
fn clean_line(line: &str) -> String {
    let mut out = String::new();
    let mut escape = false;
    for ch in line.chars() {
        if escape {
            if ch.is_ascii_alphabetic() {
                escape = false;
            }
        } else if ch == '\u{1b}' {
            escape = true;
        } else if !ch.is_control() {
            out.push(ch);
            if out.chars().count() >= 260 {
                out.push('…');
                break;
            }
        }
    }
    out.trim().to_string()
}

fn last_output_line(output: &str) -> String {
    output
        .lines()
        .rev()
        .map(clean_line)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

fn output_has_ok(output: &str) -> bool {
    output.lines().any(|line| {
        let line = line.trim();
        line == "OK" || line.ends_with(" OK")
    })
}

fn first_url(text: &str) -> Option<String> {
    let start = text.find("https://")?;
    let rest = &text[start..];
    let end = rest
        .find(|ch: char| ch.is_whitespace() || matches!(ch, ')' | ']' | ',' | '"'))
        .unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// A CLI must be installed; an API agent must also have its key available, or it would join the
/// roster only to fail every turn. MiniMax is the one API agent whose key Rigel stores itself;
/// the room injects the keychain value into that child at spawn.
fn harness_ready(api: bool, command: &str, api_key_env: Option<&str>) -> bool {
    if api {
        return match api_key_env {
            Some(name) => api_agent_key_present(name),
            None => true,
        };
    }
    which(command).is_some()
}

fn api_agent_key_present(name: &str) -> bool {
    std::env::var_os(name).is_some()
        || (name == "MINIMAX_API_KEY" && crate::credentials::present(crate::credentials::MINIMAX))
}

/// A hidden console cannot answer a permission prompt, so the harnesses that have such prompts
/// start unattended — the same flags `windows/start_*_skip-permissions.bat` pass by hand.
fn unattended_flag(name: &str) -> Option<&'static str> {
    match name {
        "claude" => Some("--dangerously-skip-permissions"),
        "codex" => Some("--dangerously-bypass-approvals-and-sandbox"),
        // agy otherwise stops on its first MCP tool call, so a queued room mention is typed
        // into an approval dialog and never answered.
        "antigravity" => Some("--dangerously-skip-permissions"),
        // Muse Code keeps its OS sandbox; only the interactive approval prompt is turned off.
        "muse" => Some("--disable-approval"),
        _ => None,
    }
}

/// Room-only CLI arguments a harness needs to work unattended. Codex's global config may point
/// at a local proxy that is not running; the room uses the ChatGPT session that is already
/// signed in, and clearing that override is the only way to choose it per launch. The wrapper
/// ignores an `args` key in `config.toml`, so these travel with the argv Rigel builds.
fn room_args(name: &str) -> &'static [&'static str] {
    match name {
        // The update panel is not modal, but the alternate screen makes the first frame race the
        // prompt on cold starts. Linear rendering keeps the composer visible from the beginning.
        "codex" => &["-c", "openai_base_url=", "--no-alt-screen"],
        // A headless room cannot answer Gemini's folder-trust prompt or its per-tool approval.
        // --skip-trust is session-scoped; the folder remains untrusted for interactive runs.
        "gemini" => &["--skip-trust", "--yolo"],
        // Qwen's own /auth choice must remain authoritative. Do not force
        // `qwen-subscription`: that is the Qwen Plus/Pro subscription OAuth path,
        // while Alibaba Cloud Coding Plan is a separate API-key setup. Let Qwen's
        // own /auth selection decide so stored credentials and user-chosen plans work.
        _ => &[],
    }
}

/// The argv an agent is launched with. Pulled out of the spawn so the flags are testable.
fn harness_argv(harness: &ChatHarness) -> Vec<String> {
    let script = if harness.api {
        "wrapper_api.py"
    } else {
        "wrapper.py"
    };
    let mut argv = vec![script.to_string(), harness.name.clone()];
    if let Some(flag) = unattended_flag(&harness.name) {
        argv.push(flag.to_string());
    }
    argv.extend(
        room_args(&harness.name)
            .iter()
            .map(|arg| (*arg).to_string()),
    );
    argv
}

/// A per-launch identity for Concord. The timestamp keeps two instances of the same configured
/// harness distinct; the pid keeps identities distinct across Rigel processes.
fn concord_agent_id(name: &str) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("agentchattr:{name}:{}:{stamp}", std::process::id())
}

fn spawn_harness(dir: &Path, harness: &ChatHarness) -> Result<ChatProcess, String> {
    spawn_harness_env(dir, harness, &[])
}

fn spawn_harness_env(
    dir: &Path,
    harness: &ChatHarness,
    env: &[(&str, String)],
) -> Result<ChatProcess, String> {
    let python = chat_python(dir).ok_or("no python found on PATH to start an agent")?;
    launch_hidden_env(
        &python,
        &harness_argv(harness),
        dir,
        &harness.name,
        &harness.label,
        env,
    )
}

/// Variables the harness must not inherit.
///
/// Interactive Claude refuses to start when both `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN`
/// are set: it prints an approval message and waits, so every room prompt typed into its console
/// is discarded. When the local proxy token is present, drop the conflicting API key for Claude
/// only; other harnesses and a token-less setup are untouched.
fn harness_env_removals(name: &str, has_auth_token: bool) -> &'static [&'static str] {
    if name == "claude" && has_auth_token {
        &["ANTHROPIC_API_KEY"]
    } else {
        &[]
    }
}

/// The CreateProcess half, split from the harness table so a test can drive it with a stub
/// wrapper. Everything that decides whether the console is visible lives here.
fn launch_hidden_env(
    program: &Path,
    argv: &[String],
    dir: &Path,
    name: &str,
    label: &str,
    env: &[(&str, String)],
) -> Result<ChatProcess, String> {
    let mut command = std::process::Command::new(program);
    command
        .args(argv)
        .current_dir(dir)
        // Concord treats this as the session owner. It must be unique per harness process; a
        // shared `.mcp.json` entry would otherwise make every Chat agent the same agent.
        .env("CONCORD_AGENT_ID", concord_agent_id(name))
        .env("CONCORD_AGENT_KIND", "agentchattr")
        // Inherited on purpose: `CREATE_NO_WINDOW` hands the child its own console, and the
        // wrapper injects keystrokes through those inherited handles. Redirecting them would
        // leave the agent deaf.
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    for (key, value) in env {
        command.env(key, value);
    }
    // A CLI that talks to its vendor directly needs the key Rigel already stores. Gemini is the
    // one interactive harness with that requirement; its CLI refuses to start without either an
    // OAuth login or this variable, and the wrapper inherits what we put here.
    if name == "gemini"
        && std::env::var_os("GEMINI_API_KEY").is_none()
        && let Ok(key) = crate::credentials::get(crate::credentials::GEMINI)
    {
        command.env("GEMINI_API_KEY", key);
    }
    // MiniMax's room entry is an API agent (`type = "api"`), not a CLI with its own login.
    // The wrapper reads this variable, so a key stored in Settings has to cross only into that
    // one child. An explicit environment value still wins for a shell or CI run.
    if name == "minimax"
        && std::env::var_os("MINIMAX_API_KEY").is_none()
        && let Ok(key) = crate::credentials::get(crate::credentials::MINIMAX)
    {
        command.env("MINIMAX_API_KEY", key);
    }
    let has_auth_token = std::env::var_os("ANTHROPIC_AUTH_TOKEN").is_some();
    for key in harness_env_removals(name, has_auth_token) {
        command.env_remove(*key);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: a real console with valid handles and no window. `GetConsoleWindow`
        // returns 0 inside it, which is exactly "running, not in view".
        command.creation_flags(0x0800_0000);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start {label} ({error})"))?;
    let pid = child.id();
    let process = ChatProcess {
        name: name.to_string(),
        label: label.to_string(),
        pid,
    };
    if let Ok(mut launched) = LAUNCHED.lock() {
        launched.push(process.clone());
    }

    // Keep the handle and reap it. `mem::forget` used to leak it, which is why there was no Stop:
    // with no waiter, nothing removed the entry when the wrapper died on its own.
    let reaped = process.clone();
    std::thread::spawn(move || {
        let _ = child.wait();
        if let Ok(mut launched) = LAUNCHED.lock() {
            launched.retain(|known| known.pid != reaped.pid);
        }
    });
    Ok(process)
}

/// The harness processes Rigel is holding. The roster uses this to offer Stop only for agents it
/// owns; anything else in the room was started by hand and stays the user's to close.
pub fn launched() -> Vec<ChatProcess> {
    LAUNCHED
        .lock()
        .map(|launched| launched.clone())
        .unwrap_or_default()
}

/// Does `candidate` name the same agent as `configured`? The room registers `claude`, but a
/// second live copy claims `claude-2`, and a stale slot keeps the suffix until it expires.
fn same_agent(candidate: &str, configured: &str) -> bool {
    candidate == configured || candidate.starts_with(&format!("{configured}-"))
}

/// Kill one process and everything it started. The wrapper is the parent of the CLI, which is the
/// parent of whatever the CLI spawned (Codex's MCP proxy, Claude's node children); killing only the
/// wrapper would leave the CLI holding the console, and the room believing the agent is alive.
pub fn kill_tree(pid: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        // `/T` is the whole tree, `/F` is required for a console process that ignores WM_CLOSE.
        let mut command = std::process::Command::new("taskkill");
        command.args(["/PID", &pid.to_string(), "/T", "/F"]);
        crate::process::hide(&mut command);
        let output = command
            .output()
            .map_err(|error| format!("could not run taskkill: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        // Already gone is the outcome we wanted; taskkill reports it as a failure, and its text is
        // localised, so ask tasklist whether the pid still exists instead of reading the message.
        if !pid_running(pid) {
            return Ok(());
        }
        Err(format!("taskkill could not stop pid {pid}: {stderr}"))
    }
    #[cfg(not(windows))]
    {
        let mut command = std::process::Command::new("kill");
        command.args(["-TERM", &pid.to_string()]);
        crate::process::hide(&mut command);
        let output = command
            .output()
            .map_err(|error| format!("could not run kill: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!("kill could not stop pid {pid}"))
        }
    }
}

/// Stop every agent this process started. Called when the window closes: a hidden wrapper with no
/// window and no owner is exactly the orphan pile the 22 Oct QA run had to `taskkill` by hand.
pub fn stop_all() -> usize {
    let ours = launched();
    for process in &ours {
        let _ = kill_tree(process.pid);
    }
    if let Ok(mut launched) = LAUNCHED.lock() {
        launched.clear();
    }
    ours.len()
}

/// Is this pid alive? `tasklist` is present on every supported Windows and prints the pid only for
/// a live process, which keeps the liveness check locale-independent.
#[cfg(windows)]
fn pid_running(pid: u32) -> bool {
    let mut command = std::process::Command::new("tasklist");
    command.args(["/FI", &format!("PID eq {pid}"), "/NH"]);
    crate::process::hide(&mut command);
    command
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
        .unwrap_or(true)
}

/// Stop an agent Rigel started. Refuses what it does not own rather than guessing at a pid.
pub async fn stop_agent(name: &str) -> Result<String, String> {
    let ours = launched();
    let process = ours
        .iter()
        .find(|process| same_agent(name, &process.name))
        .cloned()
        .ok_or_else(|| {
            format!(
                "Rigel did not start `{name}`, so it has no pid to stop — close that agent's own window"
            )
        })?;

    kill_tree(process.pid)?;
    if let Ok(mut launched) = LAUNCHED.lock() {
        launched.retain(|known| known.pid != process.pid);
    }

    // The room frees the slot when the wrapper stops heartbeating, so poll it rather than promise
    // the canonical name is back. A room Rigel cannot authenticate against is not polled at all.
    let client = http_client();
    if !healthy(&client).await {
        return Ok(format!("{} stopped", process.label));
    }
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let present = api_call(reqwest::Method::GET, "/api/status", None)
            .await
            .map(|value| online_names(&value))
            .unwrap_or_default();
        if !present
            .iter()
            .any(|online| same_agent(online, &process.name))
        {
            return Ok(format!(
                "{} stopped; @{} is free again",
                process.label, process.name
            ));
        }
    }
    Ok(format!(
        "{} stopped, but @{} still holds a slot — the room frees it when the heartbeat expires",
        process.label, process.name
    ))
}

/// The names actually present in the room. `/api/status` is a flat map that also carries
/// `paused`, and a crashed slot can linger with `available: false` until its heartbeat expires;
/// neither is an agent a caller can address.
fn online_names(status: &Value) -> Vec<String> {
    status
        .as_object()
        .map(|map| {
            map.iter()
                .filter(|(name, row)| {
                    name.as_str() != "paused"
                        && row.get("available").and_then(Value::as_bool) == Some(true)
                })
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// True when the status object still lists this agent family, even if it is not answering yet.
fn status_has_agent(status: &Value, name: &str) -> bool {
    status
        .as_object()
        .map(|map| {
            map.keys()
                .any(|key| key != "paused" && same_agent(key, name))
        })
        .unwrap_or(false)
}

/// True when this process started the wrapper behind `name` (including a suffixed slot).
pub fn owns(name: &str) -> bool {
    launched()
        .iter()
        .any(|process| same_agent(name, &process.name))
}

/// Restart a wrapper this process owns: kill its tree, wait for the room to free the name, then
/// launch the configured harness again. A wrapper another process started is refused, because
/// this process has no pid for it and killing a guessed pid is worse than reporting the state.
pub async fn restart_agent(project_root: Option<&str>, name: &str) -> Result<String, String> {
    let client = http_client();
    if !healthy(&client).await {
        return Err("the room is not running".to_string());
    }
    let ours = launched();
    let process = ours
        .iter()
        .find(|process| same_agent(name, &process.name))
        .cloned()
        .ok_or_else(|| {
            format!("Rigel did not start `{name}` in this process, so it cannot restart it")
        })?;
    let dir = dir_for(project_root).ok_or_else(missing_checkout)?;
    let harness = harnesses(&dir)
        .into_iter()
        .find(|harness| same_agent(name, &harness.name))
        .ok_or_else(|| format!("`{name}` is no longer configured"))?;
    kill_tree(process.pid)?;

    // The room removes a dead wrapper from `/api/status` immediately, but its registry keeps the
    // slot for the 15 s crash timeout and then reserves the name for a 30 s grace period. A launch
    // during that window registers as `name-2`, which is how the room accumulated duplicate
    // agents. Wait both out (45 s) with margin, then require the status key to be gone as well.
    let stopped = Instant::now();
    let deadline = stopped + Duration::from_secs(70);
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let freed = api_call(reqwest::Method::GET, "/api/status", None)
            .await
            .map(|status| !status_has_agent(&status, name))
            .unwrap_or(false);
        if freed && stopped.elapsed() >= Duration::from_secs(50) {
            break;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{} was stopped but the room still holds @{name}; the slot frees on its heartbeat expiry",
                process.label
            ));
        }
    }

    ensure_project_mcps(project_root)?;
    let started = spawn_harness(&dir, &harness)?;
    Ok(format!("restarted {} (pid {})", started.label, started.pid))
}

/// One automatic restart, kept for the `/v1/room/watchdog` view.
#[derive(Debug, Clone, Serialize)]
pub struct WatchdogRestart {
    pub name: String,
    pub label: String,
    pub at_ms: i64,
    pub outcome: String,
}

/// Detects a harness that has been `busy` for too long without posting a room message.
///
/// Busy alone is not a fault: a real turn can run for minutes and never post until it ends. The
/// clock starts when the agent first appears busy, resets whenever it posts something new, and
/// only then does `stuck_after` mean the wrapper is wedged rather than working.
#[derive(Default)]
pub struct Watchdog {
    busy_since: HashMap<String, Instant>,
    last_id: HashMap<String, u64>,
    cooldown: HashMap<String, Instant>,
    stuck: Vec<String>,
    last_check_ms: i64,
    restarts: VecDeque<WatchdogRestart>,
}

impl Watchdog {
    /// Update the clocks and return the agents that have been silent past the threshold.
    pub fn inspect(
        &mut self,
        status: &Value,
        messages: &[ChatMessage],
        now: Instant,
        stuck_after: Duration,
    ) -> Vec<String> {
        let busy = busy_agents(status);
        self.busy_since.retain(|name, _| busy.contains(name));
        self.last_id.retain(|name, _| busy.contains(name));
        let mut stuck = Vec::new();
        for name in &busy {
            let max_id = messages
                .iter()
                .filter(|message| message.sender == *name)
                .map(|message| message.id)
                .max()
                .unwrap_or(0);
            match self.last_id.get(name).copied() {
                None => {
                    self.last_id.insert(name.clone(), max_id);
                    self.busy_since.insert(name.clone(), now);
                }
                Some(previous) if max_id > previous => {
                    self.last_id.insert(name.clone(), max_id);
                    self.busy_since.insert(name.clone(), now);
                }
                Some(_) => {
                    let since = *self.busy_since.entry(name.clone()).or_insert(now);
                    let cooled = self
                        .cooldown
                        .get(name)
                        .is_none_or(|last| now.duration_since(*last) >= stuck_after);
                    if cooled && now.duration_since(since) >= stuck_after {
                        stuck.push(name.clone());
                    }
                }
            }
        }
        self.stuck = stuck.clone();
        self.last_check_ms = crate::now_ms();
        stuck
    }

    /// Remember the outcome and hold this agent back for one more full threshold, so a restart
    /// that needs a moment to come online is not restarted again immediately.
    pub fn record(&mut self, name: &str, label: &str, outcome: String, now: Instant) {
        self.cooldown.insert(name.to_string(), now);
        self.busy_since.remove(name);
        self.restarts.push_back(WatchdogRestart {
            name: name.to_string(),
            label: label.to_string(),
            at_ms: crate::now_ms(),
            outcome,
        });
        while self.restarts.len() > 20 {
            self.restarts.pop_front();
        }
    }

    pub fn snapshot(&self, stuck_after: Duration, owned: Vec<String>) -> Value {
        serde_json::json!({
            "stuck_after_secs": stuck_after.as_secs(),
            "last_check_ms": self.last_check_ms,
            "stuck": self.stuck,
            "owned": owned,
            "restarts": self.restarts,
        })
    }
}

/// The agents the room reports as both present and busy.
fn busy_agents(status: &Value) -> Vec<String> {
    status
        .as_object()
        .map(|map| {
            map.iter()
                .filter(|(name, row)| {
                    name.as_str() != "paused"
                        && row.get("available").and_then(Value::as_bool) == Some(true)
                        && row.get("busy").and_then(Value::as_bool) == Some(true)
                })
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Run the watchdog for the life of this process. Each process watches only the wrappers it
/// started, because only those pids are known; the window and `rigel serve` therefore each run
/// this and together cover every agent launched through either surface.
pub fn spawn_watchdog<F>(
    state: Arc<Mutex<Watchdog>>,
    project: F,
    stuck_after: Duration,
    interval: Duration,
) where
    F: Fn() -> Option<String> + Send + 'static,
{
    std::thread::Builder::new()
        .name("rigel-room-watchdog".to_string())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            loop {
                let now = Instant::now();
                if let Some(root) = project() {
                    let status = runtime.block_on(agents());
                    let messages = runtime.block_on(messages(None, 0, 100));
                    if let (Ok(status), Ok(messages)) = (status, messages) {
                        let stuck = state
                            .lock()
                            .map(|mut guard| guard.inspect(&status, &messages, now, stuck_after))
                            .unwrap_or_default();
                        for name in stuck {
                            if !owns(&name) {
                                continue;
                            }
                            let label = launched()
                                .into_iter()
                                .find(|process| same_agent(&name, &process.name))
                                .map(|process| process.label)
                                .unwrap_or_else(|| name.clone());
                            let outcome = runtime
                                .block_on(restart_agent(Some(&root), &name))
                                .unwrap_or_else(|error| error);
                            if let Ok(mut guard) = state.lock() {
                                guard.record(&name, &label, outcome, Instant::now());
                            }
                        }
                    }
                }
                std::thread::sleep(interval);
            }
        })
        .ok();
}

/// Start every configured agent that is not already in the room, hidden. Returns the labels
/// launched so the tab can say what just happened.
/// agentchattr merges the project's `.mcp.json` into the harnesses it launches (Claude,
/// Codex, Gemini and Kimi). Adding Rigel b there gives the chat room the same browser as the
/// session roster; an existing entry is the user's and is never overwritten.
fn ensure_obscura(project_root: Option<&str>) -> Result<(), String> {
    let Some(root) = project_root.filter(|root| !root.trim().is_empty()) else {
        return Ok(());
    };
    let root = Path::new(root);
    if !root.is_dir() {
        return Ok(());
    }
    obscura::ensure_project_mcp(root).map(|_| ())
}

/// Add the project-scoped MCP servers a Chat room needs. Rigel b is the browser; Roblox Studio
/// is the official local MCP server when Studio is installed; Concord is the shared work-state
/// layer across harnesses; AgentCAD is the optional CAD tool. Existing user entries are
/// preserved, but a malformed `.mcp.json` is reported rather than silently ignored.
fn ensure_project_mcps(project_root: Option<&str>) -> Result<(), String> {
    ensure_obscura(project_root)?;
    ensure_roblox(project_root)?;
    ensure_concord(project_root)?;
    ensure_agentcad(project_root)
}

/// Studio ships its own MCP server. Register the exact local launch recipe only when it is
/// installed, so a machine without Studio does not get a Chat agent that always fails at spawn.
fn ensure_roblox(project_root: Option<&str>) -> Result<(), String> {
    let Some(root) = project_root.map(str::trim).filter(|root| !root.is_empty()) else {
        return Ok(());
    };
    let root = Path::new(root);
    if !root.is_dir() {
        return Ok(());
    }
    let Some(server) = roblox::mcp_server() else {
        return Ok(());
    };
    ensure_project_server(root, roblox::SERVER_NAME, server)
}

/// Concord needs a unique `CONCORD_AGENT_ID` per live session, so the server entry itself carries
/// only the repository root; `launch_hidden` supplies the per-harness identity. Adding a fixed ID
/// here would make every Chat agent act as the same owner, which Concord's manual setup explicitly
/// warns against. The package is resolved through `npx`, so Node 22.12+ is required for this
/// optional integration.
fn ensure_concord(project_root: Option<&str>) -> Result<(), String> {
    let Some(root) = project_root.map(str::trim).filter(|root| !root.is_empty()) else {
        return Ok(());
    };
    let root = Path::new(root);
    if !root.is_dir() {
        return Ok(());
    }
    ensure_project_server(root, "concord", concord_server(root))
}

/// AgentCAD is Python-only and its OpenCascade binding supports Python 3.10–3.12. Register it
/// only when a launcher can actually import `agentcad.mcp`, rather than leaving every Chat agent
/// with a server that fails to launch: an installed interpreter first, else `uv run --with`.
fn ensure_agentcad(project_root: Option<&str>) -> Result<(), String> {
    let Some(root) = project_root.map(str::trim).filter(|root| !root.is_empty()) else {
        return Ok(());
    };
    let root = Path::new(root);
    if !root.is_dir() {
        return Ok(());
    }
    let Some((program, prefix)) = agentcad_launcher() else {
        return Ok(());
    };
    ensure_project_server(root, "agentcad", agentcad_server(&program, &prefix))
}

/// Merge one named server into a project's `.mcp.json` without touching any other server.
fn ensure_project_server(root: &Path, name: &str, server: Value) -> Result<(), String> {
    let path = root.join(".mcp.json");
    let mut config = match std::fs::read_to_string(&path) {
        Ok(text) if text.trim().is_empty() => serde_json::json!({}),
        Ok(text) => serde_json::from_str::<Value>(&text)
            .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    let object = config
        .as_object_mut()
        .ok_or_else(|| format!("{} must contain a JSON object", path.display()))?;
    let servers = object
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("{} has a non-object `mcpServers`", path.display()))?;
    if servers.contains_key(name) {
        return Ok(());
    }
    servers.insert(name.to_string(), server);
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&config)
            .map_err(|error| format!("could not serialize .mcp.json: {error}"))?
    );
    std::fs::write(&path, text)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    Ok(())
}

/// A command line that can import `agentcad.mcp`, as `(program, args-before--m)`.
fn agentcad_launcher() -> Option<(PathBuf, Vec<String>)> {
    agentcad_python().or_else(agentcad_uv)
}

/// Find a compatible Python that can import both `agentcad` and its MCP extra. `py -3.12` is
/// tried first on Windows because a machine often has 3.12 beside a newer default `python`.
fn agentcad_python() -> Option<(PathBuf, Vec<String>)> {
    let mut candidates: Vec<(PathBuf, Vec<String>)> = Vec::new();
    #[cfg(windows)]
    if let Some(py) = which("py") {
        candidates.push((py, vec!["-3.12".to_string()]));
    }
    if let Some(python) = which("python3.12") {
        candidates.push((python, Vec::new()));
    }
    if let Some(python) = which("python3") {
        candidates.push((python, Vec::new()));
    }
    if let Some(python) = which("python") {
        candidates.push((python, Vec::new()));
    }

    candidates.into_iter().find(|(program, prefix)| {
        std::process::Command::new(program)
            .args(prefix)
            .args([
                "-c",
                "import sys, agentcad.mcp; sys.exit(0 if (3,10) <= sys.version_info[:2] <= (3,12) else 1)",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// Fall back to `uv run --with`, which builds its environment under uv's own cache instead of a
/// system interpreter. The prefix stops at `python`, so `agentcad_server` still appends the module.
fn agentcad_uv() -> Option<(PathBuf, Vec<String>)> {
    let uv = which("uv")?;
    Some((
        uv,
        [
            "run",
            "--python",
            "3.12",
            "--with",
            "agentcad[mcp]",
            "python",
        ]
        .map(str::to_string)
        .to_vec(),
    ))
}

fn agentcad_server(program: &Path, prefix: &[String]) -> Value {
    let mut args = prefix.to_vec();
    args.extend(["-m".to_string(), "agentcad.mcp".to_string()]);
    serde_json::json!({
        "command": program.to_string_lossy().to_string(),
        "args": args
    })
}

/// The stdio launch recipe for Concord. Windows resolves `npx.cmd` through `cmd /c`, matching
/// the MCP catalogue's package recipe and the way the harnesses already launch npm-backed tools.
fn concord_server(root: &Path) -> Value {
    let mut server = if cfg!(windows) {
        serde_json::json!({
            "command": "cmd",
            "args": ["/c", "npx", "-y", "@concord-ai/concord-mcp"]
        })
    } else {
        serde_json::json!({
            "command": "npx",
            "args": ["-y", "@concord-ai/concord-mcp"]
        })
    };
    server["env"] = serde_json::json!({
        "CONCORD_REPO_ROOT": root.to_string_lossy().to_string(),
        "CONCORD_AGENT_KIND": "agentchattr"
    });
    server
}

pub async fn start_agents(project_root: Option<&str>) -> Result<Vec<String>, String> {
    start_agents_with_free(project_root, None).await
}

/// Start every configured agent, plus the managed Free-mode agent when one is supplied.
/// Free mode's API agent has no key in Rigel's environment, so its readiness is explicit: the
/// key travels to the child only.
pub async fn start_agents_with_free(
    project_root: Option<&str>,
    free: Option<&FreeAgent>,
) -> Result<Vec<String>, String> {
    let client = http_client();
    if !healthy(&client).await {
        return Err("the room is not running — start it first".to_string());
    }
    let dir = dir_for(project_root).ok_or_else(missing_checkout)?;
    ensure_project_mcps(project_root)?;
    if let Some(free) = free {
        install_free_agent(&dir, free)?;
    }

    let present = api_call(reqwest::Method::GET, "/api/status", None)
        .await
        .map(|value| online_names(&value))
        .unwrap_or_default();

    let mut started = Vec::new();
    let mut problems = Vec::new();
    for harness in harnesses(&dir) {
        let managed = free.is_some() && harness.name == FREE_AGENT_NAME;
        if !harness.ready && !managed {
            continue;
        }
        if present.iter().any(|name| same_agent(name, &harness.name)) {
            continue;
        }
        let env = match (managed, free) {
            (true, Some(free)) => free
                .api_key
                .clone()
                .map(|key| vec![(FREE_AGENT_API_KEY_ENV, key)])
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        match spawn_harness_env(&dir, &harness, &env) {
            Ok(process) => started.push(process.label),
            Err(problem) => problems.push(problem),
        }
    }

    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    Ok(started)
}

/// Start just the managed Free-mode agent. Idempotent: an already-online agent is left alone.
pub async fn start_free_agent(
    project_root: Option<&str>,
    free: &FreeAgent,
) -> Result<String, String> {
    let client = http_client();
    if !healthy(&client).await {
        return Err("the room is not running — start it first".to_string());
    }
    let dir = dir_for(project_root).ok_or_else(missing_checkout)?;
    install_free_agent(&dir, free)?;
    let present = api_call(reqwest::Method::GET, "/api/status", None)
        .await
        .map(|value| online_names(&value))
        .unwrap_or_default();
    if present.iter().any(|name| same_agent(name, FREE_AGENT_NAME)) {
        return Ok(format!(
            "Rigel Free is already in the room as @{FREE_AGENT_NAME}"
        ));
    }
    let harness = harnesses(&dir)
        .into_iter()
        .find(|harness| harness.name == FREE_AGENT_NAME)
        .ok_or_else(|| {
            "the managed Free-mode agent is missing from config.local.toml".to_string()
        })?;
    let env = free
        .api_key
        .clone()
        .map(|key| vec![(FREE_AGENT_API_KEY_ENV, key)])
        .unwrap_or_default();
    let process = spawn_harness_env(&dir, &harness, &env)?;
    Ok(format!("{} started (pid {})", process.label, process.pid))
}

/// Start one configured harness by name — the roster's per-agent button. Same readiness rules as
/// the all-at-once launch: an uninstalled CLI or a keyless API agent is refused with the reason
/// rather than joining the room only to fail.
pub async fn start_agent(project_root: Option<&str>, name: &str) -> Result<String, String> {
    let client = http_client();
    if !healthy(&client).await {
        return Err("the room is not running — start it first".to_string());
    }
    let dir = dir_for(project_root).ok_or_else(missing_checkout)?;
    ensure_project_mcps(project_root)?;
    let harness = harnesses(&dir)
        .into_iter()
        .find(|harness| same_agent(name, &harness.name))
        .ok_or_else(|| {
            format!("`{name}` is not configured in this checkout (config.toml / config.local.toml)")
        })?;
    if !harness.ready {
        return Err(format!(
            "{} cannot start: the CLI is not on PATH{}",
            harness.label,
            if harness.api {
                " or its provider key is unset"
            } else {
                ""
            }
        ));
    }
    let present = api_call(reqwest::Method::GET, "/api/status", None)
        .await
        .map(|value| online_names(&value))
        .unwrap_or_default();
    if present
        .iter()
        .any(|online| same_agent(online, &harness.name))
    {
        return Err(format!("{} is already in the room", harness.label));
    }
    let process = spawn_harness(&dir, &harness)?;
    Ok(format!("{} started (pid {})", process.label, process.pid))
}

/// The harnesses this checkout configures, for the roster's per-agent Start buttons. Empty when
/// there is no checkout; the Chat tab explains that case separately.
pub fn configured(project_root: Option<&str>) -> Vec<ChatHarness> {
    dir_for(project_root)
        .map(|dir| probed_harnesses(&dir))
        .unwrap_or_default()
}

/// Re-probe the room after a setup change. The readiness cache is deliberately short, but a
/// user who has just signed in or added a key should not have to wait for it to expire.
pub fn configured_fresh(project_root: Option<&str>) -> Vec<ChatHarness> {
    clear_readiness_cache();
    configured(project_root)
}

/// Drop every cached readiness answer. Settings calls this after a keychain write; the Chat tab
/// and the settings guide then see the new credential on their next read.
pub fn clear_readiness_cache() {
    READINESS_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut cache) = readiness_cache().lock() {
        cache.clear();
    }
}

/// One message for both spawn paths, so the Chat tab can say the same thing everywhere.
fn missing_checkout() -> String {
    "no chat sidecar and no agentchattr checkout: build the sidecar with      `python scripts/sidecar/build-agentchat.py`, clone https://github.com/bcurts/agentchattr      into vendor/agentchattr, or set RIGEL_AGENTCHAT_DIR to an existing checkout"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_verdicts_are_cached_and_can_be_cleared() {
        let dir = PathBuf::from("readiness-cache-test");
        let row = ChatHarness {
            name: "minimax".to_string(),
            label: "MiniMax".to_string(),
            api: true,
            ready: false,
            status: HarnessStatus::MissingKey,
            evidence: "no key".to_string(),
            remediation: None,
            credential: Some("MINIMAX_API_KEY".to_string()),
        };
        clear_readiness_cache();
        let generation = READINESS_GENERATION.load(std::sync::atomic::Ordering::SeqCst);
        remember_readiness(&dir, std::slice::from_ref(&row), generation);
        let cached = cached_readiness(&dir).expect("a stored verdict is returned within the TTL");
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].status, HarnessStatus::MissingKey);
        // A probe that started before the clear must not write its stale answer back.
        clear_readiness_cache();
        assert!(cached_readiness(&dir).is_none());
        remember_readiness(&dir, std::slice::from_ref(&row), generation);
        assert!(cached_readiness(&dir).is_none());
        let fresh = READINESS_GENERATION.load(std::sync::atomic::Ordering::SeqCst);
        remember_readiness(&dir, std::slice::from_ref(&row), fresh);
        assert!(cached_readiness(&dir).is_some());
        clear_readiness_cache();
    }

    #[test]
    fn slow_clis_get_a_longer_budget_and_timeouts_are_their_own_state() {
        assert!(probe_budget("antigravity") >= Duration::from_secs(60));
        assert!(probe_budget("qwen") >= Duration::from_secs(60));
        assert!(probe_budget("kilo") > probe_budget("claude"));
        assert_eq!(
            serde_json::to_string(&HarnessStatus::ProbeTimeout).unwrap(),
            "\"probe_timeout\""
        );
        assert_eq!(
            serde_json::to_string(&HarnessStatus::ProviderBlocked).unwrap(),
            "\"provider_blocked\""
        );
        assert!(provider_billing_diagnosis("status: 402").is_some());
        assert!(provider_billing_diagnosis("Your prepayment credits are depleted").is_some());
    }

    #[test]
    fn the_room_passes_the_arguments_each_cli_needs_to_run_unattended() {
        let harness = |name: &str| ChatHarness {
            name: name.to_string(),
            label: name.to_string(),
            api: false,
            ready: true,
            status: HarnessStatus::Unknown,
            evidence: String::new(),
            remediation: None,
            credential: None,
        };
        let codex = harness_argv(&harness("codex"));
        assert!(
            codex
                .windows(2)
                .any(|pair| pair == ["-c", "openai_base_url="])
        );
        let antigravity = harness_argv(&harness("antigravity"));
        assert!(
            antigravity
                .iter()
                .any(|arg| arg == "--dangerously-skip-permissions")
        );
        assert_eq!(
            harness_argv(&harness("gemini")),
            vec!["wrapper.py", "gemini", "--skip-trust", "--yolo"]
        );
    }

    #[test]
    fn claude_drops_the_conflicting_api_key_only_when_a_token_is_present() {
        assert!(harness_env_removals("claude", true).contains(&"ANTHROPIC_API_KEY"));
        assert!(harness_env_removals("claude", false).is_empty());
        assert!(harness_env_removals("codex", true).is_empty());
    }

    #[test]
    fn only_live_agents_count_as_online() {
        let status = serde_json::json!({
            "paused": false,
            "claude": { "available": true, "busy": false },
            "codex": { "available": false, "busy": false }
        });
        assert_eq!(online_names(&status), vec!["claude".to_string()]);
    }

    #[test]
    fn a_busy_agent_without_a_reply_becomes_stuck() {
        let status = serde_json::json!({
            "claude": { "available": true, "busy": true }
        });
        let threshold = Duration::from_secs(600);
        let mut watchdog = Watchdog::default();
        let start = Instant::now();
        assert!(watchdog.inspect(&status, &[], start, threshold).is_empty());
        assert_eq!(
            watchdog.inspect(
                &status,
                &[],
                start + threshold + Duration::from_secs(1),
                threshold
            ),
            vec!["claude".to_string()]
        );
    }

    #[test]
    fn a_room_reply_resets_the_stuck_clock() {
        let status = serde_json::json!({
            "claude": { "available": true, "busy": true }
        });
        let threshold = Duration::from_secs(600);
        let mut watchdog = Watchdog::default();
        let start = Instant::now();
        assert!(watchdog.inspect(&status, &[], start, threshold).is_empty());
        let reply = ChatMessage {
            id: 7,
            sender: "claude".to_string(),
            text: "still here".to_string(),
            time: String::new(),
            kind: "chat".to_string(),
            channel: "general".to_string(),
            attachments: Vec::new(),
        };
        assert!(
            watchdog
                .inspect(
                    &status,
                    &[reply],
                    start + threshold - Duration::from_secs(1),
                    threshold
                )
                .is_empty()
        );
        assert!(
            watchdog
                .inspect(
                    &status,
                    &[],
                    start + threshold + Duration::from_secs(1),
                    threshold
                )
                .is_empty()
        );
        assert_eq!(
            watchdog.inspect(
                &status,
                &[],
                start + threshold * 2 + Duration::from_secs(1),
                threshold
            ),
            vec!["claude".to_string()]
        );
    }

    #[test]
    fn agent_tables_read_through_quotes_and_comments() {
        let tables = parse_agents(
            r##"
[server]
port = 8300

# a comment
[agents.claude]
command = "claude"   # trailing comment
color = "#da7756"
label = "Claude"

[agents.phi4]
type = "api"
label = "Phi4 (local)"
"##,
        );
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].0, "claude");
        let field = |index: usize, key: &str| {
            tables[index]
                .1
                .iter()
                .find(|(known, _)| known == key)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(field(0, "command").as_deref(), Some("claude"));
        assert_eq!(field(0, "color").as_deref(), Some("#da7756"));
        assert_eq!(field(1, "type").as_deref(), Some("api"));
        assert_eq!(tables[1].0, "phi4");
    }

    #[test]
    fn multiline_strip_env_keeps_every_name_for_the_probe() {
        let tables = parse_agents(
            r#"
[agents.kimi]
command = "kimi"
strip_env = [
  "ANTHROPIC_API_KEY",
  "GH_TOKEN",
]

[agents.qwen]
command = "qwen"
"#,
        );
        assert_eq!(tables.len(), 2);
        let list = tables[0]
            .1
            .iter()
            .find(|(key, _)| key == "strip_env")
            .map(|(_, value)| env_list(value))
            .unwrap();
        assert_eq!(list, vec!["ANTHROPIC_API_KEY", "GH_TOKEN"]);
    }

    #[test]
    fn an_auth_line_beats_a_zero_exit_status() {
        let (evidence, remediation, credential) = auth_diagnosis(
            "codebuddy",
            "Authentication required. Please use /login command to sign in to your account\n",
        )
        .expect("CodeBuddy's auth line is recognised");
        assert!(evidence.contains("Authentication required"), "{evidence}");
        assert!(remediation.unwrap().contains("/login"));
        assert!(credential.unwrap().contains("CodeBuddy"));
        assert!(!output_has_ok("OKAY"));
        assert!(output_has_ok("warning: stale config\nOK\n"));
    }

    #[test]
    fn kimi_and_kilo_diagnostics_name_their_real_fixes() {
        let kimi = auth_diagnosis(
            "kimi",
            "error: provider.auth_error: Your current subscription does not have access to Kimi Code right now. Upgrade your plan: https://www.kimi.com/code/#pricing\n",
        )
        .expect("Kimi's subscription error is recognised");
        assert!(kimi.1.unwrap().contains("kimi.com/code"));
        assert!(kimi.2.unwrap().contains("subscription"));

        let kilo = auth_diagnosis("kilo", "You need to sign in to use this model.\n")
            .expect("Kilo's sign-in line is recognised");
        assert!(kilo.1.unwrap().contains("kilo auth login"));
    }

    #[test]
    fn the_vendored_config_offers_the_agent_tables_rigel_starts() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent);
        let Some(repo_root) = repo_root else { return };
        let Some(dir) = dir_for(Some(&repo_root.display().to_string())) else {
            return;
        };
        let found = harnesses(&dir);
        assert!(found.iter().any(|harness| harness.name == "claude"));
        assert!(found.iter().any(|harness| harness.name == "codex"));
        // The pinned config grows and shrinks with upstream; these two are what Rigel's own
        // start flags are written for, and every table must parse into a usable harness.
        assert!(found.len() >= 2);
        assert!(
            found
                .iter()
                .all(|harness| !harness.name.is_empty() && !harness.label.is_empty())
        );
    }

    #[test]
    fn an_api_agent_needs_its_key_before_it_counts_as_ready() {
        // A missing key would join the roster and fail every turn; a keyless local model is fine.
        assert!(!harness_ready(
            true,
            "minimax",
            Some("RIGEL_TEST_KEY_THAT_IS_NOT_SET")
        ));
        assert!(harness_ready(true, "phi4", None));
        assert!(!harness_ready(
            false,
            "definitely-not-an-installed-cli",
            None
        ));
    }

    #[test]
    fn the_free_agent_table_is_managed_without_touching_user_tables() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.local.toml");
        std::fs::write(
            &path,
            "[agents.phi4]\ntype = \"api\"\nbase_url = \"http://localhost:8189/v1\"\n\
             model = \"qwen3-4b\"\n",
        )
        .unwrap();
        let first = FreeAgent {
            base_url: "https://openrouter.ai/api/v1".to_string(),
            model: "deepseek/deepseek-r1:free".to_string(),
            api_key: Some("secret-value".to_string()),
        };
        assert!(!format!("{first:?}").contains("secret-value"));
        install_free_agent(tmp.path(), &first).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[agents.phi4]"));
        assert_eq!(text.matches("[agents.rigel-free]").count(), 1);
        assert!(text.contains("deepseek/deepseek-r1:free"));
        assert!(
            !text.contains("secret-value"),
            "the key travels in the child's environment, never the config"
        );

        let second = FreeAgent {
            model: "openai/gpt-oss-120b:free".to_string(),
            ..first
        };
        install_free_agent(tmp.path(), &second).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("[agents.rigel-free]").count(), 1);
        assert!(text.contains("openai/gpt-oss-120b:free"));
        assert!(!text.contains("deepseek/deepseek-r1:free"));
        assert!(
            harnesses(tmp.path())
                .iter()
                .any(|harness| harness.name == FREE_AGENT_NAME),
            "the managed table is visible to the launch path"
        );
    }

    #[test]
    fn a_harness_sets_its_own_script_and_unattended_flag() {
        let claude = ChatHarness {
            name: "claude".into(),
            label: "Claude".into(),
            api: false,
            ready: true,
            status: HarnessStatus::Unknown,
            evidence: String::new(),
            remediation: None,
            credential: None,
        };
        assert_eq!(
            harness_argv(&claude),
            vec!["wrapper.py", "claude", "--dangerously-skip-permissions"]
        );
        let phi4 = ChatHarness {
            name: "phi4".into(),
            label: "Phi4".into(),
            api: true,
            ready: true,
            status: HarnessStatus::Unknown,
            evidence: String::new(),
            remediation: None,
            credential: None,
        };
        assert_eq!(harness_argv(&phi4), vec!["wrapper_api.py", "phi4"]);
    }

    #[test]
    fn the_dir_guess_finds_a_vendored_checkout() {
        // `vendor/agentchattr` is a checkout, not tracked, so the fixture is built here rather
        // than relying on the machine the test runs on.
        if std::env::var_os("RIGEL_AGENTCHAT_DIR").is_some() {
            // The env override wins by design; that is the other test's subject.
            return;
        }
        let root = std::env::temp_dir().join(format!("rigel-chat-guess-{}", std::process::id()));
        let checkout = root.join("vendor/agentchattr");
        std::fs::create_dir_all(&checkout).expect("temp checkout");
        std::fs::write(checkout.join("run.py"), "").expect("run.py");

        let found = dir_for(Some(&root.display().to_string()));
        assert_eq!(
            found.as_deref(),
            Some(checkout.as_path()),
            "the guess should land on vendor/agentchattr next to the project"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_env_and_project_mean_no_dir() {
        assert_eq!(dir_for(Some("/nonexistent-skills-shelf")), None);
    }

    #[test]
    fn the_setup_note_names_the_clone_path_and_the_env_override() {
        let note = setup_help(Some("C:/work/demo/"));
        assert!(note.contains("https://github.com/bcurts/agentchattr"));
        assert!(note.contains("C:/work/demo/vendor/agentchattr"), "{note}");
        assert!(note.contains("RIGEL_AGENTCHAT_DIR"));
        // No project: the copy must not pretend to know where it goes.
        assert!(setup_help(None).contains("<project>/vendor/agentchattr"));
    }

    #[test]
    fn a_sidecar_copy_is_refreshed_without_touching_the_room() {
        let root = std::env::temp_dir().join(format!(
            "rigel-sidecar-copy-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let source = root.join("source/agentchattr");
        let data = root.join("data");
        std::fs::create_dir_all(source.join("static")).expect("source tree");
        std::fs::write(source.join("run.py"), "v1").expect("run.py");
        std::fs::write(source.join("stale.py"), "gone").expect("stale.py");
        std::fs::write(source.join("static/index.html"), "one").expect("static");
        std::fs::write(source.join(".rigel-sidecar"), "v1\n").expect("marker");

        copy_tree(&source, &data, &["config.local.toml", ".rigel-sidecar"]).expect("first copy");
        assert_eq!(std::fs::read_to_string(data.join("run.py")).unwrap(), "v1");

        // The room writes these; a rebuild must not take them away.
        std::fs::create_dir_all(data.join("data")).expect("data dir");
        std::fs::write(data.join("data/chat.jsonl"), "a message").expect("history");
        std::fs::write(data.join("config.local.toml"), "[agents.local]").expect("local config");

        std::fs::write(source.join("run.py"), "v2").expect("new run.py");
        std::fs::remove_file(source.join("stale.py")).expect("drop stale.py");
        std::fs::write(source.join("static/index.html"), "two").expect("new static");
        copy_tree(&source, &data, &["config.local.toml", ".rigel-sidecar"]).expect("refresh");

        assert_eq!(std::fs::read_to_string(data.join("run.py")).unwrap(), "v2");
        assert_eq!(
            std::fs::read_to_string(data.join("static/index.html")).unwrap(),
            "two"
        );
        assert!(!data.join("stale.py").exists(), "stale source is removed");
        assert_eq!(
            std::fs::read_to_string(data.join("data/chat.jsonl")).unwrap(),
            "a message"
        );
        assert_eq!(
            std::fs::read_to_string(data.join("config.local.toml")).unwrap(),
            "[agents.local]"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_chat_start_adds_obscura_and_concord_to_the_project_mcp_file() {
        let project = tempfile::tempdir().unwrap();
        ensure_project_mcps(Some(project.path().to_str().unwrap())).unwrap();
        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(project.path().join(".mcp.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            config["mcpServers"]["obscura"]["args"],
            serde_json::json!(["mcp"])
        );
        assert_eq!(
            config["mcpServers"]["concord"]["env"]["CONCORD_REPO_ROOT"],
            serde_json::json!(project.path().to_string_lossy())
        );
        assert_eq!(
            config["mcpServers"]["concord"]["env"]["CONCORD_AGENT_KIND"],
            "agentchattr"
        );
        let args = config["mcpServers"]["concord"]["args"].as_array().unwrap();
        assert!(args.iter().any(|arg| arg == "@concord-ai/concord-mcp"));
        if roblox::mcp_server().is_some() {
            assert!(
                config["mcpServers"]["roblox_studio"]["command"]
                    .as_str()
                    .is_some(),
                "an installed Studio server is written into .mcp.json"
            );
        }

        // A second start is a no-op and does not rewrite the user's existing entry.
        let before = std::fs::read_to_string(project.path().join(".mcp.json")).unwrap();
        ensure_project_mcps(Some(project.path().to_str().unwrap())).unwrap();
        assert_eq!(
            before,
            std::fs::read_to_string(project.path().join(".mcp.json")).unwrap()
        );
    }

    #[test]
    fn agentcad_uses_the_python_module_and_keeps_launcher_args() {
        let server = agentcad_server(Path::new("C:/Windows/py.exe"), &["-3.12".to_string()]);
        assert_eq!(server["command"], "C:/Windows/py.exe");
        assert_eq!(
            server["args"],
            serde_json::json!(["-3.12", "-m", "agentcad.mcp"])
        );

        // The uv fallback stops at `python`, so the module flags are appended the same way.
        let uv = agentcad_server(
            Path::new("C:/Tools/uv.exe"),
            &["run", "--with", "agentcad[mcp]", "python"].map(str::to_string),
        );
        assert_eq!(
            uv["args"],
            serde_json::json!([
                "run",
                "--with",
                "agentcad[mcp]",
                "python",
                "-m",
                "agentcad.mcp"
            ])
        );
    }

    #[test]
    fn attachments_keep_only_room_uploads_and_their_names() {
        let message = parse_message(&serde_json::json!({
            "id": 4,
            "sender": "claude",
            "text": "the screenshot",
            "time": "21:18:02",
            "type": "chat",
            "channel": "general",
            "attachments": [
                {"name": "shot.png", "url": "/uploads/shot.png"},
                {"url": "/uploads/plain.jpg"},
                {"name": "outside", "url": "https://example.com/a.png"},
                {"name": "nested", "url": "/uploads/../secret.png"},
                {"name": "scheme", "url": "file:///C:/secret.png"}
            ]
        }))
        .expect("a message");
        assert_eq!(message.attachments.len(), 2, "only room uploads survive");
        assert_eq!(message.attachments[0].name, "shot.png");
        assert_eq!(message.attachments[1].name, "plain.jpg");
    }

    #[test]
    fn the_attachment_reader_refuses_anything_but_a_plain_upload_name() {
        assert!(attachment_path("/uploads/shot.png").is_ok());
        for bad in [
            "/uploads/",
            "/uploads/../secret.png",
            "/uploads/a/b.png",
            "http://127.0.0.1:8300/uploads/a.png",
            "file:///C:/secret.png",
            "/api/messages",
        ] {
            assert!(attachment_path(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn only_image_uploads_have_a_mime() {
        assert_eq!(image_mime("a.PNG"), Some("image/png"));
        assert_eq!(image_mime("a.jpeg"), Some("image/jpeg"));
        assert_eq!(image_mime("a.pdf"), None);
        assert_eq!(image_mime("noextension"), None);
    }

    #[test]
    fn concord_does_not_overwrite_a_users_entry() {
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join(".mcp.json");
        std::fs::write(
            &path,
            r#"{"mcpServers":{"concord":{"command":"custom"},"keep":{"command":"keep"}}}"#,
        )
        .unwrap();
        ensure_concord(Some(project.path().to_str().unwrap())).unwrap();
        let config: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["concord"]["command"], "custom");
        assert_eq!(config["mcpServers"]["keep"]["command"], "keep");
    }

    #[test]
    fn the_copied_config_points_every_agent_at_the_open_project() {
        let root = std::env::temp_dir().join(format!(
            "rigel-sidecar-cwd-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).expect("temp dir");
        let config = root.join("config.toml");
        std::fs::write(
            &config,
            "[server]\nport = 8300\n\n[agents.claude]\ncommand = \"claude\"\ncwd = \"..\"\n\
             color = \"#da7756\"\n\n[agents.phi4]\ntype = \"api\"\n",
        )
        .expect("config");

        let project = Path::new("C:/work/demo");
        assert!(rewrite_agent_cwd(&config, project).expect("rewrite"));
        let text = std::fs::read_to_string(&config).unwrap();
        assert_eq!(text.matches("cwd = \"C:/work/demo\"").count(), 2, "{text}");
        assert!(!text.contains("cwd = \"..\""), "{text}");
        // Everything else survives, including the flat tables and colours.
        assert!(text.contains("command = \"claude\""));
        assert!(text.contains("color = \"#da7756\""));
        assert!(text.contains("[server]\nport = 8300"));
        // A second call is a no-op, so a poll loop does not touch the file.
        assert!(!rewrite_agent_cwd(&config, project).expect("rewrite again"));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_sidecar_installer_refuses_the_rigel_data_root() {
        let root = std::env::temp_dir().join(format!(
            "rigel-sidecar-guard-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let source = root.join("source");
        std::fs::create_dir_all(source.join("agentchattr")).expect("source dir");
        std::fs::write(source.join("agentchattr/run.py"), "").expect("run.py");
        let index = root.join("index");
        std::fs::create_dir_all(&index).expect("index dir");
        std::fs::write(index.join("api.json"), "{}").expect("api.json");
        let error = install_sidecar(&source, &index).expect_err("must refuse the data root");
        assert!(error.contains("data root"), "{error}");
        assert!(index.join("api.json").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_installed_sidecar_is_found_beside_the_binary() {
        let root = std::env::temp_dir().join(format!(
            "rigel-packaged-sidecar-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(root.join("sidecar/agentchattr")).expect("sidecar dir");
        std::fs::write(root.join("sidecar/agentchattr/run.py"), "").expect("run.py");
        assert_eq!(packaged_sidecar_dir(&root), Some(root.join("sidecar")));

        std::fs::remove_dir_all(root.join("sidecar")).expect("remove sidecar");
        std::fs::create_dir_all(root.join("resources/sidecar/agentchattr")).expect("resource dir");
        std::fs::write(root.join("resources/sidecar/agentchattr/run.py"), "")
            .expect("resource run.py");
        assert_eq!(
            packaged_sidecar_dir(&root),
            Some(root.join("resources/sidecar"))
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_bundled_python_only_answers_for_the_bundled_copy() {
        let root = std::env::temp_dir().join(format!(
            "rigel-sidecar-python-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(root.join("python")).expect("python dir");
        #[cfg(windows)]
        std::fs::write(root.join("python/python.exe"), "").expect("python");
        #[cfg(not(windows))]
        std::fs::write(root.join("python/bin/python3"), "").expect("python");
        assert!(bundled_python(&root).is_some());
        assert!(bundled_python(&root.join("elsewhere")).is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(windows)]
    #[test]
    fn the_server_prefers_the_windowless_interpreter() {
        let root = std::env::temp_dir().join(format!(
            "rigel-pythonw-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(root.join("python")).expect("python dir");
        std::fs::write(root.join("python/python.exe"), "").expect("python");
        assert!(bundled_python_windowless(&root).is_none());
        std::fs::write(root.join("python/pythonw.exe"), "").expect("pythonw");
        assert_eq!(
            bundled_python_windowless(&root),
            Some(root.join("python/pythonw.exe"))
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_suffixed_room_name_belongs_to_the_configured_agent() {
        // The room registers `claude`, a second copy claims `claude-2`, and `claudette` is a
        // different agent entirely — a prefix match without the dash would confuse them.
        assert!(same_agent("claude", "claude"));
        assert!(same_agent("claude-2", "claude"));
        assert!(!same_agent("claude", "claude-2"));
        assert!(!same_agent("claudette", "claude"));
    }

    #[cfg(windows)]
    #[test]
    fn kill_tree_ends_a_hidden_child_and_tolerates_a_dead_pid() {
        use std::os::windows::process::CommandExt;
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 60 127.0.0.1 >/dev/null"])
            .creation_flags(0x0800_0000)
            .spawn()
            .expect("spawn a long-running hidden child");
        let pid = child.id();
        kill_tree(pid).expect("taskkill /T /F");
        let status = child.wait().expect("the child is reaped");
        assert!(!status.success(), "a killed child must not report success");
        assert!(!pid_running(pid), "tasklist must not find the pid");
        // A pid that died on its own between the click and the kill is not an error.
        assert!(kill_tree(pid).is_ok());
    }

    /// The `CreateProcess` half, pinned. A stub wrapper launched the way `start_agents` launches a
    /// real one must get a console with a usable stdin handle and no window, and must receive the
    /// argv the harness table chose. `GetConsoleWindow() == 0` is the whole "nothing on screen"
    /// claim; `GetConsoleMode` on the stdin handle is what the wrapper's `WriteConsoleInput`
    /// injection needs and what a redirected pipe would fail.
    #[cfg(windows)]
    #[test]
    fn a_launched_wrapper_gets_a_hidden_console_and_its_argv() {
        let dir = std::env::temp_dir().join(format!("rigel-spawn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(
            dir.join("wrapper.py"),
            r#"
import ctypes, pathlib, sys, time, traceback

here = pathlib.Path(__file__).resolve().parent

def publish(name, text):
    # Publish atomically: a reader must never see the empty file between create and write.
    target = here / name
    staged = here / (name + ".tmp")
    staged.write_text(text, encoding="utf-8")
    staged.replace(target)

try:
    kernel32 = ctypes.windll.kernel32
    kernel32.CreateFileW.restype = ctypes.c_void_p
    # CONIN$ names the console the child owns, whatever handles it inherited. A writable console
    # input buffer with a console mode is exactly what the wrapper's WriteConsoleInput needs.
    conin = kernel32.CreateFileW("CONIN$", 0xC0000000, 3, None, 3, 0, None)
    mode = ctypes.c_uint(0)
    console_ok = (
        1
        if conin not in (None, 0, ctypes.c_void_p(-1).value)
        and kernel32.GetConsoleMode(ctypes.c_void_p(conin), ctypes.byref(mode))
        else 0
    )
    publish(
        "report.txt",
        "hwnd=%d\nconsole_ok=%d\nargv=%s\n"
        % (kernel32.GetConsoleWindow(), console_ok, " ".join(sys.argv[1:])),
    )
    time.sleep(30)
except BaseException:
    # A stub that dies leaves the test no report to read. Its status and traceback go in
    # `error.txt`, so the failure names a cause instead of only saying the report is missing.
    publish("error.txt", "exit=1\n" + traceback.format_exc())
    sys.exit(1)
"#,
        )
        .expect("stub wrapper");

        let Some(python) = chat_python(&dir) else {
            eprintln!("no python on PATH; skipping the CreateProcess check");
            return;
        };
        let argv = harness_argv(&ChatHarness {
            name: "claude".to_string(),
            label: "Claude".to_string(),
            api: false,
            ready: true,
            status: HarnessStatus::Unknown,
            evidence: String::new(),
            remediation: None,
            credential: None,
        });
        assert_eq!(
            argv,
            vec![
                "wrapper.py".to_string(),
                "claude".to_string(),
                "--dangerously-skip-permissions".to_string()
            ],
            "the harness table is what the spawn is given"
        );

        let process = launch_hidden_env(&python, &argv, &dir, "claude", "Claude", &[])
            .expect("the stub wrapper starts");
        let report = dir.join("report.txt");
        let failure = dir.join("error.txt");
        let mut text = String::new();
        let mut stub_error = String::new();
        let mut exited = false;
        let mut missing = 0;
        for tick in 0..100 {
            if let Ok(read) = std::fs::read_to_string(&report) {
                text = read;
                // `argv=` is the last line, so its presence means the whole report is there.
                if text.contains("argv=") {
                    break;
                }
            }
            // The stub records its own status and traceback: `launch_hidden` hands the child to
            // the reaper that keeps the roster honest, so its `ExitStatus` never comes back here.
            if let Ok(read) = std::fs::read_to_string(&failure) {
                stub_error = read;
                break;
            }
            // A stub that died without writing used to burn the whole budget and then say
            // nothing. `tasklist` is a process of its own, so ask it every fourth tick, and only
            // call it an exit once two answers agree — a single miss would fake a failure.
            if tick % 4 == 0 && tick >= 4 {
                missing = if pid_running(process.pid) {
                    0
                } else {
                    missing + 1
                };
                if missing == 2 {
                    exited = true;
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = kill_tree(process.pid);
        // The kill can land after the last poll; a report already on disk still counts.
        if text.is_empty() {
            text = std::fs::read_to_string(&report).unwrap_or_default();
        }
        if stub_error.is_empty() {
            stub_error = std::fs::read_to_string(&failure).unwrap_or_default();
        }
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            !text.is_empty(),
            "the stub wrapper left no report in {}; python={}, argv={:?}, {}; stub reported: {}",
            dir.display(),
            python.display(),
            argv,
            if stub_error.is_empty() {
                if exited {
                    "it exited without writing"
                } else {
                    "it was still running when the budget ran out"
                }
            } else {
                "it recorded its own failure"
            },
            if stub_error.is_empty() {
                "<nothing>"
            } else {
                stub_error.as_str()
            },
        );
        assert!(text.contains("hwnd=0"), "a console window leaked: {text}");
        assert!(
            text.contains("console_ok=1"),
            "the hidden console has no writable input buffer: {text}"
        );
        assert!(
            text.contains("argv=claude --dangerously-skip-permissions"),
            "the launched argv lost the harness flags: {text}"
        );
    }

    #[tokio::test]
    async fn a_chat_sender_must_be_a_real_name() {
        assert!(send_as("hello", "general", "").await.is_err());
        assert!(send_as("hello", "general", "   ").await.is_err());
        assert!(send_as("hello", "general", "system").await.is_err());
        assert!(send_as("hello", "general", &"x".repeat(41)).await.is_err());
    }

    #[test]
    fn native_names_are_readable_and_distinguishable() {
        assert_eq!(native_name_from(Some("Reader"), "9b1c2d3e"), "reader-9b1c");
        assert_eq!(native_name_from(Some("reader"), "9b1c2d3e"), "reader-9b1c");
        assert_eq!(native_name_from(Some("reader"), "aa11bb22"), "reader-aa11");
        assert_eq!(native_name_from(None, "child-1"), "agent-chil");
        assert_eq!(native_name_from(Some("  "), ""), "agent");
        assert_eq!(
            native_name_from(Some("a very long role name that should be cut"), "ffff"),
            "averylongrolenamethatsho-ffff"
        );
    }

    #[test]
    fn the_landing_page_token_is_read_only_from_the_script_it_injects() {
        let html = r#"<html><head><script>window.__SESSION_TOKEN__="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";</script></head></html>"#;
        assert_eq!(
            landing_token(html).as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        assert_eq!(landing_token("<html>no token here</html>"), None);
        assert_eq!(
            landing_token(r#"<script>window.__SESSION_TOKEN__="too-short";</script>"#),
            None
        );
    }

    #[test]
    fn messages_read_the_store_shape() {
        let parsed = parse_messages(serde_json::json!([
            {"id": 5, "sender": "user", "text": "hello", "type": "chat", "time": "12:00:00",
             "channel": "general", "timestamp": 1.0, "uid": "x"}
        ]));
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, 5);
        assert_eq!(parsed[0].kind, "chat");
        assert_eq!(parsed[0].channel, "general");
    }
}

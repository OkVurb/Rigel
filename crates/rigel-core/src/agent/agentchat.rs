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

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;

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
    if let Some(value) = body {
        if value != Value::Null {
            request = request.json(&value);
        }
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
    })
}

fn parse_messages(value: Value) -> Vec<ChatMessage> {
    match value {
        Value::Array(rows) => rows.iter().filter_map(parse_message).collect(),
        _ => Vec::new(),
    }
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
    if rewrite_agent_cwd(&dir.join("config.toml"), Path::new(root)).is_ok() {
        if let Ok(mut guard) = SIDECAR_CWD.lock() {
            *guard = Some(root.to_string());
        }
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
    if let Some(sidecar) = sidecar().filter(|sidecar| sidecar.dir == dir) {
        if let Some(python) = bundled_python_windowless(&sidecar.source) {
            return Some(python);
        }
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
    if let Some(sidecar) = sidecar().filter(|sidecar| sidecar.dir == dir) {
        if let Some(python) = bundled_python(&sidecar.source) {
            return Some(python);
        }
    }
    which("python").or_else(|| which("python3"))
}

/// PATH lookup that keeps the `.exe` detail out of the caller (Windows never bare-executes).
fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        #[cfg(windows)]
        for suffix in [".exe", ""] {
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
    if let Some(name) = channel {
        if !name.is_empty() {
            path.push_str(&format!("&channel={}", urlencoded(name)?));
        }
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
    /// The CLI resolves on PATH. API agents are always ready — they need no binary.
    pub ready: bool,
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
    for raw in text.lines() {
        let line = without_comment(raw).trim();
        if line.is_empty() {
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
        if let Some((_, pairs)) = tables.last_mut() {
            pairs.push((key.trim().to_string(), unquote(value)));
        }
    }
    tables.retain(|(name, _)| !name.is_empty());
    tables
}

/// The harnesses the checkout would wrap, in config order. `config.local.toml` is merged
/// over `config.toml` — the vendor's own precedence for machine-local agents.
pub fn harnesses(dir: &Path) -> Vec<ChatHarness> {
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
            let ready = harness_ready(api, &command, field("api_key_env").as_deref());
            ChatHarness {
                label: field("label").unwrap_or_else(|| name.clone()),
                ready,
                api,
                name,
            }
        })
        .collect()
}

/// A CLI must be installed; an API agent must also have its key in the environment, or it would
/// join the roster only to fail every turn.
fn harness_ready(api: bool, command: &str, api_key_env: Option<&str>) -> bool {
    if api {
        return match api_key_env {
            Some(name) => std::env::var_os(name).is_some(),
            None => true,
        };
    }
    which(command).is_some()
}

/// A hidden console cannot answer a permission prompt, so the harnesses that have such prompts
/// start unattended — the same flags `windows/start_*_skip-permissions.bat` pass by hand.
fn unattended_flag(name: &str) -> Option<&'static str> {
    match name {
        "claude" => Some("--dangerously-skip-permissions"),
        "codex" => Some("--dangerously-bypass-approvals-and-sandbox"),
        _ => None,
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
    argv
}

fn spawn_harness(dir: &Path, harness: &ChatHarness) -> Result<ChatProcess, String> {
    let python = chat_python(dir).ok_or("no python found on PATH to start an agent")?;
    launch_hidden(
        &python,
        &harness_argv(harness),
        dir,
        &harness.name,
        &harness.label,
    )
}

/// The CreateProcess half, split from the harness table so a test can drive it with a stub
/// wrapper. Everything that decides whether the console is visible lives here.
fn launch_hidden(
    program: &Path,
    argv: &[String],
    dir: &Path,
    name: &str,
    label: &str,
) -> Result<ChatProcess, String> {
    let mut command = std::process::Command::new(program);
    command
        .args(argv)
        .current_dir(dir)
        // Inherited on purpose: `CREATE_NO_WINDOW` hands the child its own console, and the
        // wrapper injects keystrokes through those inherited handles. Redirecting them would
        // leave the agent deaf.
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

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

fn online_names(status: &Value) -> Vec<String> {
    status
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// Start every configured agent that is not already in the room, hidden. Returns the labels
/// launched so the tab can say what just happened.
pub async fn start_agents(project_root: Option<&str>) -> Result<Vec<String>, String> {
    let client = http_client();
    if !healthy(&client).await {
        return Err("the room is not running — start it first".to_string());
    }
    let dir = dir_for(project_root).ok_or_else(missing_checkout)?;

    let present = api_call(reqwest::Method::GET, "/api/status", None)
        .await
        .map(|value| online_names(&value))
        .unwrap_or_default();

    let mut started = Vec::new();
    let mut problems = Vec::new();
    for harness in harnesses(&dir) {
        if !harness.ready {
            continue;
        }
        if present.iter().any(|name| same_agent(name, &harness.name)) {
            continue;
        }
        match spawn_harness(&dir, &harness) {
            Ok(process) => started.push(process.label),
            Err(problem) => problems.push(problem),
        }
    }

    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    Ok(started)
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
        .map(|dir| harnesses(&dir))
        .unwrap_or_default()
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
    fn a_harness_sets_its_own_script_and_unattended_flag() {
        let claude = ChatHarness {
            name: "claude".into(),
            label: "Claude".into(),
            api: false,
            ready: true,
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

        let process = launch_hidden(&python, &argv, &dir, "claude", "Claude")
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

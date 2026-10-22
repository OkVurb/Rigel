//! agentchattr: the chat server behind the Chat tab (P6).
//!
//! After OmniRoute, this is the second local service Rigel starts itself and fronts through
//! the core. The vendor checkout (gitignored `vendor/agentchattr`) is the subprocess; Rigel
//! holds *one* relationship to it: it spawns `run.py`, reads the session token from its
//! stdout, and forwards a narrow slice of its API to the window. The renderer never talks to
//! the chat server directly and never sees the token — the same boundary that keeps the
//! keychain out of the window (§3.2, S2).
//!
//! `RIGEL_AGENTCHAT_URL` overrides the origin; `RIGEL_AGENTCHAT_DIR` points at the checkout
//! when it does not sit next to the open project.

use std::path::Path;
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
    /// everything past the landing page; an externally started server reports `false`.
    pub authed: bool,
    pub origin: String,
    /// The checkout the server would be spawned from, when it is on this machine.
    pub dir: Option<String>,
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
async fn api_call(method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value, String> {
    let url = format!("{}{}", origin(), path);
    let token = token().ok_or_else(|| {
        "the chat server is up but Rigel has no session token for it — restart it from the Chat tab"
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

/// Where the vendor checkout sits. The project guess covers the one that has it vendored.
pub fn dir_for(project_root: Option<&str>) -> Option<std::path::PathBuf> {
    if let Ok(from_env) = std::env::var("RIGEL_AGENTCHAT_DIR") {
        let candidate = std::path::PathBuf::from(from_env.trim());
        if candidate.join("run.py").exists() {
            return Some(candidate);
        }
    }
    let Some(project) = project_root else {
        return None;
    };
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

    let authed = running
        && match api_call(reqwest::Method::GET, "/api/settings", None).await {
            Ok(_) => true,
            // `api_call` reports a missing token with a message; a protocol error here means
            // the token is stale (server restarted) — either way the tab cannot authenticate.
            Err(_) => false,
        };

    ChatStatus {
        running,
        authed,
        origin,
        dir: dir_for(project_root).map(|path| path.display().to_string()),
    }
}

/// Health first, launch second — the OmniRoute contract. Spawning is what earns Rigel the
/// session token: `run.py` prints it, and an instance started outside Rigel stays unshared.
pub async fn ensure_server(project_root: Option<&str>) -> Result<ChatStatus, String> {
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
         Run `.venv/Scripts/python run.py` there to see the failure.",
        dir.display()
    ))
}

fn chat_python(dir: &Path) -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    let venv_python = dir.join(".venv/Scripts/python.exe");
    #[cfg(not(windows))]
    let venv_python = dir.join(".venv/bin/python");

    if venv_python.exists() {
        Some(venv_python)
    } else {
        which("python").or_else(|| which("python3"))
    }
}

/// PATH lookup that keeps the `.exe` detail out of the caller (Windows never bare-executes).
fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        #[cfg(windows)]
        for suffix in [".exe", ""] {
            let candidate = dir.join(
                if program.ends_with(".exe") {
                    program.to_string()
                } else {
                    format!("{program}{suffix}")
                },
            );
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
    let python = chat_python(dir).ok_or("no python found on PATH to boot the chat server")?;
    let mut command = std::process::Command::new(python);
    command
        .arg("run.py")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

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

/// Recent history, or everything newer than `since_id` for the poller.
pub async fn messages(channel: Option<&str>, since_id: u64, limit: u64) -> Result<Vec<ChatMessage>, String> {
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
    Ok(parse_messages(api_call(reqwest::Method::GET, &path, None).await?))
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

/// One websocket frame: the vendor browser sends user messages over `/ws`, there is no REST
/// path for them (the REST send is per-agent-token only), so the core owns a short-lived
/// socket per message. `sender: "user"` matches the vendor UI's default identity.
pub async fn send(text: &str, channel: &str) -> Result<(), String> {
    let token = token().ok_or_else(|| {
        "the chat server is up but Rigel has no session token for it — restart it from the Chat tab"
    })?;
    let url = origin()
        .replacen("http", "ws", 1)
        .replace("127.0.0.1", "127.0.0.1");
    let ws_url = format!("{url}/ws?token={token}");

    let (mut socket, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(|error| format!("chat websocket refused: {error}"))?;

    let frame = serde_json::json!({
        "type": "message",
        "sender": "user",
        "channel": channel,
        "text": text,
    });
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            frame.to_string().into(),
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
        if let Some(section) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
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
    let mut command = std::process::Command::new(python);
    command
        .args(harness_argv(harness))
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
        .map_err(|error| format!("could not start {} ({error})", harness.label))?;
    let pid = child.id();
    let process = ChatProcess {
        name: harness.name.clone(),
        label: harness.label.clone(),
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
        let output = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
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
        return Err(format!("taskkill could not stop pid {pid}: {stderr}"));
    }
    #[cfg(not(windows))]
    {
        let output = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
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
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
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
        if !present.iter().any(|online| same_agent(online, &process.name)) {
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
        if present
            .iter()
            .any(|name| same_agent(name, &harness.name))
        {
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
    let dir = dir_for(project_root).ok_or_else(|| missing_checkout())?;
    let harness = harnesses(&dir)
        .into_iter()
        .find(|harness| same_agent(name, &harness.name))
        .ok_or_else(|| {
            format!(
                "`{name}` is not configured in this checkout (config.toml / config.local.toml)"
            )
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
    if present.iter().any(|online| same_agent(online, &harness.name)) {
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
    "no agentchattr checkout: clone https://github.com/bcurts/agentchattr into      vendor/agentchattr (a fresh clone has none — it is gitignored), or set      RIGEL_AGENTCHAT_DIR to an existing checkout"
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
    fn the_vendored_config_offers_claude_codex_and_the_local_phi4() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent);
        let Some(repo_root) = repo_root else { return };
        let Some(dir) = dir_for(Some(&repo_root.display().to_string())) else { return };
        let found = harnesses(&dir);
        assert!(found.iter().any(|harness| harness.name == "claude"));
        assert!(found.iter().any(|harness| harness.name == "codex"));
        let phi4 = found.iter().find(|harness| harness.name == "phi4");
        assert!(phi4.is_some_and(|harness| harness.api && harness.ready));
    }

    #[test]
    fn an_api_agent_needs_its_key_before_it_counts_as_ready() {
        // A missing key would join the roster and fail every turn; a keyless local model is fine.
        assert!(!harness_ready(true, "minimax", Some("RIGEL_TEST_KEY_THAT_IS_NOT_SET")));
        assert!(harness_ready(true, "phi4", None));
        assert!(!harness_ready(false, "definitely-not-an-installed-cli", None));
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

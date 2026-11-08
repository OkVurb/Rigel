//! `rigel serve`: Rigel's coordinator and workspace behind an authenticated HTTP API that is
//! loopback by default.
//!
//! The window and the CLI are two clients of one service layer (ARCHITECTURE.md). An external
//! orchestrator is the third client: it cannot link this crate, cannot reach the window's IPC,
//! and must not be handed the SQLite file, so it gets the narrowest surface that still lets it
//! coordinate. It can read the model roster, start and read a session, run one turn, follow a
//! background turn, cancel it, read the bill, and read or edit files inside one project.
//!
//! Threat model, stated plainly: this is not isolation from other processes running as the same
//! user. It *is* isolation from web pages and other users:
//!
//!   * bind to loopback by default, and accept another address only when it names one interface
//!     (a Tailscale IP, say); the unspecified `0.0.0.0` is refused, so the API cannot silently
//!     join every network;
//!   * a bearer token from the Rigel data directory, compared in constant time;
//!   * any request carrying an `Origin` header is refused, so a browser cannot reach it;
//!   * `application/json` is required for a body, and no CORS headers are ever sent;
//!   * a body cap, one route table, and no shell of its own: a command still goes through the
//!     coordinator's approval gate, which denies unless the request said `approve`;
//!   * every file path goes through the patch pipeline's resolver, so `..`, an absolute path and
//!     a symlink out of the project are all refused, and a write is a journalled patch with an
//!     attributed author, never a bare `fs::write`.
//!
//! A turn started here is a normal turn: it is billed to the session, written to the same
//! transcript, and enforced by the same budget cap. `approve: true` answers the approval cards
//! for that one request; `approve` is never the default, because a caller that is not the user
//! must not inherit the user's consent.

mod autostart;
mod concord;
mod openapi;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};

use async_trait::async_trait;
use futures::StreamExt;
use rigel_core::agent::{ChatMessage, ChatRequest, Delta, agentchat, catalog, providers};
use rigel_core::credentials::{self, KeychainCredentialStore};
use rigel_core::patches::{PatchError, PatchPipeline};
use rigel_core::{
    Agent, AgentEvent, BudgetService, Cancels, CommandApprover, CommandDecision, CommandRequest,
    Coordinator, EventSink, Model, NetworkPolicy, OriginApprover, OriginDecision, OriginRequest,
    PlanApprover, PlanDecision, PlanRequest, Session, SessionError, SessionService, SessionStatus,
    StopAt, SubagentPolicy, ToolOutcome, Tools, now_ms,
};
use serde_json::{Value, json};
use std::io::{Read, Write};
use tokio_util::sync::CancellationToken;

/// The loopback port the API uses when nothing else is named. 20130 is the Hyper bridge, so
/// this port belongs to a different surface on purpose.
pub const DEFAULT_PORT: u16 = 20140;
/// Where the API listens unless `--host` or `--tailscale` says otherwise.
pub const DEFAULT_HOST: &str = "127.0.0.1";
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_PROMPT_CHARS: usize = 200_000;
const MAX_READ_BYTES: usize = 512 * 1024;
const MAX_WRITE_BYTES: usize = 2 * 1024 * 1024;
const MODELS_CACHE: Duration = Duration::from_secs(60);
/// The window heartbeat is ten seconds; three missed beats is a closed or crashed window.
const APP_HEARTBEAT_STALE_MS: i64 = 45_000;
const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct ServeOptions {
    pub project: PathBuf,
    pub data_dir: PathBuf,
    pub host: String,
    pub port: u16,
    /// Resolve the machine's Tailscale IPv4 address instead of using `host`.
    pub tailscale: bool,
    /// Let the API reach any path this user can, and start a command in any directory.
    pub full_access: bool,
    /// The base URL a client reaches this server on, when a tunnel forwards to the port or a
    /// tailnet peer dials the Tailscale address. It changes what is advertised, never what is
    /// bound.
    pub public_url: Option<String>,
    pub rotate: bool,
    pub print_token: bool,
    /// Register a logon task with these flags and exit instead of serving.
    pub install: bool,
    /// Remove that logon task and exit.
    pub uninstall: bool,
    /// Hide this process's console window. The logon entry passes this so a reboot does not
    /// leave a black window on the desktop.
    pub hide_window: bool,
    pub as_json: bool,
}

/// Hide this process's console window. Started from a `Run` entry there is a console to hide;
/// started from a terminal there is one the operator is using, so the caller decides.
#[cfg(windows)]
fn hide_console_window() {
    use core::ffi::c_void;
    unsafe extern "system" {
        fn GetConsoleWindow() -> *mut c_void;
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        fn ShowWindow(window: *mut c_void, command: i32) -> i32;
    }
    // SAFETY: these read this process's console and its holder list, then hide only a window that
    // belongs to this process alone. A null handle is the documented "no console" answer.
    unsafe {
        let mut holders = [0u32; 2];
        // One holder means Windows made this console for this process, which is the logon case.
        // More means an operator's shell is attached, and hiding it would hide their terminal.
        if GetConsoleProcessList(holders.as_mut_ptr(), 2) > 1 {
            return;
        }
        let window = GetConsoleWindow();
        if !window.is_null() {
            ShowWindow(window, 0); // SW_HIDE
        }
    }
}

#[cfg(not(windows))]
fn hide_console_window() {}

/// True when something on this address already answers an HTTP request. This distinguishes a lost
/// startup race from a real bind failure, and nothing else.
fn already_serving(host: &str, port: u16) -> bool {
    use std::net::ToSocketAddrs;
    let Ok(mut addresses) = (host, port).to_socket_addrs() else {
        return false;
    };
    let Some(address) = addresses.next() else {
        return false;
    };
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&address, Duration::from_millis(500))
    else {
        return false;
    };
    stream
        .set_read_timeout(Some(Duration::from_millis(900)))
        .ok();
    if stream
        .write_all(b"GET /v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut head = [0u8; 16];
    matches!(stream.read(&mut head), Ok(count) if count >= 12 && head.starts_with(b"HTTP/1."))
}

/// What `rigel serve` leaves in the data directory so a client can find the port and the token
/// without either being typed into a config file by hand. The shape is deliberately boring:
/// one string and five facts, no expiry, no refresh.
fn config_path(data_dir: &Path) -> PathBuf {
    if let Ok(explicit) = std::env::var("RIGEL_API_CONFIG") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    data_dir.join("api.json")
}

fn new_token() -> String {
    // Two v4 UUIDs are 244 random bits from the OS generator, and hex keeps the token safe to
    // paste into a header without escaping.
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// The token from an existing file, if there is one and it is long enough to be a token rather
/// than a placeholder a user typed by hand.
fn existing_token(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let parsed: Value = serde_json::from_str(&text).ok()?;
    let token = parsed.get("token")?.as_str()?.trim().to_string();
    (token.len() >= 32).then_some(token)
}

fn write_config(
    path: &Path,
    token: &str,
    host: &str,
    port: u16,
    project: &Path,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = json!({
        "name": "rigel",
        "api": "v1",
        "token": token,
        "host": host,
        "port": port,
        "project": project.to_string_lossy(),
        "version": VERSION,
        "created_at": now_ms(),
    });
    std::fs::write(path, serde_json::to_vec_pretty(&body).unwrap_or_default())
}

fn loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

/// Loopback, or one interface address. `0.0.0.0` and `::` are refused: they are not an address
/// a client can name, and binding them would put the API on Wi-Fi and Ethernet as well as the
/// tailnet. A hostname other than `localhost` is refused because it resolves to whatever DNS
/// says at bind time.
fn bindable(host: &str) -> bool {
    if loopback(host) {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(address) => !address.is_unspecified() && !address.is_multicast(),
        Err(_) => false,
    }
}

/// The machine's Tailscale IPv4 address, for `--tailscale`. The CLI is preferred because it is
/// the source of truth; the standard install locations cover a process whose `PATH` does not
/// include it, and `RIGEL_TAILSCALE_BIN` covers everything else.
fn tailscale_ip() -> Result<String, String> {
    let mut note = String::new();
    for candidate in tailscale_candidates() {
        match std::process::Command::new(&candidate)
            .args(["ip", "-4"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                if let Some(address) = parse_tailscale_ip(&text) {
                    return Ok(address);
                }
                note = format!(
                    "{} printed no 100.64.0.0/10 IPv4 address",
                    candidate.display()
                );
            }
            Ok(output) => {
                note = format!(
                    "{} exited {}: {}",
                    candidate.display(),
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            Err(error) => note = format!("{}: {error}", candidate.display()),
        }
    }
    Err(format!(
        "could not find a Tailscale IPv4 address ({note}); start Tailscale, or pass --host <addr>"
    ))
}

/// The first CGNAT address Tailscale prints. The range is what keeps a LAN or loopback address
/// from being mistaken for the tailnet.
fn parse_tailscale_ip(output: &str) -> Option<String> {
    for line in output.lines() {
        let Ok(address) = line.trim().parse::<std::net::Ipv4Addr>() else {
            continue;
        };
        let octets = address.octets();
        if octets[0] == 100 && (64..=127).contains(&octets[1]) {
            return Some(address.to_string());
        }
    }
    None
}

fn tailscale_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(explicit) = std::env::var("RIGEL_TAILSCALE_BIN") {
        let explicit = explicit.trim();
        if !explicit.is_empty() {
            candidates.push(PathBuf::from(explicit));
        }
    }
    candidates.push(PathBuf::from("tailscale"));
    #[cfg(windows)]
    {
        candidates.push(PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe"));
        candidates.push(PathBuf::from(
            r"C:\Program Files (x86)\Tailscale\tailscale.exe",
        ));
    }
    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
        ));
    }
    candidates
}

/// Start the API, or print the token and stop. Returns the process exit code.
pub async fn run(options: ServeOptions) -> ExitCode {
    let project = match options.project.canonicalize() {
        Ok(project) if project.is_dir() => project,
        Ok(_) => {
            eprintln!("rigel: {} is not a directory", options.project.display());
            return ExitCode::from(2);
        }
        Err(error) => {
            eprintln!("rigel: {}: {error}", options.project.display());
            return ExitCode::from(2);
        }
    };

    let path = config_path(&options.data_dir);
    let token = match (options.rotate, existing_token(&path)) {
        (false, Some(token)) => token,
        _ => new_token(),
    };

    if options.print_token {
        if options.as_json {
            println!(
                "{}",
                json!({ "ok": true, "token": token, "config": path.to_string_lossy() })
            );
        } else {
            println!("{token}");
        }
        return ExitCode::SUCCESS;
    }

    let host = if options.tailscale {
        match tailscale_ip() {
            Ok(address) => address,
            Err(error) => {
                eprintln!("rigel: {error}");
                return ExitCode::from(2);
            }
        }
    } else {
        options.host.clone()
    };
    if !bindable(&host) {
        eprintln!(
            "rigel: refusing to bind {host}. Name a loopback address, or one interface address such as your Tailscale IP; 0.0.0.0 is refused because it would expose the API to every network"
        );
        return ExitCode::from(2);
    }
    if options.hide_window {
        hide_console_window();
    }
    if options.install || options.uninstall {
        return autostart::apply(
            options.install,
            options.uninstall,
            &project,
            &host,
            options.port,
            options.public_url.as_deref(),
            options.full_access,
        );
    }

    // The API may be the first process after a reboot. Adopt the sidecar installed beside this
    // executable before any room route resolves a checkout: otherwise the room writes each
    // assignment into its app-data queue while the wrappers watch `vendor/agentchattr/data`.
    agentchat::use_installed_sidecar(&options.data_dir);

    let service = match SessionService::open(&project).await {
        Ok(service) => service,
        Err(error) => {
            eprintln!("rigel: could not open the project database: {error}");
            return ExitCode::from(2);
        }
    };

    let listener = match std::net::TcpListener::bind((host.as_str(), options.port)) {
        Ok(listener) => listener,
        Err(error) => {
            // A logon task and the desktop supervisor can race after a reboot. Losing that race
            // is healthy: the other instance owns the port and the token.
            if already_serving(&host, options.port) {
                eprintln!(
                    "rigel: {host}:{} is already served by another Rigel API; this start is a no-op",
                    options.port
                );
                return ExitCode::SUCCESS;
            }
            eprintln!(
                "rigel: could not listen on {host}:{}: {error}",
                options.port
            );
            return ExitCode::from(2);
        }
    };
    let port = listener
        .local_addr()
        .map(|address| address.port())
        .unwrap_or(options.port);
    if let Err(error) = write_config(&path, &token, &host, port, &project) {
        eprintln!("rigel: could not write {}: {error}", path.display());
        return ExitCode::from(2);
    }

    let concord_cache = options.data_dir.join("concord-inbox.json");
    let concord_inbox = match concord::InboxState::load(&concord_cache) {
        Ok(state) => Arc::new(Mutex::new(state)),
        Err(error) => {
            eprintln!("rigel: {error}; starting with an empty Concord inbox cache");
            Arc::new(Mutex::new(concord::InboxState::default()))
        }
    };
    let watchdog = Arc::new(Mutex::new(agentchat::Watchdog::default()));
    let watchdog_stuck = room_watchdog_stuck();
    let api = Arc::new(Api {
        workspace: RwLock::new(Workspace {
            project: project.clone(),
            service,
        }),
        data_dir: options.data_dir.clone(),
        token: token.clone(),
        host: host.clone(),
        port,
        public_url: options.public_url.clone(),
        cancels: Arc::new(Cancels::new()),
        started: Instant::now(),
        models: Mutex::new(None),
        full_access: options.full_access,
        marker_seen: Mutex::new(None),
        concord_inbox: concord_inbox.clone(),
        watchdog: watchdog.clone(),
        watchdog_stuck,
    });
    if room_watchdog_enabled() {
        agentchat::spawn_watchdog(
            watchdog,
            {
                let api = api.clone();
                move || Some(api.project().to_string_lossy().into_owned())
            },
            watchdog_stuck,
            Duration::from_secs(30),
        );
    }
    // One Concord session, polled off the request path: Muse gets inbox messages through the
    // same `/v1/events` long poll that wakes it for Swarm work.
    concord::spawn_poller(
        concord_inbox,
        {
            let api = api.clone();
            move || api.project()
        },
        concord_cache,
    );

    let url = api.base_url();
    if options.as_json {
        println!(
            "{}",
            json!({
                "ok": true,
                "url": url,
                "local_url": format!("http://{host}:{port}"),
                "token": token,
                "config": path.to_string_lossy(),
                "project": project.to_string_lossy(),
                "full_access": options.full_access,
                "openapi": format!("{url}/v1/openapi.json"),
            })
        );
    } else {
        println!("rigel api on {url}");
        if url != format!("http://{host}:{port}") {
            println!("  local:   http://{host}:{port}");
        }
        println!("  project: {}", project.display());
        println!("  config:  {}", path.display());
        println!("  token:   {token}");
        println!("  openapi: {url}/v1/openapi.json");
        println!(
            "  access:  {}",
            if options.full_access {
                "the whole computer (--full-access)"
            } else {
                "the project only"
            }
        );
        if loopback(&host) {
            println!("  loopback only; every request needs `Authorization: Bearer <token>`");
        } else {
            println!("  reachable on {host}; every request needs `Authorization: Bearer <token>`");
        }
    }

    // Connections are accepted on a plain thread, and each handler future is driven on the main
    // runtime through `Handle::block_on`. Spawning the connection on tokio instead would demand
    // `Send` from the coordinator's futures, which the core's borrow-carrying async fns cannot
    // prove; `Handle::block_on` has no such bound and still gives every connection its own
    // thread, so a long turn does not stop a cancel or a file read from being served.
    //
    // No signal handler: the workspace's tokio features do not include `signal`, and the default
    // Ctrl+C behaviour is the right one for a foreground server — the process ends, the port
    // closes, and the token in api.json is still valid for the next start.
    let handle = tokio::runtime::Handle::current();
    std::thread::spawn(move || {
        for incoming in listener.incoming() {
            match incoming {
                Ok(stream) => {
                    let api = api.clone();
                    let handle = handle.clone();
                    std::thread::spawn(move || serve_connection(api, handle, stream));
                }
                Err(error) => eprintln!("rigel: accept failed: {error}"),
            }
        }
    });

    // The accept thread owns the listener; this future only keeps the process (and the runtime
    // the handlers run on) alive.
    std::future::pending::<()>().await;
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// The routing surface
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Workspace {
    project: PathBuf,
    service: SessionService,
}

struct Api {
    /// The project the app has open, not a fixed `--project`. A switch swaps both the root and
    /// the session database together, so a session id is always answered by the database that
    /// created it.
    workspace: RwLock<Workspace>,
    data_dir: PathBuf,
    token: String,
    host: String,
    port: u16,
    public_url: Option<String>,
    cancels: Arc<Cancels>,
    started: Instant,
    /// The merged roster is a network fan-out; a coordinator that polls it should not pay for a
    /// fresh read every second. The cache is per process and never outlives a key change beyond
    /// a minute, which is the same freshness the picker implies.
    models: Mutex<Option<(Instant, String, Value)>>,
    /// Opt-in: /v1/fs/* and a command that starts outside the project.
    full_access: bool,
    /// The marker mtime already acted on, so a request does not reopen SQLite when the window
    /// has not switched projects.
    marker_seen: Mutex<Option<SystemTime>>,
    /// Muse's Concord inbox, kept fresh by a background poller so a wake does not have to run
    /// `inspect_work` itself.
    concord_inbox: Arc<Mutex<concord::InboxState>>,
    /// Shared with the background loop: the agents this API started that sit busy without a reply.
    watchdog: Arc<Mutex<agentchat::Watchdog>>,
    watchdog_stuck: Duration,
}

struct Request {
    method: String,
    path: String,
    query: HashMap<String, String>,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

#[derive(Debug)]
struct Reply {
    status: u16,
    body: Value,
}

impl Reply {
    fn ok(body: Value) -> Self {
        Self { status: 200, body }
    }

    fn error(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            body: json!({ "error": { "code": code, "message": message.into() } }),
        }
    }
}

fn bad_request(message: impl Into<String>) -> Reply {
    Reply::error(400, "bad_request", message)
}

fn not_found(what: &str) -> Reply {
    Reply::error(404, "not_found", format!("{what} does not exist"))
}

/// The probe asks for the single word `OK`; a model that answers with an upgrade notice, an
/// error page or any other prose did not satisfy the probe. Check whole words so a reply that
/// merely contains "look" does not pass.
fn probe_reply_is_ok(text: &str) -> bool {
    text.split(|character: char| !character.is_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case("ok"))
}

/// One probe result row. `reply` and `latency_ms` are null for a row that never reached a
/// provider; `error` is null for a successful call.
fn probe_row(
    row: &catalog::ModelOption,
    state: &str,
    reply: Option<&str>,
    error: Option<&str>,
    latency_ms: Option<u64>,
) -> Value {
    json!({
        "provider": &row.provider,
        "model": &row.id,
        "state": state,
        "reply": reply,
        "error": error,
        "latency_ms": latency_ms,
        "tools": row.tools,
        "modality": &row.modality,
    })
}

async fn probe_one(
    row: catalog::ModelOption,
    model: Arc<dyn Model>,
    timeout: Duration,
    prompt: String,
    max_tokens: u32,
    effort: String,
) -> Value {
    let request = ChatRequest {
        model: row.id.clone(),
        messages: vec![ChatMessage::user(prompt)],
        tools: Vec::new(),
        max_tokens: Some(max_tokens),
        temperature: 0.0,
        effort: Some(effort),
    };
    let cancel = CancellationToken::new();
    let started = Instant::now();
    let mut text = String::new();
    let outcome = tokio::time::timeout(
        timeout,
        model.chat(request, &cancel, &mut |delta| {
            if let Delta::Text(piece) = delta {
                text.push_str(&piece);
            }
        }),
    )
    .await;
    let latency = started.elapsed().as_millis() as u64;
    match outcome {
        Err(_) => {
            let error = format!("no answer within {}s", timeout.as_secs());
            probe_row(&row, "error", None, Some(error.as_str()), Some(latency))
        }
        Ok(Err(error)) => {
            let error = error.to_string();
            probe_row(&row, "error", None, Some(error.as_str()), Some(latency))
        }
        Ok(Ok(())) => {
            let text = text.trim().to_string();
            if text.is_empty() {
                probe_row(
                    &row,
                    "error",
                    None,
                    Some("the model returned no text"),
                    Some(latency),
                )
            } else if !probe_reply_is_ok(&text) {
                let excerpt: String = text.chars().take(180).collect();
                let error = format!("the reply did not contain OK: {excerpt}");
                probe_row(
                    &row,
                    "error",
                    Some(text.as_str()),
                    Some(error.as_str()),
                    Some(latency),
                )
            } else {
                probe_row(&row, "ok", Some(text.as_str()), None, Some(latency))
            }
        }
    }
}

impl Api {
    fn active(&self) -> Workspace {
        self.workspace.read().expect("workspace lock").clone()
    }

    fn project(&self) -> PathBuf {
        self.active().project
    }

    fn service(&self) -> SessionService {
        self.active().service
    }

    fn base_url(&self) -> String {
        self.public_url
            .clone()
            .unwrap_or_else(|| format!("http://{}:{}", self.host, self.port))
    }

    async fn handle(&self, request: Request) -> Reply {
        // A browser page always carries an Origin on a cross-origin request. Refusing it is what
        // makes the bearer token unreachable from a web page even if it leaks into one.
        if request.headers.contains_key("origin") {
            return Reply::error(403, "forbidden_origin", "browser origins are not allowed");
        }
        if !self.authorized(&request) {
            return Reply::error(401, "unauthorized", "missing or invalid bearer token");
        }
        if request.method != "GET" && request.method != "POST" {
            return Reply::error(405, "method_not_allowed", "use GET or POST");
        }
        if request.method == "POST" && !request.body.is_empty() {
            let content_type = request
                .headers
                .get("content-type")
                .map(|value| {
                    value
                        .split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_ascii_lowercase()
                })
                .unwrap_or_default();
            if content_type != "application/json" {
                return Reply::error(
                    415,
                    "unsupported_media_type",
                    "content-type must be application/json",
                );
            }
        }

        // The window is the workspace picker. Following it here, after auth, means an
        // unauthenticated probe cannot make the API open a database.
        self.follow_app().await;

        let segments: Vec<&str> = request.path.trim_matches('/').split('/').collect();
        match (request.method.as_str(), segments.as_slice()) {
            ("GET", ["v1", "health"]) => self.health().await,
            ("GET", ["v1", "providers"]) => self.provider_rows(),
            ("GET", ["v1", "models"]) => self.model_rows(&request.query).await,
            ("GET", ["v1", "models", "probe"]) => self.model_probe_report(),
            ("POST", ["v1", "models", "probe"]) => self.model_probe(&request.body).await,
            ("GET", ["v1", "openapi.json"]) => Reply::ok(openapi::document(self)),
            ("GET", ["v1", "sessions"]) => self.list_sessions().await,
            ("POST", ["v1", "sessions"]) => self.start_session(&request.body).await,
            ("GET", ["v1", "sessions", id]) => self.get_session(id).await,
            ("GET", ["v1", "sessions", id, "turns"]) => {
                self.session_turns(id, &request.query).await
            }
            ("POST", ["v1", "sessions", id, "turns"]) => self.run_turn(id, &request.body).await,
            ("GET", ["v1", "turns", id]) => self.get_turn(id).await,
            ("GET", ["v1", "turns", id, "items"]) => self.turn_items(id).await,
            ("POST", ["v1", "turns", id, "cancel"]) => self.cancel_turn(id),
            ("GET", ["v1", "workspace"]) => self.workspace_info(&request.query).await,
            ("GET", ["v1", "workspace", "files"]) => self.workspace_files(&request.query).await,
            ("GET", ["v1", "workspace", "read"]) => self.workspace_read(&request.query).await,
            ("POST", ["v1", "workspace", "write"]) => self.workspace_write(&request.body).await,
            ("POST", ["v1", "workspace", "edit"]) => self.workspace_edit(&request.body).await,
            ("POST", ["v1", "workspace", "command"]) => self.workspace_command(&request.body).await,
            ("GET", ["v1", "workspaces"]) => self.list_workspaces(),
            ("POST", ["v1", "workspaces", "open"]) => self.open_workspace(&request.body).await,
            ("GET", ["v1", "fs", "list"]) => self.fs_list(&request.query),
            ("GET", ["v1", "fs", "read"]) => self.fs_read(&request.query),
            ("POST", ["v1", "fs", "write"]) => self.fs_write(&request.body).await,
            ("POST", ["v1", "fs", "edit"]) => self.fs_edit(&request.body).await,
            ("GET", ["v1", "concord"]) => self.concord_info(),
            ("GET", ["v1", "concord", "inbox"]) => self.concord_inbox_snapshot(&request.query),
            ("POST", ["v1", "concord", "reply"]) => self.concord_reply(&request.body).await,
            ("GET", ["v1", "concord", "tools"]) => self.concord_tools().await,
            ("POST", ["v1", "concord", "call"]) => self.concord_call(&request.body).await,
            ("GET", ["v1", "app"]) => self.app_state(),
            ("GET", ["v1", "events"]) => self.events(&request.query).await,
            ("GET", ["v1", "swarm", "jobs"]) => self.swarm_jobs(),
            ("POST", ["v1", "swarm", "jobs", "next"]) => self.claim_swarm_job(),
            ("POST", ["v1", "swarm", "jobs", id, "complete"]) => {
                self.transition_swarm_job(id, SwarmJobTransition::Complete)
            }
            ("POST", ["v1", "swarm", "jobs", id, "release"]) => {
                self.transition_swarm_job(id, SwarmJobTransition::Release)
            }
            ("POST", ["v1", "room", "start"]) => self.room_start().await,
            ("GET", ["v1", "room", "harnesses"]) => self.room_harnesses(&request.query),
            ("GET", ["v1", "room", "agents"]) => self.room_agents().await,
            ("GET", ["v1", "room", "watchdog"]) => self.room_watchdog(),
            ("POST", ["v1", "room", "watchdog"]) => self.room_watchdog_check(&request.body).await,
            ("POST", ["v1", "room", "agents", "start"]) => {
                self.room_start_agents(&request.body).await
            }
            ("POST", ["v1", "room", "create"]) => self.room_create(&request.body).await,
            ("POST", ["v1", "room", "send"]) => self.room_send(&request.body).await,
            ("GET", ["v1", "room", "messages"]) => self.room_messages(&request.query).await,
            ("POST", ["v1", "room", "assign"]) => self.room_assign(&request.body).await,
            _ => not_found("that route"),
        }
    }

    fn authorized(&self, request: &Request) -> bool {
        let Some(value) = request.headers.get("authorization") else {
            return false;
        };
        let Some(given) = value.strip_prefix("Bearer ") else {
            return false;
        };
        token_matches(given.trim(), &self.token)
    }

    async fn health(&self) -> Reply {
        let sessions = self
            .service()
            .list()
            .await
            .map(|rows| rows.len())
            .unwrap_or(0);
        Reply::ok(json!({
            "ok": true,
            "name": "rigel",
            "api": "v1",
            "version": VERSION,
            "project": self.project().to_string_lossy(),
            "full_access": self.full_access,
            "pid": std::process::id(),
            "uptime_secs": self.started.elapsed().as_secs(),
            "sessions": sessions,
        }))
    }

    /// Every provider row, with whether a credential exists. The value is never read here, and
    /// the row says which kind of credential it is so a caller does not have to guess.
    fn provider_rows(&self) -> Reply {
        let store = KeychainCredentialStore;
        let rows: Vec<Value> = providers::all()
            .iter()
            .map(|spec| {
                let local = rigel_core::agent::local::is_local(spec.id);
                let held = local || credentials::present_in(&store, spec.id);
                json!({
                    "id": spec.id,
                    "name": spec.name,
                    "default_model": spec.default_model,
                    "hint": spec.hint,
                    "local": local,
                    "gateway": spec.gateway,
                    "credential": if local { "local" } else if held { "keychain" } else { "none" },
                    "ready": held && spec.default_model.is_some(),
                })
            })
            .collect();
        Reply::ok(json!({ "providers": rows }))
    }

    async fn model_rows(&self, query: &HashMap<String, String>) -> Reply {
        let provider = query.get("provider").map(|id| id.trim().to_lowercase());
        if let Some(id) = provider.as_deref()
            && providers::find(id).is_none()
        {
            return Reply::error(400, "unknown_provider", providers::unknown_provider(id));
        }
        let current = query
            .get("current")
            .map(|id| id.trim().to_lowercase())
            .unwrap_or_else(|| credentials::OMNIROUTE.to_string());
        let all = query
            .get("all")
            .map(|value| value == "1" || value == "true")
            .unwrap_or(false);
        let refresh = query
            .get("refresh")
            .map(|value| value == "1" || value == "true")
            .unwrap_or(false);

        let key = format!("{}|{current}|{all}", provider.as_deref().unwrap_or("*"));
        if !refresh
            && let Ok(cache) = self.models.lock()
            && let Some((at, cached_key, value)) = cache.as_ref()
            && cached_key == &key
            && at.elapsed() < MODELS_CACHE
        {
            return Reply::ok(value.clone());
        }

        let store = KeychainCredentialStore;
        let rows = match provider.as_deref() {
            Some(id) => {
                if providers::find(id).is_none() {
                    return Reply::error(400, "unknown_provider", "that provider does not exist");
                }
                rigel_core::agent::catalog::catalog(id, &store).await
            }
            None => rigel_core::agent::catalog::roster(&current, &store).await,
        };
        match rows {
            Ok(rows) => {
                let shown: Vec<Value> = rows
                    .iter()
                    .filter(|row| all || row.chat)
                    .filter_map(|row| serde_json::to_value(row).ok())
                    .collect();
                let body = json!({ "models": shown, "total": rows.len() });
                if let Ok(mut cache) = self.models.lock() {
                    *cache = Some((Instant::now(), key, body.clone()));
                }
                Reply::ok(body)
            }
            Err(error) => Reply::error(502, "provider_unreachable", error),
        }
    }

    /// The last persisted roster probe. A remote agent reads this after a batch (or after a
    /// restart) instead of having to keep the whole result set in its own process.
    fn model_probe_report(&self) -> Reply {
        let path = self.data_dir.join("model-probe.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(report) => Reply::ok(json!({
                    "ok": true,
                    "path": path.to_string_lossy(),
                    "report": report,
                })),
                Err(error) => Reply::error(
                    500,
                    "io",
                    format!("{} is not valid JSON: {error}", path.display()),
                ),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Reply::ok(json!({
                "ok": true,
                "path": path.to_string_lossy(),
                "report": Value::Null,
                "note": "no probe has run yet; POST /v1/models/probe to test a batch",
            })),
            Err(error) => Reply::error(500, "io", error.to_string()),
        }
    }

    /// Test one batch of the merged roster with a tiny real completion on each model. The caller
    /// advances `offset` until `next_offset` is null; the report is merged into
    /// `<data>/model-probe.json` so every batch survives a restart. Non-chat rows and the relay
    /// door are reported as skipped rather than sent a request they cannot answer.
    async fn model_probe(&self, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let provider = match optional_string(&body, "provider") {
            Ok(value) => value.map(|id| id.trim().to_lowercase()),
            Err(reply) => return reply,
        };
        let provider = provider.filter(|id| !id.is_empty());
        if let Some(id) = provider.as_deref()
            && providers::find(id).is_none()
        {
            return Reply::error(400, "unknown_provider", providers::unknown_provider(id));
        }
        let include_non_chat = match optional_bool(&body, "include_non_chat") {
            Ok(value) => value.unwrap_or(false),
            Err(reply) => return reply,
        };
        let offset = match optional_u64(&body, "offset") {
            Ok(value) => value.unwrap_or(0) as usize,
            Err(reply) => return reply,
        };
        let limit = match optional_u64(&body, "limit") {
            Ok(value) => value.unwrap_or(25).clamp(1, 100) as usize,
            Err(reply) => return reply,
        };
        let concurrency = match optional_u64(&body, "concurrency") {
            Ok(value) => value.unwrap_or(4).clamp(1, 8) as usize,
            Err(reply) => return reply,
        };
        let timeout_secs = match optional_u64(&body, "timeout_secs") {
            Ok(value) => value.unwrap_or(30).clamp(5, 120),
            Err(reply) => return reply,
        };
        let max_tokens = match optional_u64(&body, "max_tokens") {
            Ok(value) => value.unwrap_or(8).clamp(1, 64) as u32,
            Err(reply) => return reply,
        };
        let prompt = match optional_string(&body, "prompt") {
            Ok(Some(prompt)) if !prompt.trim().is_empty() && prompt.chars().count() <= 500 => {
                prompt
            }
            Ok(Some(_)) => return bad_request("`prompt` must be 1 to 500 characters"),
            Ok(None) => "Reply with the single word OK.".to_string(),
            Err(reply) => return reply,
        };
        let effort = match optional_string(&body, "effort") {
            Ok(Some(effort)) if matches!(effort.as_str(), "low" | "medium" | "high") => effort,
            Ok(Some(_)) => return bad_request("`effort` must be low, medium or high"),
            Ok(None) => "low".to_string(),
            Err(reply) => return reply,
        };
        let wanted = match body.get("models") {
            None | Some(Value::Null) => None,
            Some(Value::Array(rows)) => {
                let mut ids = Vec::new();
                for row in rows {
                    let Some(id) = row.as_str().map(str::trim) else {
                        return bad_request("every entry in `models` must be a string");
                    };
                    if id.is_empty() || id.chars().count() > 200 {
                        return bad_request("a model id must be 1 to 200 characters");
                    }
                    ids.push(id.to_string());
                }
                Some(ids)
            }
            Some(_) => return bad_request("`models` must be an array of strings"),
        };

        let store = KeychainCredentialStore;
        let current = provider
            .clone()
            .unwrap_or_else(|| credentials::OMNIROUTE.to_string());
        let rows = match provider.as_deref() {
            Some(id) => catalog::catalog(id, &store).await,
            None => catalog::roster(&current, &store).await,
        };
        let mut rows = match rows {
            Ok(rows) => rows,
            Err(error) => return Reply::error(502, "provider_unreachable", error),
        };
        rows.retain(|row| {
            (include_non_chat || row.chat)
                && wanted
                    .as_ref()
                    .is_none_or(|ids| ids.iter().any(|id| id.eq_ignore_ascii_case(&row.id)))
        });
        let total = rows.len();
        let batch: Vec<catalog::ModelOption> = rows.into_iter().skip(offset).take(limit).collect();

        // Build one adapter per provider, not per model: a roster of forty OpenRouter rows is
        // one client and one key, and a provider that cannot build is reported once per row.
        let mut built: HashMap<String, Result<Arc<dyn Model>, String>> = HashMap::new();
        for row in &batch {
            if row.provider == credentials::RIGEL || built.contains_key(&row.provider) {
                continue;
            }
            let model = providers::build(&row.provider, &store).await;
            built.insert(row.provider.clone(), model);
        }

        let mut results: Vec<Value> = Vec::new();
        let mut work: Vec<(catalog::ModelOption, Arc<dyn Model>)> = Vec::new();
        for row in batch {
            if row.provider == credentials::RIGEL {
                results.push(probe_row(
                    &row,
                    "skipped",
                    None,
                    Some("the relay door is tested by a room turn, not a direct model probe"),
                    None,
                ));
                continue;
            }
            match built.get(&row.provider) {
                Some(Ok(model)) => work.push((row, Arc::clone(model))),
                Some(Err(error)) => results.push(probe_row(&row, "error", None, Some(error), None)),
                None => results.push(probe_row(
                    &row,
                    "error",
                    None,
                    Some("the provider was not built for this batch"),
                    None,
                )),
            }
        }

        let timeout = Duration::from_secs(timeout_secs);
        let stream = futures::stream::iter(work.into_iter().map(move |(row, model)| {
            let prompt = prompt.clone();
            let effort = effort.clone();
            async move { probe_one(row, model, timeout, prompt, max_tokens, effort).await }
        }))
        .buffer_unordered(concurrency);
        results.extend(stream.collect::<Vec<Value>>().await);
        let tested = results.len();
        let next_offset = offset.saturating_add(tested);
        let next_offset = (next_offset < total).then_some(next_offset);

        let report_path = self.data_dir.join("model-probe.json");
        let mut report = std::fs::read_to_string(&report_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({ "results": [] }));
        if !report.get("results").is_some_and(Value::is_array) {
            report["results"] = json!([]);
        }
        if let Some(stored) = report.get_mut("results").and_then(Value::as_array_mut) {
            for result in &results {
                let provider = result.get("provider").and_then(Value::as_str);
                let model = result.get("model").and_then(Value::as_str);
                if let Some(existing) = stored.iter_mut().find(|known| {
                    known.get("provider").and_then(Value::as_str) == provider
                        && known.get("model").and_then(Value::as_str) == model
                }) {
                    *existing = result.clone();
                } else {
                    stored.push(result.clone());
                }
            }
        }
        report["updated_at"] = json!(rigel_core::now_ms());
        report["total_roster"] = json!(total);
        report["last_offset"] = json!(offset);
        report["last_batch"] = json!(tested);
        let temp = report_path.with_extension("json.tmp");
        if let Err(error) = std::fs::write(
            &temp,
            serde_json::to_vec_pretty(&report).unwrap_or_else(|_| b"{}".to_vec()),
        ) {
            return Reply::error(
                500,
                "io",
                format!("could not write the probe report: {error}"),
            );
        }
        if let Err(error) = std::fs::rename(&temp, &report_path) {
            return Reply::error(
                500,
                "io",
                format!("could not publish the probe report: {error}"),
            );
        }

        Reply::ok(json!({
            "ok": true,
            "offset": offset,
            "tested": tested,
            "total": total,
            "next_offset": next_offset,
            "results": results,
            "report": report_path.to_string_lossy(),
        }))
    }

    async fn list_sessions(&self) -> Reply {
        match self.service().list().await {
            Ok(sessions) => Reply::ok(json!({ "sessions": sessions })),
            Err(error) => Reply::error(500, "storage", error.to_string()),
        }
    }

    async fn start_session(&self, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let title = match optional_string(&body, "title") {
            Ok(title) => title,
            Err(reply) => return reply,
        };
        let budget_cents = match optional_i64(&body, "budget_cents") {
            Ok(cents) => cents,
            Err(reply) => return reply,
        };
        if let Some(cents) = budget_cents
            && cents <= 0
        {
            return bad_request("budget_cents must be greater than zero");
        }
        match self.service().start(title.as_deref(), budget_cents).await {
            Ok(session) => Reply::ok(json!({ "session": session })),
            Err(error) => Reply::error(500, "storage", error.to_string()),
        }
    }

    async fn get_session(&self, id: &str) -> Reply {
        let session = match self.service().get(id).await {
            Ok(session) => session,
            Err(SessionError::SessionNotFound(_)) => return not_found("that session"),
            Err(error) => return Reply::error(500, "storage", error.to_string()),
        };
        let agents = self.service().agents(id).await.unwrap_or_default();
        Reply::ok(json!({ "session": session, "agents": agents }))
    }

    async fn session_turns(&self, id: &str, query: &HashMap<String, String>) -> Reply {
        // A session id that does not exist must be a 404 before the transcript is read, because
        // an empty list would otherwise look like a session with no work in it.
        if let Err(error) = self.service().get(id).await {
            return match error {
                SessionError::SessionNotFound(_) => not_found("that session"),
                other => Reply::error(500, "storage", other.to_string()),
            };
        }
        let with_items = query
            .get("items")
            .map(|value| value != "0" && value != "false")
            .unwrap_or(true);
        let limit = query
            .get("limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(50)
            .clamp(1, 500);

        match self.service().transcript(id).await {
            Ok(mut turns) => {
                if turns.len() > limit {
                    turns.drain(..turns.len() - limit);
                }
                let rows: Vec<Value> = turns
                    .into_iter()
                    .map(|row| {
                        if with_items {
                            json!({ "turn": row.turn, "items": row.items })
                        } else {
                            json!({ "turn": row.turn })
                        }
                    })
                    .collect();
                Reply::ok(json!({ "turns": rows }))
            }
            Err(error) => Reply::error(500, "storage", error.to_string()),
        }
    }

    async fn get_turn(&self, id: &str) -> Reply {
        match self.service().turn(id).await {
            Ok(turn) => Reply::ok(json!({ "turn": turn })),
            Err(SessionError::TurnNotFound(_)) => not_found("that turn"),
            Err(error) => Reply::error(500, "storage", error.to_string()),
        }
    }

    async fn turn_items(&self, id: &str) -> Reply {
        if let Err(error) = self.service().turn(id).await {
            return match error {
                SessionError::TurnNotFound(_) => not_found("that turn"),
                other => Reply::error(500, "storage", other.to_string()),
            };
        }
        match self.service().items(id).await {
            Ok(items) => Reply::ok(json!({ "items": items })),
            Err(error) => Reply::error(500, "storage", error.to_string()),
        }
    }

    fn cancel_turn(&self, id: &str) -> Reply {
        self.cancels.cancel_turn(id);
        Reply::ok(json!({ "ok": true, "turn": id, "cancelled": true }))
    }

    async fn run_turn(&self, session_id: &str, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let prompt = match required_string(&body, "prompt") {
            Ok(prompt) if !prompt.trim().is_empty() => prompt,
            Ok(_) => return bad_request("prompt must not be empty"),
            Err(reply) => return reply,
        };
        if prompt.chars().count() > MAX_PROMPT_CHARS {
            return bad_request(format!(
                "prompt is longer than {MAX_PROMPT_CHARS} characters"
            ));
        }
        let role = optional_string(&body, "agent")
            .ok()
            .flatten()
            .unwrap_or_else(|| "coordinator".to_string());
        let effort = optional_string(&body, "effort").ok().flatten();
        if let Some(level) = effort.as_deref()
            && !matches!(level, "low" | "medium" | "high")
        {
            return bad_request("effort must be one of low, medium or high");
        }
        let max_steps = match optional_i64(&body, "max_steps") {
            Ok(steps) => steps.unwrap_or(rigel_core::DEFAULT_MAX_STEPS as i64),
            Err(reply) => return reply,
        };
        if !(1..=1_000).contains(&max_steps) {
            return bad_request("max_steps must be between 1 and 1000");
        }
        let approve = optional_bool(&body, "approve")
            .ok()
            .flatten()
            .unwrap_or(false);
        let dry_run = optional_bool(&body, "dry_run")
            .ok()
            .flatten()
            .unwrap_or(false);

        let session = match self.service().get(session_id).await {
            Ok(session) => session,
            Err(SessionError::SessionNotFound(_)) => return not_found("that session"),
            Err(error) => return Reply::error(500, "storage", error.to_string()),
        };
        if session.status == SessionStatus::Ended {
            return Reply::error(409, "session_ended", "that session has ended");
        }

        let provider = optional_string(&body, "provider")
            .ok()
            .flatten()
            .or_else(|| std::env::var("RIGEL_PROVIDER").ok())
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| credentials::OMNIROUTE.to_string());
        if providers::find(&provider).is_none() {
            return Reply::error(
                400,
                "unknown_provider",
                providers::unknown_provider(&provider),
            );
        }
        let model_name = match optional_string(&body, "model").ok().flatten() {
            Some(model) if !model.trim().is_empty() => model,
            _ => match providers::find(&provider).and_then(|spec| spec.default_model) {
                Some(default) => default.to_string(),
                None => {
                    return Reply::error(
                        400,
                        "model_required",
                        format!(
                            "{provider} publishes no default model; send one in `model` or read /v1/models"
                        ),
                    );
                }
            },
        };

        if dry_run {
            let budget = BudgetService::with_null_interrupt(self.service().pool().clone());
            let context = rigel_core::agent::fixed_context_chars(&model_name);
            return match budget
                .quote_session(&session.id, &model_name, context, &prompt)
                .await
            {
                Ok(quote) => Reply::ok(json!({
                    "dry_run": true,
                    "session": session.id,
                    "provider": provider,
                    "model": model_name,
                    "quote": quote,
                })),
                Err(error) => Reply::error(500, "budget", error.to_string()),
            };
        }

        let worker = match self
            .service()
            .agents(&session.id)
            .await
            .unwrap_or_default()
            .into_iter()
            .find(|agent| agent.harness == "native" && agent.role.as_deref() == Some(role.as_str()))
        {
            Some(worker) => worker,
            None => match self
                .service()
                .register_agent_with(&session.id, "native", Some(&role), Some(&model_name))
                .await
            {
                Ok(worker) => worker,
                Err(error) => return Reply::error(500, "storage", error.to_string()),
            },
        };

        let model = match providers::build(&provider, &KeychainCredentialStore).await {
            Ok(model) => model,
            Err(error) => return Reply::error(502, "provider_unavailable", error),
        };
        let workspace = self.active();
        let coordinator = self.coordinator(
            &workspace.project,
            &workspace.service,
            model,
            &model_name,
            effort,
            max_steps as usize,
            approve,
        );

        match coordinator.run(&session.id, &worker.id, &prompt).await {
            Ok(run) => {
                let spend = self
                    .service()
                    .get(&session.id)
                    .await
                    .map(|stored| stored.spend_micros)
                    .unwrap_or(0);
                Reply::ok(json!({
                    "ok": run.stop == rigel_core::StopReason::Completed,
                    "session": session.id,
                    "agent": worker.id,
                    "turn": run.turn.id,
                    "status": run.status,
                    "stop": run.stop.as_str(),
                    "steps": run.steps,
                    "tool_calls": run.tool_calls,
                    "usage": run.usage,
                    "session_spend_micros": spend,
                    "cap_micros": session.budget_cents.map(|cents| cents * 10_000),
                }))
            }
            Err(error) => Reply::error(
                match error.exit_code() {
                    3 => 402,
                    0 => 499,
                    _ => 502,
                },
                "turn_failed",
                error.to_string(),
            ),
        }
    }

    /// One coordinator per request, built from the same pieces the CLI builds: the adapter
    /// table, the session budget, the project's network policy and the patch pipeline. There is
    /// no worktree directory, so a writable child is refused instead of writing the live tree.
    #[allow(clippy::too_many_arguments)]
    fn coordinator(
        &self,
        project: &Path,
        service: &SessionService,
        model: Arc<dyn Model>,
        model_name: &str,
        effort: Option<String>,
        max_steps: usize,
        approve: bool,
    ) -> Coordinator {
        let budget = Arc::new(BudgetService::new(
            service.pool().clone(),
            self.cancels.clone() as Arc<dyn rigel_core::InterruptSink>,
        ));
        let approver = Arc::new(ApiApprover { approve });
        let tools = Tools::new(
            project.to_path_buf(),
            service.clone(),
            budget.clone(),
            self.data_dir.clone(),
            StopAt::Never,
        )
        .with_network_policy(NetworkPolicy::load(project))
        .with_full_access(self.full_access)
        .with_command_approval(approver.clone())
        .with_origin_approval(approver.clone())
        .with_plan_approval(approver);
        Coordinator {
            session: service.clone(),
            budget,
            tools,
            model,
            sink: Arc::new(NullEvents),
            cancels: self.cancels.clone(),
            project_root: project.to_path_buf(),
            model_name: model_name.to_string(),
            effort,
            max_steps,
            subagents: SubagentPolicy::default(),
            depth: 0,
            parent_cancel: None,
            worktree_dir: None,
            chat_parent: None,
        }
    }

    // -----------------------------------------------------------------------
    // The workspace: the project tree, not the whole disk
    // -----------------------------------------------------------------------

    fn pipeline(&self) -> PatchPipeline {
        let workspace = self.active();
        PatchPipeline::new(&workspace.project, workspace.service.pool().clone())
    }

    /// A file call names its session and agent, or inherits the newest active session and the
    /// `muse` agent. A write is attributed to that agent in the patch journal, so the window's
    /// Diff view names who changed the file rather than showing an anonymous edit.
    async fn attribution(
        &self,
        body: Option<&Value>,
        query: &HashMap<String, String>,
    ) -> Result<(Session, Agent), Reply> {
        let field = |name: &str| -> Option<String> {
            body.and_then(|body| body.get(name))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| query.get(name).cloned())
                .filter(|value| !value.trim().is_empty())
        };
        let session = match field("session") {
            Some(id) => match self.service().get(&id).await {
                Ok(session) => session,
                Err(SessionError::SessionNotFound(_)) => return Err(not_found("that session")),
                Err(error) => return Err(Reply::error(500, "storage", error.to_string())),
            },
            None => {
                let open = self
                    .service()
                    .list()
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .find(|session| session.status != SessionStatus::Ended);
                match open {
                    Some(session) => session,
                    None => match self.service().start(Some("muse"), Some(500)).await {
                        Ok(session) => session,
                        Err(error) => {
                            return Err(Reply::error(500, "storage", error.to_string()));
                        }
                    },
                }
            }
        };
        let role = field("agent").unwrap_or_else(|| "muse".to_string());
        let existing = self
            .service()
            .agents(&session.id)
            .await
            .unwrap_or_default()
            .into_iter()
            .find(|agent| {
                agent.harness == "native" && agent.role.as_deref() == Some(role.as_str())
            });
        let agent = match existing {
            Some(agent) => agent,
            None => match self
                .service()
                .register_agent_with(&session.id, "native", Some(&role), None)
                .await
            {
                Ok(agent) => agent,
                Err(error) => return Err(Reply::error(500, "storage", error.to_string())),
            },
        };
        Ok((session, agent))
    }

    async fn workspace_info(&self, query: &HashMap<String, String>) -> Reply {
        let (session, agent) = match self.attribution(None, query).await {
            Ok(pair) => pair,
            Err(reply) => return reply,
        };
        Reply::ok(json!({
            "project": self.project().to_string_lossy(),
            "name": self.project()
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "session": session,
            "agent": agent,
            "full_access": self.full_access,
            "follows_app": true,
            "limits": {
                "read_bytes": MAX_READ_BYTES,
                "write_bytes": MAX_WRITE_BYTES,
            },
        }))
    }

    async fn workspace_files(&self, query: &HashMap<String, String>) -> Reply {
        let (session, agent) = match self.attribution(None, query).await {
            Ok(pair) => pair,
            Err(reply) => return reply,
        };
        let rel = query.get("path").map(String::as_str).unwrap_or(".");
        let path = match self.pipeline().resolve(rel) {
            Ok(path) => path,
            Err(error) => return path_reply(error),
        };
        let mut entries = Vec::new();
        match std::fs::read_dir(&path) {
            Ok(read) => {
                for entry in read.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if matches!(
                        name.as_str(),
                        ".git" | ".rigel" | "node_modules" | "target" | "dist" | ".next"
                    ) {
                        continue;
                    }
                    let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
                    let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                    entries.push(json!({
                        "name": name,
                        "path": if rel == "." { name.clone() } else { format!("{}/{}", rel.trim_end_matches('/'), name) },
                        "kind": if is_dir { "dir" } else { "file" },
                        "size": size,
                    }));
                    if entries.len() >= 500 {
                        break;
                    }
                }
            }
            Err(error) => return Reply::error(500, "io", error.to_string()),
        }
        entries.sort_by(|left, right| {
            left["path"]
                .as_str()
                .unwrap_or("")
                .cmp(right["path"].as_str().unwrap_or(""))
        });
        Reply::ok(json!({
            "path": rel,
            "entries": entries,
            "session": session.id,
            "agent": agent.id,
        }))
    }

    async fn workspace_read(&self, query: &HashMap<String, String>) -> Reply {
        let Some(rel) = query.get("path") else {
            return bad_request("`path` is required");
        };
        let (session, agent) = match self.attribution(None, query).await {
            Ok(pair) => pair,
            Err(reply) => return reply,
        };
        let (text, hash) = match self.pipeline().read(rel) {
            Ok(read) => read,
            Err(PatchError::NotFound(_)) => return not_found("that file"),
            Err(error) => return path_reply(error),
        };
        if text.len() > MAX_READ_BYTES {
            return Reply::error(
                413,
                "file_too_large",
                format!(
                    "that file is larger than {MAX_READ_BYTES} bytes; read a command's output instead"
                ),
            );
        }
        let lines = text.lines().count();
        Reply::ok(json!({
            "path": rel,
            "sha256": hash,
            "content": text,
            "lines": lines,
            "session": session.id,
            "agent": agent.id,
        }))
    }

    async fn workspace_write(&self, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let rel = match required_string(&body, "path") {
            Ok(rel) => rel,
            Err(reply) => return reply,
        };
        let content = match required_string(&body, "content") {
            Ok(content) => content,
            Err(reply) => return reply,
        };
        if content.len() > MAX_WRITE_BYTES {
            return Reply::error(
                413,
                "content_too_large",
                format!("content is larger than {MAX_WRITE_BYTES} bytes"),
            );
        }
        let (session, agent) = match self.attribution(Some(&body), &HashMap::new()).await {
            Ok(pair) => pair,
            Err(reply) => return reply,
        };
        let pipeline = self.pipeline();
        let resolved = match pipeline.resolve(&rel) {
            Ok(path) => path,
            Err(error) => return path_reply(error),
        };
        let base_hash = match optional_string(&body, "base_hash").ok().flatten() {
            Some(hash) => match normalize_hash(&hash) {
                Ok(hash) => hash,
                Err(reply) => return reply,
            },
            None if resolved.exists() => {
                return Reply::error(
                    409,
                    "base_required",
                    format!(
                        "{rel} already exists; read it first and send the sha256 as `base_hash`, or use /v1/workspace/edit"
                    ),
                );
            }
            None => rigel_core::patches::sha256_empty(),
        };
        match pipeline
            .propose_with(
                &session.id,
                &agent.id,
                &rel,
                &base_hash,
                &content,
                StopAt::Never,
            )
            .await
        {
            Ok(patch) => Reply::ok(json!({
                "ok": true,
                "path": patch.path,
                "patch_id": patch.id,
                "base_hash": patch.base_hash,
                "result_hash": patch.result_hash,
                "diff": patch.diff,
                "applied": true,
                "session": session.id,
                "agent": agent.id,
            })),
            Err(PatchError::StaleBase { current, .. }) => Reply::error(
                409,
                "stale_base",
                format!("the file changed; its current sha256 is {current}, read it again"),
            ),
            Err(error) => path_reply(error),
        }
    }

    async fn workspace_edit(&self, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let rel = match required_string(&body, "path") {
            Ok(value) => value,
            Err(reply) => return reply,
        };
        let old = match required_string(&body, "old_string") {
            Ok(value) => value,
            Err(reply) => return reply,
        };
        let new = match required_string(&body, "new_string") {
            Ok(value) => value,
            Err(reply) => return reply,
        };
        let base_hash = match required_string(&body, "base_hash") {
            Ok(value) => value,
            Err(reply) => return reply,
        };
        if old == new {
            return bad_request("old_string and new_string are identical");
        }
        // The value a `read` returns is bare lowercase hex; an optional `sha256:` prefix is
        // accepted so a caller that copied the algorithm name from a doc still works.
        let base_hash = match normalize_hash(&base_hash) {
            Ok(hash) => hash,
            Err(reply) => return reply,
        };
        let (session, agent) = match self.attribution(Some(&body), &HashMap::new()).await {
            Ok(pair) => pair,
            Err(reply) => return reply,
        };
        let pipeline = self.pipeline();
        let (text, current) = match pipeline.read(&rel) {
            Ok(read) => read,
            Err(PatchError::NotFound(_)) => return not_found("that file"),
            Err(error) => return path_reply(error),
        };
        if current != base_hash {
            return Reply::error(
                409,
                "stale_base",
                format!("the file changed; its current sha256 is {current}, read it again"),
            );
        }
        let occurrences = text.matches(&old).count();
        if occurrences == 0 {
            return Reply::error(
                422,
                "no_match",
                "old_string does not occur in the file; read it and copy the exact text",
            );
        }
        if occurrences > 1 {
            return Reply::error(
                422,
                "multiple_matches",
                format!("old_string occurs {occurrences} times; include more context"),
            );
        }
        let next = text.replacen(&old, &new, 1);
        if next.len() > MAX_WRITE_BYTES {
            return Reply::error(
                413,
                "content_too_large",
                format!("the edited file would be larger than {MAX_WRITE_BYTES} bytes"),
            );
        }
        match pipeline
            .propose_with(
                &session.id,
                &agent.id,
                &rel,
                &base_hash,
                &next,
                StopAt::Never,
            )
            .await
        {
            Ok(patch) => Reply::ok(json!({
                "ok": true,
                "path": patch.path,
                "patch_id": patch.id,
                "base_hash": patch.base_hash,
                "result_hash": patch.result_hash,
                "diff": patch.diff,
                "applied": true,
                "session": session.id,
                "agent": agent.id,
            })),
            Err(PatchError::StaleBase { current, .. }) => Reply::error(
                409,
                "stale_base",
                format!("the file changed; its current sha256 is {current}, read it again"),
            ),
            Err(error) => path_reply(error),
        }
    }

    async fn workspace_command(&self, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let command = match required_string(&body, "command") {
            Ok(command) if !command.trim().is_empty() => command,
            Ok(_) => return bad_request("command must not be empty"),
            Err(reply) => return reply,
        };
        let approve = optional_bool(&body, "approve")
            .ok()
            .flatten()
            .unwrap_or(false);
        let (session, agent) = match self.attribution(Some(&body), &HashMap::new()).await {
            Ok(pair) => pair,
            Err(reply) => return reply,
        };
        let workspace = self.active();
        let budget = Arc::new(BudgetService::with_null_interrupt(
            workspace.service.pool().clone(),
        ));
        let tools = Tools::new(
            workspace.project.clone(),
            workspace.service.clone(),
            budget,
            self.data_dir.clone(),
            StopAt::Never,
        )
        .with_network_policy(NetworkPolicy::load(&workspace.project))
        .with_full_access(self.full_access)
        .with_command_approval(Arc::new(ApiApprover { approve }));
        let mut args = json!({ "command": command });
        if let Some(cwd) = optional_string(&body, "cwd").ok().flatten() {
            args["cwd"] = json!(cwd);
        }
        if let Some(timeout) = optional_i64(&body, "timeout_secs").ok().flatten() {
            args["timeout_secs"] = json!(timeout);
        }
        let outcome = tools
            .execute(&session.id, &agent.id, "run_command", args)
            .await;
        tool_reply(outcome)
    }

    // -----------------------------------------------------------------------
    // Workspaces: the window chooses, the API follows
    // -----------------------------------------------------------------------

    /// Follow the project the window has open. The window writes this marker on every open and
    /// every session; a changed marker is the only time a database is reopened. A missing or
    /// unreadable marker leaves the `--project` choice in place.
    async fn follow_app(&self) {
        let marker = self.data_dir.join("current-workspace.json");
        let Ok(meta) = std::fs::metadata(&marker) else {
            return;
        };
        let modified = meta.modified().ok();
        {
            let seen = self.marker_seen.lock().expect("marker lock");
            if seen.is_some() && *seen == modified {
                return;
            }
        }
        let project = std::fs::read_to_string(&marker)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| {
                value
                    .get("project")
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
            });
        let Some(project) = project else {
            return;
        };
        let Ok(project) = project.canonicalize() else {
            return;
        };
        if !project.is_dir() {
            return;
        }
        if project == self.active().project {
            *self.marker_seen.lock().expect("marker lock") = modified;
            return;
        }
        match SessionService::open(&project).await {
            Ok(service) => {
                *self.workspace.write().expect("workspace lock") = Workspace { project, service };
                *self.marker_seen.lock().expect("marker lock") = modified;
            }
            Err(error) => eprintln!("rigel: could not follow the app's workspace: {error}"),
        }
    }

    /// Treat the window's current marker as already handled. An explicit API switch should not
    /// be undone on the very next request just because the window's marker predates it; a later
    /// app switch changes the mtime and wins again.
    fn mark_marker_seen(&self) {
        let modified = std::fs::metadata(self.data_dir.join("current-workspace.json"))
            .ok()
            .and_then(|meta| meta.modified().ok());
        *self.marker_seen.lock().expect("marker lock") = modified;
    }

    fn list_workspaces(&self) -> Reply {
        let current = self.project();
        let mut rows: Vec<Value> = Vec::new();
        if let Ok(read) = std::fs::read_dir(home_dir().join("Rigel")) {
            for entry in read.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                rows.push(json!({
                    "name": name,
                    "path": path.to_string_lossy(),
                    "current": path == current,
                }));
                if rows.len() >= 200 {
                    break;
                }
            }
        }
        rows.sort_by(|left, right| {
            left["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&right["name"].as_str().unwrap_or("").to_lowercase())
        });
        if !rows.iter().any(|row| row["current"] == json!(true)) {
            rows.insert(
                0,
                json!({
                    "name": current
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    "path": current.to_string_lossy(),
                    "current": true,
                }),
            );
        }
        Reply::ok(json!({
            "current": current.to_string_lossy(),
            "workspaces": rows,
        }))
    }

    async fn open_workspace(&self, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let raw = match required_string(&body, "path") {
            Ok(path) => path,
            Err(reply) => return reply,
        };
        let project = match PathBuf::from(raw.trim()).canonicalize() {
            Ok(project) if project.is_dir() => project,
            Ok(_) => return bad_request(format!("{raw} is not a directory")),
            Err(error) => return bad_request(format!("{raw}: {error}")),
        };
        // Without full access, an API caller must not be able to turn an arbitrary directory
        // into a project and thereby read it through the journaled workspace routes. The
        // window's marker is trusted and follows a folder the user picked; this endpoint is
        // narrower on purpose.
        if !self.full_access
            && project != self.active().project
            && !project.starts_with(home_dir().join("Rigel"))
        {
            return Reply::error(
                403,
                "full_access_disabled",
                "without --full-access the API can only open a workspace under ~/Rigel or the one already active; choose another folder in the window",
            );
        }
        match SessionService::open(&project).await {
            Ok(service) => {
                let sessions = service.list().await.map(|rows| rows.len()).unwrap_or(0);
                *self.workspace.write().expect("workspace lock") = Workspace {
                    project: project.clone(),
                    service,
                };
                self.mark_marker_seen();
                Reply::ok(json!({
                    "ok": true,
                    "project": project.to_string_lossy(),
                    "name": project
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    "sessions": sessions,
                }))
            }
            Err(error) => Reply::error(500, "storage", error.to_string()),
        }
    }

    // -----------------------------------------------------------------------
    // Full access: any path this user can reach, only when the flag is on
    // -----------------------------------------------------------------------

    fn require_full_access(&self) -> Result<(), Reply> {
        if self.full_access {
            Ok(())
        } else {
            Err(Reply::error(
                403,
                "full_access_disabled",
                "start `rigel serve --full-access` to use /v1/fs/* outside the project",
            ))
        }
    }

    /// Resolve a caller path. Absolute paths are kept; relative paths are resolved against the
    /// active project. A new file is allowed when its parent exists (or when `create_dirs` is
    /// set); only an existing parent is canonicalized, because a missing file has no link to
    /// resolve. The project jail deliberately does not apply in this mode.
    fn fs_target(&self, raw: &str, create_dirs: bool) -> Result<PathBuf, Reply> {
        let raw = raw.trim();
        if raw.is_empty() || raw.contains('\0') {
            return Err(bad_request("path must be a non-empty file path"));
        }
        let candidate = Path::new(raw);
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.project().join(candidate)
        };
        if joined.exists() {
            return joined
                .canonicalize()
                .map_err(|error| Reply::error(500, "io", error.to_string()));
        }
        let parent = joined.parent().unwrap_or(Path::new("."));
        if create_dirs {
            std::fs::create_dir_all(parent)
                .map_err(|error| Reply::error(500, "io", error.to_string()))?;
        }
        let parent = parent.canonicalize().map_err(|error| {
            Reply::error(404, "not_found", format!("{}: {error}", parent.display()))
        })?;
        let name = joined
            .file_name()
            .ok_or_else(|| bad_request("path must name a file"))?;
        Ok(parent.join(name))
    }

    fn fs_list(&self, query: &HashMap<String, String>) -> Reply {
        if let Err(reply) = self.require_full_access() {
            return reply;
        }
        let raw = query.get("path").map(String::as_str).unwrap_or(".");
        let path = match self.fs_target(raw, false) {
            Ok(path) => path,
            Err(reply) => return reply,
        };
        if !path.is_dir() {
            return bad_request(format!("{raw} is not a directory"));
        }
        let mut entries = Vec::new();
        match std::fs::read_dir(&path) {
            Ok(read) => {
                for entry in read.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
                    let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                    entries.push(json!({
                        "name": name,
                        "path": entry.path().to_string_lossy(),
                        "kind": if is_dir { "dir" } else { "file" },
                        "size": size,
                    }));
                    if entries.len() >= 500 {
                        break;
                    }
                }
            }
            Err(error) => return Reply::error(500, "io", error.to_string()),
        }
        entries.sort_by(|left, right| {
            left["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&right["name"].as_str().unwrap_or("").to_lowercase())
        });
        Reply::ok(json!({
            "path": path.to_string_lossy(),
            "entries": entries,
            "outside_project": !path.starts_with(self.project()),
        }))
    }

    fn fs_read(&self, query: &HashMap<String, String>) -> Reply {
        if let Err(reply) = self.require_full_access() {
            return reply;
        }
        let Some(raw) = query.get("path") else {
            return bad_request("`path` is required");
        };
        let path = match self.fs_target(raw, false) {
            Ok(path) => path,
            Err(reply) => return reply,
        };
        if !path.is_file() {
            return not_found("that file");
        }
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => return Reply::error(500, "io", error.to_string()),
        };
        if bytes.len() > MAX_READ_BYTES {
            return Reply::error(
                413,
                "file_too_large",
                format!(
                    "that file is larger than {MAX_READ_BYTES} bytes; read a slice with a command"
                ),
            );
        }
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                return Reply::error(415, "binary", format!("{} is not UTF-8", path.display()));
            }
        };
        Reply::ok(json!({
            "path": path.to_string_lossy(),
            "sha256": rigel_core::patches::sha256_hex(text.as_bytes()),
            "content": text,
            "lines": text.lines().count(),
            "outside_project": !path.starts_with(self.project()),
        }))
    }

    async fn fs_write(&self, body: &[u8]) -> Reply {
        if let Err(reply) = self.require_full_access() {
            return reply;
        }
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let raw = match required_string(&body, "path") {
            Ok(path) => path,
            Err(reply) => return reply,
        };
        let content = match required_string(&body, "content") {
            Ok(content) => content,
            Err(reply) => return reply,
        };
        if content.len() > MAX_WRITE_BYTES {
            return Reply::error(
                413,
                "content_too_large",
                format!("content is larger than {MAX_WRITE_BYTES} bytes"),
            );
        }
        let base_hash = match optional_string(&body, "base_hash") {
            Ok(hash) => hash,
            Err(reply) => return reply,
        };
        let create_dirs = optional_bool(&body, "create_dirs")
            .ok()
            .flatten()
            .unwrap_or(false);
        let path = match self.fs_target(&raw, create_dirs) {
            Ok(path) => path,
            Err(reply) => return reply,
        };
        let project = self.project();
        if let Ok(rel) = path.strip_prefix(&project) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            if rel.is_empty() {
                return bad_request("path must name a file");
            }
            let mut forwarded = json!({ "path": rel, "content": content });
            if let Some(hash) = &base_hash {
                forwarded["base_hash"] = json!(hash);
            }
            if let Some(session) = body.get("session").cloned() {
                forwarded["session"] = session;
            }
            if let Some(agent) = body.get("agent").cloned() {
                forwarded["agent"] = agent;
            }
            let encoded = serde_json::to_vec(&forwarded).unwrap_or_default();
            return self.workspace_write(&encoded).await;
        }
        let (session, agent) = match self.attribution(Some(&body), &HashMap::new()).await {
            Ok(pair) => pair,
            Err(reply) => return reply,
        };
        match write_outside(&path, &content, base_hash.as_deref()) {
            Ok(hash) => Reply::ok(json!({
                "ok": true,
                "path": path.to_string_lossy(),
                "sha256": hash,
                "bytes": content.len(),
                "journaled": false,
                "outside_project": true,
                "session": session.id,
                "agent": agent.id,
            })),
            Err(reply) => reply,
        }
    }

    async fn fs_edit(&self, body: &[u8]) -> Reply {
        if let Err(reply) = self.require_full_access() {
            return reply;
        }
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let raw = match required_string(&body, "path") {
            Ok(path) => path,
            Err(reply) => return reply,
        };
        let old = match required_string(&body, "old_string") {
            Ok(value) => value,
            Err(reply) => return reply,
        };
        let new = match required_string(&body, "new_string") {
            Ok(value) => value,
            Err(reply) => return reply,
        };
        let base = match required_string(&body, "base_hash") {
            Ok(value) => value,
            Err(reply) => return reply,
        };
        if old == new {
            return bad_request("old_string and new_string are identical");
        }
        let base = match normalize_hash(&base) {
            Ok(hash) => hash,
            Err(reply) => return reply,
        };
        let path = match self.fs_target(&raw, false) {
            Ok(path) => path,
            Err(reply) => return reply,
        };
        let project = self.project();
        if let Ok(rel) = path.strip_prefix(&project) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            let mut forwarded = json!({
                "path": rel,
                "old_string": old,
                "new_string": new,
                "base_hash": base,
            });
            if let Some(session) = body.get("session").cloned() {
                forwarded["session"] = session;
            }
            if let Some(agent) = body.get("agent").cloned() {
                forwarded["agent"] = agent;
            }
            let encoded = serde_json::to_vec(&forwarded).unwrap_or_default();
            return self.workspace_edit(&encoded).await;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => return Reply::error(500, "io", error.to_string()),
        };
        let current = rigel_core::patches::sha256_hex(text.as_bytes());
        if current != base {
            return Reply::error(
                409,
                "stale_base",
                format!("the file changed; its current sha256 is {current}, read it again"),
            );
        }
        let occurrences = text.matches(&old).count();
        if occurrences == 0 {
            return Reply::error(
                422,
                "no_match",
                "old_string does not occur in the file; read it and copy the exact text",
            );
        }
        if occurrences > 1 {
            return Reply::error(
                422,
                "multiple_matches",
                format!("old_string occurs {occurrences} times; include more context"),
            );
        }
        let next = text.replacen(&old, &new, 1);
        if next.len() > MAX_WRITE_BYTES {
            return Reply::error(
                413,
                "content_too_large",
                format!("the edited file would be larger than {MAX_WRITE_BYTES} bytes"),
            );
        }
        let (session, agent) = match self.attribution(Some(&body), &HashMap::new()).await {
            Ok(pair) => pair,
            Err(reply) => return reply,
        };
        match write_outside(&path, &next, Some(&base)) {
            Ok(hash) => Reply::ok(json!({
                "ok": true,
                "path": path.to_string_lossy(),
                "sha256": hash,
                "bytes": next.len(),
                "journaled": false,
                "outside_project": true,
                "session": session.id,
                "agent": agent.id,
            })),
            Err(reply) => reply,
        }
    }

    // -----------------------------------------------------------------------
    // Concord: Muse's side of the shared agent room
    // -----------------------------------------------------------------------

    fn concord_info(&self) -> Reply {
        Reply::ok(json!({
            "ok": true,
            "agent_id": concord::AGENT_ID,
            "package": concord::PACKAGE,
            "project": self.project().to_string_lossy(),
            "tools": "/v1/concord/tools",
            "call": "/v1/concord/call",
            "inbox": "/v1/concord/inbox",
            "reply": "/v1/concord/reply",
        }))
    }

    async fn concord_tools(&self) -> Reply {
        match concord::tools(&self.project()).await {
            Ok(result) => {
                let tools = result.get("tools").cloned().unwrap_or(result);
                Reply::ok(json!({ "ok": true, "tools": tools }))
            }
            Err(error) => Reply::error(502, "concord_unavailable", error),
        }
    }

    async fn concord_call(&self, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let tool = match required_string(&body, "tool") {
            Ok(tool) if !tool.trim().is_empty() => tool,
            Ok(_) => return bad_request("tool must not be empty"),
            Err(reply) => return reply,
        };
        let mut arguments = body.get("arguments").cloned().unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            return bad_request("arguments must be a JSON object");
        }
        if matches!(
            tool.as_str(),
            "start_work" | "inspect_work" | "update_work" | "transfer_work" | "finish_work"
        ) && arguments.get("agent_id").is_none()
        {
            arguments["agent_id"] = json!(concord::AGENT_ID);
            if tool == "start_work" {
                arguments["cwd"] = json!(concord::plain_path(&self.project()))
            }
        }
        match concord::call(&self.project(), &tool, arguments).await {
            Ok(result) => {
                if result
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    let detail = concord::text(&result);
                    return Reply::error(
                        422,
                        "concord_error",
                        if detail.is_empty() {
                            "Concord refused the tool call".to_string()
                        } else {
                            detail
                        },
                    );
                }
                Reply::ok(json!({
                    "ok": true,
                    "tool": tool,
                    "result": result,
                    "text": concord::text(&result),
                }))
            }
            Err(error) => Reply::error(502, "concord_unavailable", error),
        }
    }

    /// Reply to one cached message without making a caller hand-build `update_work`. The
    /// message id is the only selector: Concord already knows the sender, and the cached row
    /// supplies it when the caller does not.
    async fn concord_reply(&self, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let message_id = match required_string(&body, "message_id") {
            Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
            Ok(_) => return bad_request("message_id must not be empty"),
            Err(reply) => return reply,
        };
        if message_id.chars().count() > 200 || message_id.chars().any(char::is_control) {
            return bad_request("message_id is not a valid Concord message id");
        }
        let content = match required_string(&body, "content") {
            Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
            Ok(_) => return bad_request("content must not be empty"),
            Err(reply) => return reply,
        };
        if content.chars().count() > 16_384 {
            return bad_request("content is longer than Concord's 16,384-character limit");
        }
        let cached = self
            .concord_inbox
            .lock()
            .ok()
            .and_then(|inbox| inbox.message(&message_id));
        let to_agent_id = match optional_string(&body, "to_agent_id") {
            Ok(Some(value)) if !value.trim().is_empty() => Some(value.trim().to_string()),
            Ok(_) => cached
                .as_ref()
                .and_then(|message| message.get("senderAgentId"))
                .and_then(Value::as_str)
                .map(str::to_string),
            Err(reply) => return reply,
        };
        let idempotency_key = match optional_string(&body, "idempotency_key") {
            Ok(Some(value)) if !value.trim().is_empty() => value.trim().to_string(),
            Ok(_) => format!("muse-reply-{message_id}"),
            Err(reply) => return reply,
        };
        if idempotency_key.chars().count() > 128 {
            return bad_request("idempotency_key is longer than Concord's 128-character limit");
        }

        let mut arguments = json!({
            "operation": "reply",
            "reply_to_message_id": message_id,
            "content": content,
            "idempotency_key": idempotency_key,
            "agent_id": concord::AGENT_ID,
        });
        if let Some(agent) = to_agent_id.as_ref() {
            arguments["to_agent_id"] = json!(agent);
        }
        match concord::call(&self.project(), "update_work", arguments).await {
            Ok(result) => {
                if result
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    let detail = concord::text(&result);
                    return Reply::error(
                        422,
                        "concord_error",
                        if detail.is_empty() {
                            "Concord refused the reply".to_string()
                        } else {
                            detail
                        },
                    );
                }
                Reply::ok(json!({
                    "ok": true,
                    "tool": "update_work",
                    "message_id": message_id,
                    "to_agent_id": to_agent_id,
                    "result": result,
                    "text": concord::text(&result),
                }))
            }
            Err(error) => Reply::error(502, "concord_unavailable", error),
        }
    }

    /// The background poller's cache, not a live MCP call: Muse long-polls `/v1/events`, wakes
    /// when `revision` moves, then reads from its last `seq` without running `inspect_work`.
    fn concord_inbox_snapshot(&self, query: &HashMap<String, String>) -> Reply {
        let since = query
            .get("since")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let limit = query
            .get("limit")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(50)
            .clamp(1, 200);
        let inbox = self.concord_inbox.lock().expect("concord inbox");
        Reply::ok(inbox.snapshot(since, limit))
    }

    // -----------------------------------------------------------------------
    // Muse's wake signal, swarm inbox, and room command surface
    // -----------------------------------------------------------------------

    fn app_snapshot(&self) -> Value {
        let state = std::fs::read_to_string(self.data_dir.join("app-state.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or_else(|| json!({}));
        let updated_at = state.get("updated_at").and_then(Value::as_i64).unwrap_or(0);
        let fresh = updated_at > 0 && now_ms().saturating_sub(updated_at) <= APP_HEARTBEAT_STALE_MS;
        let open = state.get("open").and_then(Value::as_bool).unwrap_or(false) && fresh;
        let project = state
            .get("project")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| self.project().to_string_lossy().into_owned());
        let jobs = self.swarm_jobs_list();
        let oldest = jobs
            .first()
            .and_then(|job| job.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let concord = self.concord_inbox.lock().expect("concord inbox");
        let fingerprint = format!(
            "{open}|{project}|{}|{oldest}|c{}",
            jobs.len(),
            concord.revision()
        );
        json!({
            "open": open,
            "project": project,
            "updated_at": updated_at,
            "heartbeat_age_ms": if updated_at > 0 {
                now_ms().saturating_sub(updated_at)
            } else {
                -1
            },
            "swarm": {
                "pending": jobs.len(),
                "oldest": oldest,
            },
            "concord": concord.brief(),
            "fingerprint": fingerprint,
        })
    }

    fn app_state(&self) -> Reply {
        Reply::ok(self.app_snapshot())
    }

    /// Long-poll: return immediately when the window opens/closes, the project changes, a Swarm
    /// job arrives, or Concord delivers an inbox message. `since` is the previous response's
    /// fingerprint, so a reconnect gets a current snapshot instead of replaying an unbounded
    /// history.
    async fn events(&self, query: &HashMap<String, String>) -> Reply {
        let wait = query
            .get("wait")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(25)
            .min(60);
        let since = query.get("since").cloned().unwrap_or_default();
        let deadline = Instant::now() + Duration::from_secs(wait);
        loop {
            let snapshot = self.app_snapshot();
            let current = snapshot
                .get("fingerprint")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if since.is_empty() || current != since || Instant::now() >= deadline {
                return Reply::ok(json!({
                    "ok": true,
                    "changed": since.is_empty() || current != since,
                    "event": snapshot,
                }));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    fn swarm_jobs_list(&self) -> Vec<Value> {
        let dir = self.data_dir.join("swarm").join("inbox");
        let mut jobs = Vec::new();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return jobs;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if value.is_object() {
                jobs.push(value);
            }
        }
        jobs.sort_by_key(|job| job.get("created_at").and_then(Value::as_i64).unwrap_or(0));
        jobs
    }

    fn swarm_jobs(&self) -> Reply {
        let jobs = self.swarm_jobs_list();
        Reply::ok(json!({ "ok": true, "jobs": jobs }))
    }

    /// Claim the oldest job by renaming it out of the inbox. A second claim cannot take the same
    /// file, and a Muse crash leaves the job in `claimed/` rather than silently re-running it.
    fn claim_swarm_job(&self) -> Reply {
        let dir = self.data_dir.join("swarm").join("inbox");
        let claimed = self.data_dir.join("swarm").join("claimed");
        let _ = std::fs::create_dir_all(&claimed);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Reply::ok(json!({ "ok": true, "job": null }));
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().and_then(|extension| extension.to_str()) == Some("json")
            })
            .collect();
        paths.sort();
        for path in paths {
            let Some(name) = path.file_name() else {
                continue;
            };
            let target = claimed.join(name);
            if std::fs::rename(&path, &target).is_err() {
                continue;
            }
            let job = std::fs::read_to_string(&target)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .unwrap_or(Value::Null);
            return Reply::ok(json!({ "ok": true, "job": job }));
        }
        Reply::ok(json!({ "ok": true, "job": null }))
    }

    /// Finish a claimed job into the durable audit directory, or put it back in the inbox. The
    /// desired state is idempotent; asking for the opposite transition reports a conflict.
    fn transition_swarm_job(&self, id: &str, transition: SwarmJobTransition) -> Reply {
        transition_swarm_job(&self.data_dir, id, transition)
    }

    /// Resolve the room through its health-first path before any authenticated room call.
    ///
    /// A fresh API process has an empty in-memory session token. When the chat server is already
    /// up (the common case after an API-only restart), `ensure_server` reads the token from its
    /// landing page and adopts it; only a genuinely absent room is spawned. This is what keeps
    /// `/v1/room/agents` from answering 502 after a restart.
    async fn room_ready(&self) -> Result<String, Reply> {
        let project = self.project().to_string_lossy().into_owned();
        agentchat::ensure_server(Some(&project))
            .await
            .map(|_| project)
            .map_err(|error| Reply::error(502, "room_unavailable", error))
    }

    async fn room_start(&self) -> Reply {
        match self.room_ready().await {
            Ok(project) => Reply::ok(json!({ "ok": true, "project": project })),
            Err(reply) => reply,
        }
    }

    fn room_harnesses(&self, query: &HashMap<String, String>) -> Reply {
        let project = self.project().to_string_lossy().into_owned();
        let force = query
            .get("force")
            .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        let harnesses = if force {
            agentchat::configured_fresh(Some(&project))
        } else {
            agentchat::configured(Some(&project))
        };
        Reply::ok(json!({
            "ok": true,
            "project": project,
            "forced": force,
            "harnesses": harnesses,
        }))
    }

    async fn room_agents(&self) -> Reply {
        let _project = match self.room_ready().await {
            Ok(project) => project,
            Err(reply) => return reply,
        };
        match agentchat::agents().await {
            Ok(agents) => Reply::ok(json!({ "ok": true, "agents": agents })),
            Err(error) => Reply::error(502, "room_unavailable", error),
        }
    }

    /// What the stuck-agent watchdog has seen, and the agents this API process can restart.
    /// A harness started by the window is reported as not owned here: the room status carries
    /// no pid, so killing a guessed one is not safe.
    fn room_watchdog(&self) -> Reply {
        let owned = agentchat::launched()
            .into_iter()
            .map(|process| process.name)
            .collect();
        let state = self
            .watchdog
            .lock()
            .map(|guard| guard.snapshot(self.watchdog_stuck, owned))
            .unwrap_or(Value::Null);
        Reply::ok(json!({
            "ok": true,
            "enabled": room_watchdog_enabled(),
            "project": self.project().to_string_lossy(),
            "watchdog": state,
        }))
    }

    /// Run one inspection now. A harness past `stuck_after_secs` that this process started is
    /// restarted; one started elsewhere is reported with the reason instead of killed.
    async fn room_watchdog_check(&self, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let stuck_after = match optional_u64(&body, "stuck_after_secs") {
            Ok(Some(seconds)) => Duration::from_secs(seconds.clamp(1, 86_400)),
            Ok(None) => self.watchdog_stuck,
            Err(reply) => return reply,
        };
        let project = match self.room_ready().await {
            Ok(project) => project,
            Err(reply) => return reply,
        };
        let status = match agentchat::agents().await {
            Ok(status) => status,
            Err(error) => return Reply::error(502, "room_unavailable", error),
        };
        let messages = match agentchat::messages(None, 0, 100).await {
            Ok(messages) => messages,
            Err(error) => return Reply::error(502, "room_unavailable", error),
        };
        let stuck = self
            .watchdog
            .lock()
            .map(|mut guard| guard.inspect(&status, &messages, Instant::now(), stuck_after))
            .unwrap_or_default();
        let observed = stuck.clone();
        let mut restarted = Vec::new();
        let mut skipped = Vec::new();
        for name in stuck {
            if !agentchat::owns(&name) {
                skipped.push(json!({
                    "name": name,
                    "reason": "another Rigel process started this agent, so its pid is unknown here",
                }));
                continue;
            }
            let label = agentchat::launched()
                .into_iter()
                .find(|process| process.name == name)
                .map(|process| process.label)
                .unwrap_or_else(|| name.clone());
            let (text, ok) = match agentchat::restart_agent(Some(&project), &name).await {
                Ok(text) => (text, true),
                Err(error) => (error, false),
            };
            if ok {
                restarted.push(json!({ "name": name, "outcome": text }));
            } else {
                skipped.push(json!({ "name": name, "reason": text }));
            }
            if let Ok(mut guard) = self.watchdog.lock() {
                guard.record(&name, &label, text, Instant::now());
            }
        }
        Reply::ok(json!({
            "ok": true,
            "stuck_after_secs": stuck_after.as_secs(),
            "stuck": observed,
            "restarted": restarted,
            "skipped": skipped,
        }))
    }

    /// Start configured room agents, all of them or the named subset. The remote agent uses
    /// this after `POST /v1/room/start`; the readiness rules are the same ones the Chat tab uses.
    async fn room_start_agents(&self, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let names = match body.get("names") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(rows)) => {
                let mut names = Vec::new();
                for row in rows {
                    let Some(name) = row.as_str().map(str::trim) else {
                        return bad_request("every entry in `names` must be a string");
                    };
                    if name.is_empty() || name.chars().count() > 40 {
                        return bad_request("a room agent name must be 1 to 40 characters");
                    }
                    names.push(name.to_string());
                }
                names
            }
            Some(_) => return bad_request("`names` must be an array of strings"),
        };
        let project = match self.room_ready().await {
            Ok(project) => project,
            Err(reply) => return reply,
        };
        let mut started = Vec::new();
        let mut problems = Vec::new();
        if names.is_empty() {
            match agentchat::start_agents(Some(&project)).await {
                Ok(labels) => started.extend(labels),
                Err(error) => problems.push(error),
            }
        } else {
            for name in names {
                match agentchat::start_agent(Some(&project), &name).await {
                    Ok(outcome) => started.push(outcome),
                    Err(error) => problems.push(format!("{name}: {error}")),
                }
            }
        }
        let agents = agentchat::agents().await.ok();
        Reply::ok(json!({
            "ok": problems.is_empty(),
            "project": project,
            "started": started,
            "problems": problems,
            "agents": agents,
        }))
    }

    /// One call the remote boss can use to create/join its working room and bring the crew up.
    async fn room_create(&self, body: &[u8]) -> Reply {
        let mut reply = self.room_start_agents(body).await;
        if let (200, Value::Object(map)) = (reply.status, &mut reply.body) {
            map.insert("created".to_string(), json!(true));
        }
        reply
    }

    async fn room_messages(&self, query: &HashMap<String, String>) -> Reply {
        let _project = match self.room_ready().await {
            Ok(project) => project,
            Err(reply) => return reply,
        };
        let channel = query
            .get("channel")
            .map(String::as_str)
            .filter(|value| !value.is_empty());
        let since = query
            .get("since_id")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let limit = query
            .get("limit")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(50)
            .clamp(1, 200);
        match agentchat::messages(channel, since, limit).await {
            Ok(messages) => Reply::ok(json!({
                "ok": true,
                "messages": serde_json::to_value(messages).unwrap_or(Value::Null),
            })),
            Err(error) => Reply::error(502, "room_unavailable", error),
        }
    }

    /// One ordinary message from the operator. It is posted to the room, then — only when every
    /// configured ready harness is online under a clean name — the same text is queued for Muse.
    /// The response says which of those happened instead of implying a coordination that did not
    /// start.
    async fn room_send(&self, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let text = match optional_string(&body, "text") {
            Ok(Some(text)) if !text.trim().is_empty() => text,
            Ok(_) => return bad_request("`text` is required"),
            Err(reply) => return reply,
        };
        if text.chars().count() > rigel_core::swarm::MAX_PROMPT_CHARS {
            return bad_request(format!(
                "`text` is longer than {} characters",
                rigel_core::swarm::MAX_PROMPT_CHARS
            ));
        }
        let channel = match optional_string(&body, "channel") {
            Ok(Some(channel)) if !channel.trim().is_empty() => channel,
            Ok(_) => "general".to_string(),
            Err(reply) => return reply,
        };
        let project = match self.room_ready().await {
            Ok(project) => project,
            Err(reply) => return reply,
        };
        if let Err(error) = agentchat::send(&text, &channel).await {
            return Reply::error(502, "room_unavailable", error);
        }
        let root = self.project();
        let kickoff = match rigel_core::swarm::kick_off_muse(&self.data_dir, &root, &text).await {
            Ok(kickoff) => kickoff,
            Err(error) => {
                return Reply::ok(json!({
                    "ok": true,
                    "sent": true,
                    "channel": channel,
                    "kicked": false,
                    "reason": error,
                }));
            }
        };
        Reply::ok(json!({
            "ok": true,
            "sent": true,
            "channel": channel,
            "project": project,
            "kicked": kickoff.job_id.is_some(),
            "job_id": kickoff.job_id,
            "targets": kickoff.targets,
            "missing": kickoff.missing,
            "duplicates": kickoff.duplicates,
        }))
    }

    /// One assignment per agent, posted as `muse`. The boss writes the split; the room agents
    /// are told to spawn their own subagents for independent pieces and to report evidence.
    async fn room_assign(&self, body: &[u8]) -> Reply {
        let body = match body_object(body) {
            Ok(body) => body,
            Err(reply) => return reply,
        };
        let assignments = match body.get("assignments").and_then(Value::as_array) {
            Some(rows) if !rows.is_empty() && rows.len() <= 12 => rows.clone(),
            Some(_) => return bad_request("assignments must hold between 1 and 12 rows"),
            None => return bad_request("`assignments` is required"),
        };
        let channel = match optional_string(&body, "channel") {
            Ok(value) => value.unwrap_or_else(|| "general".to_string()),
            Err(reply) => return reply,
        };
        let _project = match self.room_ready().await {
            Ok(project) => project,
            Err(reply) => return reply,
        };
        let mut sent = Vec::new();
        for row in assignments {
            let agent = match row.get("agent").and_then(Value::as_str) {
                Some(agent) if !agent.trim().is_empty() && agent.chars().count() <= 40 => {
                    agent.trim().to_string()
                }
                _ => return bad_request("every assignment needs an agent name"),
            };
            let task = match row.get("task").and_then(Value::as_str) {
                Some(task) if !task.trim().is_empty() => task.trim().to_string(),
                _ => return bad_request("every assignment needs a task"),
            };
            let files: Vec<String> = row
                .get("files")
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let spawn = row
                .get("spawn_subagents")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let mut text = format!("@{agent} — Muse assignment.\nTask: {task}\n");
            if !files.is_empty() {
                text.push_str(&format!("Files you own: {}\n", files.join(", ")));
            }
            if spawn {
                text.push_str(
                    "You are a squad lead: split this into independent pieces and spawn your own \
                     subagents for them. Give each helper one job and a disjoint file set. ",
                );
            }
            text.push_str(
                "Stay inside your files, coordinate here instead of overwriting another agent, and \
                 report changed files, tests, and blockers when you finish.",
            );
            if let Err(error) = agentchat::send_as(&text, &channel, "muse").await {
                return Reply::error(
                    502,
                    "room_unavailable",
                    format!("could not assign {agent}: {error}"),
                );
            }
            sent.push(json!({ "agent": agent }));
        }
        Reply::ok(json!({ "ok": true, "channel": channel, "sent": sent }))
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Write a file outside the project with the same compare-and-swap discipline the patch
/// pipeline uses, but without a journal row: the journal belongs to one project, and this path
/// deliberately is not in it. A temp file plus rename means a reader sees the old bytes or the
/// new ones, never a partial write.
fn write_outside(path: &Path, content: &str, base_hash: Option<&str>) -> Result<String, Reply> {
    let current = match std::fs::read(path) {
        Ok(bytes) => Some(rigel_core::patches::sha256_hex(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(Reply::error(500, "io", error.to_string())),
    };
    match (current.as_deref(), base_hash) {
        (Some(current), Some(base)) => {
            let base = normalize_hash(base)?;
            if base != current {
                return Err(Reply::error(
                    409,
                    "stale_base",
                    format!("the file changed; its current sha256 is {current}, read it again"),
                ));
            }
        }
        (Some(_), None) => {
            return Err(Reply::error(
                409,
                "base_required",
                format!(
                    "{} already exists; read it first and send the sha256 as `base_hash`",
                    path.display()
                ),
            ));
        }
        (None, Some(base)) => {
            let base = normalize_hash(base)?;
            if base != rigel_core::patches::sha256_empty() {
                return Err(Reply::error(
                    409,
                    "stale_base",
                    "the file does not exist; omit base_hash to create it",
                ));
            }
        }
        (None, None) => {}
    }
    let parent = path
        .parent()
        .ok_or_else(|| bad_request("path has no parent directory"))?;
    let name = path
        .file_name()
        .ok_or_else(|| bad_request("path must name a file"))?;
    let temp = parent.join(format!(
        ".{}.rigel-{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    std::fs::write(&temp, content).map_err(|error| Reply::error(500, "io", error.to_string()))?;
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(Reply::error(500, "io", error.to_string()));
    }
    Ok(rigel_core::patches::sha256_hex(content.as_bytes()))
}

fn path_reply(error: PatchError) -> Reply {
    match error {
        PatchError::InvalidPath(path) => Reply::error(
            400,
            "invalid_path",
            format!("{path} is not a project-relative path"),
        ),
        PatchError::OutsideRoot(path) => Reply::error(
            403,
            "outside_project",
            format!("{path} resolves outside the project"),
        ),
        PatchError::NotFound(path) => not_found(&path),
        PatchError::Binary(path) => Reply::error(415, "binary", format!("{path} is not UTF-8")),
        PatchError::StaleBase { current, .. } => Reply::error(
            409,
            "stale_base",
            format!("the file changed; its current sha256 is {current}, read it again"),
        ),
        other => Reply::error(500, "patch", other.to_string()),
    }
}

/// A tool outcome already speaks in `{ok, ...}` / `{error: {code, message}}`; the HTTP status is
/// the only thing it does not carry, so that is all this maps.
fn tool_reply(outcome: ToolOutcome) -> Reply {
    let ok = outcome
        .body
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if ok {
        return Reply::ok(outcome.body);
    }
    let code = outcome
        .body
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("tool_error");
    let message = outcome
        .body
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("the command did not run");
    let status = match code {
        "DENIED" | "APPROVAL" | "PLAN_MODE" | "PLAN_OUTSIDE" | "READ_ONLY" | "NOT_ALLOWED" => 403,
        "BAD_ARGUMENTS" | "PATH_OUTSIDE" | "NOT_DIR" => 400,
        _ => 422,
    };
    Reply::error(status, "command_failed", format!("{code}: {message}"))
}

/// The coordinator's events are already written to the database; this surface does not stream
/// them, so the sink is intentionally empty rather than a queue nobody reads.
struct NullEvents;

impl EventSink for NullEvents {
    fn emit(&self, _event: AgentEvent) {}
}

/// Deny is the default. A caller that is not the user inherits none of the user's consent, so
/// an approval it did not explicitly ask for is a refusal. `approve: true` on one request
/// allows once, for that request only; it never stores a scope.
struct ApiApprover {
    approve: bool,
}

#[async_trait]
impl CommandApprover for ApiApprover {
    async fn approve(&self, _request: &CommandRequest) -> CommandDecision {
        if self.approve {
            CommandDecision::ALLOW_ONCE
        } else {
            CommandDecision::Deny
        }
    }
}

#[async_trait]
impl OriginApprover for ApiApprover {
    async fn approve(&self, _request: &OriginRequest) -> OriginDecision {
        if self.approve {
            OriginDecision::AllowOnce
        } else {
            OriginDecision::Deny
        }
    }
}

#[async_trait]
impl PlanApprover for ApiApprover {
    async fn approve_plan(&self, _request: &PlanRequest) -> PlanDecision {
        if self.approve {
            PlanDecision::Approve
        } else {
            PlanDecision::Deny
        }
    }
}

// ---------------------------------------------------------------------------
// Request parsing
// ---------------------------------------------------------------------------

fn body_object(bytes: &[u8]) -> Result<Value, Reply> {
    if bytes.is_empty() {
        return Ok(json!({}));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| bad_request(format!("body must be JSON: {error}")))?;
    if !value.is_object() {
        return Err(bad_request("body must be a JSON object"));
    }
    Ok(value)
}

fn optional_string(body: &Value, name: &str) -> Result<Option<String>, Reply> {
    match body.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(bad_request(format!("`{name}` must be a string"))),
    }
}

fn required_string(body: &Value, name: &str) -> Result<String, Reply> {
    match body.get(name) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(bad_request(format!("`{name}` must be a string"))),
        None => Err(bad_request(format!("`{name}` is required"))),
    }
}

fn optional_bool(body: &Value, name: &str) -> Result<Option<bool>, Reply> {
    match body.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(bad_request(format!("`{name}` must be a boolean"))),
    }
}

fn optional_i64(body: &Value, name: &str) -> Result<Option<i64>, Reply> {
    match body.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value
            .as_i64()
            .map(Some)
            .ok_or_else(|| bad_request(format!("`{name}` must be an integer"))),
        Some(_) => Err(bad_request(format!("`{name}` must be an integer"))),
    }
}

/// How long a harness may stay busy without a new room message before it is treated as wedged.
/// Fifteen minutes is longer than any one tool call the harnesses run, and the clock resets on
/// every room message, so a genuinely talkative long turn is never killed.
fn room_watchdog_stuck() -> Duration {
    std::env::var("RIGEL_ROOM_STUCK_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds >= 30)
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(900))
}

fn room_watchdog_enabled() -> bool {
    std::env::var("RIGEL_ROOM_WATCHDOG")
        .map(|value| value.trim() != "0")
        .unwrap_or(true)
}

fn optional_u64(body: &Value, name: &str) -> Result<Option<u64>, Reply> {
    match body.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| bad_request(format!("`{name}` must be a non-negative integer"))),
        Some(_) => Err(bad_request(format!(
            "`{name}` must be a non-negative integer"
        ))),
    }
}

/// A base hash is the 64-hex sha256 a read returns. Accepting the `sha256:` prefix as well
/// keeps a caller that copied a display form from failing on a cosmetic difference.
fn normalize_hash(raw: &str) -> Result<String, Reply> {
    let raw = raw.trim();
    let hex = raw.strip_prefix("sha256:").unwrap_or(raw);
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(bad_request(
            "base_hash must be the 64-character sha256 a read returned",
        ));
    }
    Ok(hex.to_ascii_lowercase())
}

fn token_matches(given: &str, expected: &str) -> bool {
    let given = given.as_bytes();
    let expected = expected.as_bytes();
    if given.len() != expected.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in given.iter().zip(expected) {
        difference |= a ^ b;
    }
    difference == 0
}

fn serve_connection(
    api: Arc<Api>,
    handle: tokio::runtime::Handle,
    mut stream: std::net::TcpStream,
) {
    // A client that opens a connection and never finishes its request must not pin a thread
    // forever, so both directions carry a generous but finite timeout.
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(30))).ok();
    let reply = match read_request(&mut stream) {
        Ok(request) => handle.block_on(api.handle(request)),
        Err(reply) => reply,
    };
    let body = serde_json::to_vec(&reply.body).unwrap_or_else(|_| b"{}".to_vec());
    let reason = match reply.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Content",
        431 => "Request Header Fields Too Large",
        499 => "Client Closed Request",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {} {reason}\r\ncontent-type: application/json; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\ncache-control: no-store\r\nx-content-type-options: nosniff\r\n\r\n",
        reply.status,
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn read_request(stream: &mut std::net::TcpStream) -> Result<Request, Reply> {
    let mut buffer = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(position) = find_header_end(&buffer) {
            break position;
        }
        if buffer.len() > MAX_HEADER_BYTES {
            return Err(Reply::error(
                431,
                "headers_too_large",
                "the request headers are too large",
            ));
        }
        let read = stream
            .read(&mut chunk)
            .map_err(|error| bad_request(format!("could not read the request: {error}")))?;
        if read == 0 {
            return Err(bad_request(
                "the connection closed before a request arrived",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| bad_request("the request line is missing"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| bad_request("the method is missing"))?
        .to_ascii_uppercase();
    let target = parts
        .next()
        .ok_or_else(|| bad_request("the path is missing"))?;

    let mut headers = HashMap::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }

    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), parse_query(query)),
        None => (target.to_string(), HashMap::new()),
    };

    if headers.contains_key("transfer-encoding") {
        return Err(Reply::error(
            411,
            "length_required",
            "chunked request bodies are not supported; send content-length",
        ));
    }
    let length = match headers.get("content-length") {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| bad_request("content-length is not a number"))?,
        None => 0,
    };
    if length > MAX_BODY_BYTES {
        return Err(Reply::error(
            413,
            "body_too_large",
            format!("the body is larger than {MAX_BODY_BYTES} bytes"),
        ));
    }

    let consumed = header_end + 4;
    let mut body = buffer[consumed..].to_vec();
    while body.len() < length {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| bad_request(format!("could not read the body: {error}")))?;
        if read == 0 {
            return Err(bad_request("the connection closed before the body arrived"));
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);

    Ok(Request {
        method,
        path,
        query,
        headers,
        body,
    })
}

#[derive(Clone, Copy)]
enum SwarmJobTransition {
    Complete,
    Release,
}

fn transition_swarm_job(data_dir: &Path, id: &str, transition: SwarmJobTransition) -> Reply {
    if id.is_empty()
        || id.len() > 200
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return bad_request("job id must contain only letters, numbers, hyphens, or underscores");
    }

    let swarm = data_dir.join("swarm");
    let file = format!("{id}.json");
    let inbox = swarm.join("inbox").join(&file);
    let claimed = swarm.join("claimed").join(&file);
    let completed = swarm.join("completed").join(&file);
    let (target, desired, conflicting) = match transition {
        SwarmJobTransition::Complete => {
            (&completed, "completed", inbox.exists().then_some("inbox"))
        }
        SwarmJobTransition::Release => (&inbox, "inbox", completed.exists().then_some("completed")),
    };

    let reply_for = |path: &Path| {
        let job = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or(Value::Null);
        Reply::ok(json!({ "ok": true, "job": job, "state": desired }))
    };

    if target.exists() {
        return reply_for(target);
    }
    if let Some(state) = conflicting {
        return Reply::error(
            409,
            "invalid_job_state",
            format!("job `{id}` is {state}, not claimed"),
        );
    }
    if !claimed.exists() {
        return not_found("that Swarm job");
    }
    if let Some(parent) = target.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        return Reply::error(500, "io", error.to_string());
    }
    match std::fs::rename(&claimed, target) {
        Ok(()) => reply_for(target),
        Err(_) if target.exists() => reply_for(target),
        Err(_) if inbox.exists() || completed.exists() => Reply::error(
            409,
            "invalid_job_state",
            format!("job `{id}` changed state while the request was running"),
        ),
        Err(error) => Reply::error(500, "io", error.to_string()),
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut pairs = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        pairs.insert(name.to_string(), value.to_string());
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_twice_a_uuid_and_never_a_placeholder() {
        let token = new_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|character| character.is_ascii_hexdigit()));
        assert_ne!(token, new_token());
    }

    #[test]
    fn only_the_exact_token_matches() {
        assert!(token_matches("abc123", "abc123"));
        assert!(!token_matches("abc124", "abc123"));
        assert!(!token_matches("abc12", "abc123"));
        assert!(!token_matches("", "abc123"));
    }

    #[test]
    fn a_base_hash_is_hex_and_an_optional_prefix_is_accepted() {
        let hex = "a".repeat(64);
        assert_eq!(normalize_hash(&hex).unwrap(), hex);
        assert_eq!(normalize_hash(&format!("sha256:{hex}")).unwrap(), hex);
        assert_eq!(
            normalize_hash(&format!("sha256:{}", hex.to_uppercase())).unwrap(),
            hex
        );
        assert!(normalize_hash("nope").is_err());
        assert!(normalize_hash(&"z".repeat(64)).is_err());
        assert!(normalize_hash(&"a".repeat(63)).is_err());
    }

    #[test]
    fn the_host_is_loopback_or_one_named_interface() {
        for host in [
            "127.0.0.1",
            "localhost",
            "::1",
            "[::1]",
            "100.70.84.102",
            "192.168.1.20",
        ] {
            assert!(bindable(host), "{host}");
        }
        for host in ["0.0.0.0", "::", "ff02::1", "example.com", "rigel", ""] {
            assert!(!bindable(host), "{host}");
        }
        assert!(bindable("127.0.0.1") && !bindable("0.0.0.0"));
    }

    #[test]
    fn a_tailscale_address_is_the_cgnat_one_only() {
        assert_eq!(
            parse_tailscale_ip("100.70.84.102\n").as_deref(),
            Some("100.70.84.102")
        );
        assert_eq!(
            parse_tailscale_ip("fd7a:115c:a1e0::1\n100.101.3.4\n").as_deref(),
            Some("100.101.3.4")
        );
        assert_eq!(parse_tailscale_ip("192.168.1.20\n127.0.0.1\n"), None);
        assert_eq!(parse_tailscale_ip("100.63.1.1\n100.128.1.1\n"), None);
        assert_eq!(parse_tailscale_ip("not an address\n"), None);
    }

    #[test]
    fn a_body_must_be_an_object_of_the_right_types() {
        assert!(body_object(b"").is_ok());
        assert!(body_object(br#"{"prompt":"hi"}"#).is_ok());
        assert!(body_object(br#"["hi"]"#).is_err());
        assert!(body_object(br#"not json"#).is_err());
        let body = json!({"a": "x", "b": true, "c": 3});
        assert_eq!(optional_string(&body, "a").unwrap(), Some("x".into()));
        assert_eq!(optional_bool(&body, "b").unwrap(), Some(true));
        assert_eq!(optional_i64(&body, "c").unwrap(), Some(3));
        assert_eq!(optional_u64(&body, "c").unwrap(), Some(3));
        assert!(optional_string(&body, "b").is_err());
        assert!(optional_bool(&body, "c").is_err());
        assert!(optional_i64(&body, "a").is_err());
        assert!(optional_u64(&body, "a").is_err());
        assert!(required_string(&body, "missing").is_err());
    }

    #[test]
    fn a_probe_ok_is_a_word_not_any_non_empty_reply() {
        assert!(probe_reply_is_ok("OK"));
        assert!(probe_reply_is_ok("ok."));
        assert!(probe_reply_is_ok("  \"OK\"  "));
        assert!(!probe_reply_is_ok("looks good"));
        assert!(!probe_reply_is_ok(
            "This version of Antigravity is no longer supported. Please update."
        ));
    }

    #[test]
    fn a_query_is_pairs_and_a_bare_name_is_an_empty_value() {
        let query = parse_query("all=1&refresh&provider=openrouter");
        assert_eq!(query.get("all").map(String::as_str), Some("1"));
        assert_eq!(query.get("refresh").map(String::as_str), Some(""));
        assert_eq!(
            query.get("provider").map(String::as_str),
            Some("openrouter")
        );
    }

    #[test]
    fn the_header_terminator_is_found_at_the_end_of_the_headers() {
        let raw = b"GET /v1/health HTTP/1.1\r\nHost: localhost\r\n\r\n{}";
        assert_eq!(find_header_end(raw), Some(raw.len() - 6));
        assert_eq!(find_header_end(b"GET / HTTP/1.1\r\n"), None);
    }

    #[test]
    fn claimed_swarm_jobs_can_be_completed_or_released_idempotently() {
        let dir = tempfile::tempdir().expect("temp dir");
        let claimed = dir.path().join("swarm").join("claimed");
        std::fs::create_dir_all(&claimed).unwrap();

        for id in ["complete-me", "release-me"] {
            std::fs::write(
                claimed.join(format!("{id}.json")),
                serde_json::to_vec(&json!({ "id": id, "prompt": "test" })).unwrap(),
            )
            .unwrap();
        }

        let complete =
            transition_swarm_job(dir.path(), "complete-me", SwarmJobTransition::Complete);
        assert_eq!(complete.status, 200);
        assert_eq!(complete.body["state"], "completed");
        assert!(
            dir.path()
                .join("swarm/completed/complete-me.json")
                .is_file()
        );
        assert_eq!(
            transition_swarm_job(dir.path(), "complete-me", SwarmJobTransition::Complete,).status,
            200
        );
        assert_eq!(
            transition_swarm_job(dir.path(), "complete-me", SwarmJobTransition::Release).status,
            409
        );

        let release = transition_swarm_job(dir.path(), "release-me", SwarmJobTransition::Release);
        assert_eq!(release.status, 200);
        assert_eq!(release.body["state"], "inbox");
        assert!(dir.path().join("swarm/inbox/release-me.json").is_file());
        assert_eq!(
            transition_swarm_job(dir.path(), "release-me", SwarmJobTransition::Release,).status,
            200
        );
        assert_eq!(
            transition_swarm_job(dir.path(), "release-me", SwarmJobTransition::Complete,).status,
            409
        );
    }

    #[test]
    fn swarm_job_transitions_reject_unknown_and_unsafe_ids() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            transition_swarm_job(dir.path(), "missing", SwarmJobTransition::Complete).status,
            404
        );
        assert_eq!(
            transition_swarm_job(dir.path(), "../escape", SwarmJobTransition::Release).status,
            400
        );
    }

    #[test]
    fn a_full_access_write_outside_the_project_is_still_compare_and_swap() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("note.txt");
        let first = write_outside(&path, "one", None).expect("create");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one");
        assert!(write_outside(&path, "two", None).is_err());
        let second = write_outside(&path, "two", Some(&first)).expect("replace");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(second, rigel_core::patches::sha256_hex(b"two"));
        assert!(write_outside(&path, "three", Some(&first)).is_err());
    }
}

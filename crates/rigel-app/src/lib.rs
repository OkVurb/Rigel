//! The Tauri shell: a thin IPC layer over `rigel-core`.
//!
//! Commands are deliberately stateless — each one opens the project database it needs. That
//! costs a connection open, and buys the property that no renderer crash can desynchronise
//! app state (handoff §3.2, S2).
//!
//! A native turn is the one long-lived thing: `turn_send` creates the turn row, returns it,
//! and spawns the coordinator. Its events go to the window on `rigel://events`, and its
//! cancellation token lives in managed state so `turn_cancel` can reach it.
//!
//! Two constraints shape the command signatures:
//! * they are private, because `#[tauri::command]` exports a macro for `pub` items and that
//!   collides with the crate-root macro namespace;
//! * they are synchronous and drive the async core with `block_on`, because Tauri's async
//!   command wrapper cannot express sqlx's higher-ranked `Executor` lifetimes.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use rigel_core::agent::approval::{
    ApprovalBus, MailApprover, MailDecision, MailRequest, PendingApproval,
};
pub use rigel_core::agent::catalog::ModelOption;
use rigel_core::agent::routing::RouteExplanation;
use rigel_core::agent::{Cancels, Coordinator, DEFAULT_MAX_STEPS, fixed_context_chars};
use rigel_core::agent::{capabilities, catalog, custom, health, openrouter, providers, vertex};
use rigel_core::{
    Agent, AgentEvent, AppliedReview, ApprovalScope, Attention, AttributedHunk, Bookmark,
    BudgetService, ClaimConflict, ClaimService, CommandApprover, CommandDecision, CommandRequest,
    ConflictWatcher, EventSink, HunkComment, HunkComments, InterruptSink, MemoryApprover,
    MemoryDecision, MemoryFile, MemoryRequest, MemoryService, MergeReview, MergeReviews, Model,
    NetworkPolicy, POLL_INTERVAL, Patch, PatchPipeline, PlanApprover, PlanDecision, PlanRequest,
    Provenance, RecoveryOffer, ResumeOutcome, Scopes, ScriptedModel, Session, SessionService,
    SessionTemplate, SessionTemplates, SkillDetail, SkillIndex, SkillsError, SnapshotService,
    StopAt, Store, SubagentPolicy, SyncPolicy, TeamAgent, Tools, Turn, TurnQuote, TurnTranscript,
    UnverifiedChange, Usage, WorktreeService, WriteMode, apply_review, credentials, diagnostics,
    skills::search::SearchHit,
};
use serde::{Deserialize, Serialize};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, State};

mod claude;
mod concord;
mod control;
mod mcp_catalog;
mod relay;
mod terminal;
mod tray_status;
pub mod webview;
pub use claude::mcp_mode;

#[derive(Debug, Clone, Serialize)]
pub struct ProjectInfo {
    pub root: String,
    pub name: String,
    pub db: String,
}

fn default_model() -> String {
    rigel_core::agent::omniroute::DEFAULT_MODEL.to_string()
}

/// Whether a saved model id still exists on the door that would receive it.
///
/// The user-visible failure this prevents is a 400 like "model X is not available in the active
/// live catalog": the id was saved when the roster listed it, and the roster moved. When the
/// provider publishes a live roster the id is checked against it; a miss re-reads that door once
/// (a cache goes stale the same way a saved id does) and then refuses with real rows instead of
/// sending a request that cannot succeed.
fn guard_live_model(provider: &str, model: &str) -> Result<(), String> {
    let model = model.trim();
    let Some(spec) = providers::find(provider) else {
        return Ok(());
    };
    // A gateway routes any id by definition, and a door with no roster has nothing to check.
    if spec.gateway || !spec.lists_models || model.is_empty() {
        return Ok(());
    }
    for force in [false, true] {
        let Ok(rows) = models_cached(force) else {
            return Ok(());
        };
        let listed: Vec<&str> = rows
            .iter()
            .filter(|row| row.provider == spec.id)
            .map(|row| row.id.as_str())
            .collect();
        if listed.is_empty() || listed.contains(&model) {
            return Ok(());
        }
        if !force {
            continue;
        }
        let sample = listed
            .iter()
            .take(4)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "`{model}` is not in {}'s live catalogue, so no request was sent; it lists {sample} — \
             pick a row in the picker, or press free mode",
            spec.name
        ));
    }
    Ok(())
}

/// Free mode reaches the native fan-out too: when the composer is on the free model Free mode
/// proved, children run on that same door and the coordinator allows the id, so a swarm does not
/// quietly spend on a paid nano default behind a free parent.
fn subagent_policy(settings: &Settings) -> SubagentPolicy {
    let model = settings.model.trim();
    if settings.free_mode && !model.is_empty() {
        return SubagentPolicy {
            model: model.to_string(),
            extra_models: vec![model.to_string()],
            ..SubagentPolicy::default()
        };
    }
    SubagentPolicy::default()
}

/// The Free-mode agent the Swarm room can launch. `None` when Free mode is off, no model is
/// saved, or the provider's API is not OpenAI-compatible (Anthropic, Vertex, a custom door).
fn free_swarm_agent(settings: &Settings) -> Option<rigel_core::agent::agentchat::FreeAgent> {
    if !settings.free_mode {
        return None;
    }
    let model = settings.model.trim();
    if model.is_empty() {
        return None;
    }
    let provider = settings.provider.trim().to_lowercase();
    let door = tauri::async_runtime::block_on(rigel_core::agent::providers::compatible_door(
        &provider,
        &credentials::KeychainCredentialStore,
    ))
    .ok()
    .flatten()?;
    Some(rigel_core::agent::agentchat::FreeAgent {
        base_url: door.base_url,
        model: model.to_string(),
        api_key: door.api_key,
    })
}

fn default_provider() -> String {
    "omniroute".to_string()
}

fn default_ui_scale() -> u16 {
    100
}

/// The separate reasoning-effort setting (D28). `medium` matches the old default model.
fn default_effort() -> String {
    "medium".to_string()
}

/// D90 defaults: notifications are on, but a platform that refuses permission stays quiet.
fn default_true() -> bool {
    true
}

fn valid_effort(effort: &str) -> bool {
    matches!(effort, "low" | "medium" | "high")
}

fn default_terminal_font_size() -> u16 {
    13
}

fn default_terminal_scrollback() -> u32 {
    8_000
}

/// The coordinator's step bound, as a setting: 10 by default (D19's loop).
fn default_max_steps() -> u16 {
    DEFAULT_MAX_STEPS as u16
}

/// Who leads a Swarm: Muse splits the prompt and assigns it, or the room gets one broadcast.
fn default_swarm_lead() -> String {
    "muse".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub theme: String,
    /// The cap a new session starts with, in cents. Zero means no cap; a running session keeps
    /// the cap it was created with.
    pub default_budget_cents: i64,
    /// `omniroute` (local router), `openrouter`, or `gemini`. Old settings files gain the
    /// default.
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    /// Free mode: the saved provider and model are the free pair Free mode proved was answering.
    /// Picking a model by hand clears it, so the composer never claims a free model it is not on.
    #[serde(default)]
    pub free_mode: bool,
    /// The Google Cloud project Vertex is billed to, saved by the Providers page. Empty means
    /// "ask the environment, then the Cloud CLI"; a saved value sits between the two.
    #[serde(default)]
    pub vertex_project: String,
    /// Sent as `reasoning_effort` on every native turn; never folded into the model id.
    #[serde(default = "default_effort")]
    pub effort: String,
    #[serde(default = "default_true")]
    pub notify_approval: bool,
    #[serde(default = "default_true")]
    pub notify_turn_done: bool,
    #[serde(default = "default_true")]
    pub notify_turn_failed: bool,
    /// The Terminal surface: xterm's font size and scrollback, and the program to open —
    /// empty means PowerShell on Windows and `$SHELL` elsewhere.
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: u16,
    #[serde(default = "default_terminal_scrollback")]
    pub terminal_scrollback: u32,
    #[serde(default)]
    pub terminal_shell: String,
    /// Re-sync a skill source whose data is older than the stale window, in the background, so a
    /// shelf nobody has touched for a day updates itself instead of staying stale.
    #[serde(default = "default_true")]
    pub skills_auto_update: bool,
    /// How many model steps one native turn may take: read → edit → verify → answer, with room
    /// to recover from a bad tool call.
    #[serde(default = "default_max_steps")]
    pub max_steps: u16,
    /// Whether the model picker opens on chat-eligible rows only. Saved rather than kept per
    /// popover: a person who works with image or audio models should not re-tick the box on every
    /// launch.
    #[serde(default = "default_true")]
    pub chat_models_only: bool,
    /// Who leads a Swarm: `muse` queues the prompt for Muse to split and assign, `broadcast`
    /// sends the same text to every ticked agent. Old settings files gain `muse`.
    #[serde(default = "default_swarm_lead")]
    pub swarm_lead: String,
    /// Refine each composer draft with the cheap model and send the result without the review
    /// card. Off means Send opens the card and nothing is sent until the user confirms it.
    #[serde(default)]
    pub auto_refine: bool,
    /// Publish a session after each of its turns finishes, so the phone and the web can read it
    /// without a button. Off by default: publishing a transcript is opt-in.
    #[serde(default)]
    pub cloud_sync_sessions: bool,
    /// D159: the window's scale, one of 90/100/110/125. Applied to the shell, not the terminal's
    /// own font, so the PTY keeps its measured cell size.
    #[serde(default = "default_ui_scale")]
    pub ui_scale: u16,
    /// D159: tighter rows in the transcript, list and diff. Layout only; no content is hidden.
    #[serde(default)]
    pub compact_density: bool,
    /// Q290: an explicit high-contrast palette, independent of the OS preference.
    #[serde(default)]
    pub high_contrast: bool,
    /// Q283: the local-time window (`HH:MM`) in which desktop notifications stay silent.
    /// Empty means quiet hours are off.
    #[serde(default)]
    pub quiet_hours_start: String,
    #[serde(default)]
    pub quiet_hours_end: String,
    /// Opt-in Gravity sponsored card on Home. Off by default: an ad request is a network call,
    /// and the user decides whether the paper carries one.
    #[serde(default)]
    pub gravity_ads: bool,
    /// Request test ads: no billing, no metrics, and a sample card even when no ad matches. The
    /// switch stays on until the publisher account is approved for production.
    #[serde(default = "default_true")]
    pub gravity_test_ads: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            default_budget_cents: 500,
            provider: default_provider(),
            model: default_model(),
            free_mode: false,
            vertex_project: String::new(),
            effort: default_effort(),
            notify_approval: true,
            notify_turn_done: true,
            notify_turn_failed: true,
            terminal_font_size: default_terminal_font_size(),
            terminal_scrollback: default_terminal_scrollback(),
            terminal_shell: String::new(),
            skills_auto_update: true,
            max_steps: default_max_steps(),
            chat_models_only: true,
            swarm_lead: default_swarm_lead(),
            auto_refine: false,
            cloud_sync_sessions: false,
            ui_scale: default_ui_scale(),
            compact_density: false,
            high_contrast: false,
            quiet_hours_start: String::new(),
            quiet_hours_end: String::new(),
            gravity_ads: false,
            gravity_test_ads: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyStatus {
    /// `true` per credential name — one entry per provider and one per harness key. A map rather
    /// than a field per vendor, so the adapter table, `rigel keys status` and this screen cannot
    /// drift apart. The renderer asks *whether* a key exists and never receives one.
    pub providers: BTreeMap<String, bool>,
    /// Whether any **chat** provider has a credential. The launch banner and the composer's hint
    /// both ask that question, and the adapter table is the only thing that knows the answer —
    /// a key that belongs to a harness alone does not make a native turn possible.
    pub native_ready: bool,
}

/// Q244: a successful save also says whether the key was checked, so Settings can report a
/// covered path instead of claiming every provider was tested.
#[derive(Debug, Clone, Serialize)]
pub struct KeySetResult {
    pub status: KeyStatus,
    /// `None` for a harness credential, which has no provider roster here; the harness checks it
    /// when it starts. A chat provider always carries a check, possibly `checked: false` when it
    /// publishes no model list.
    pub check: Option<catalog::KeyValidation>,
}

fn current_key_status() -> KeyStatus {
    let store = credentials::KeychainCredentialStore;
    KeyStatus {
        providers: credentials::PROVIDERS
            .iter()
            .map(|provider| (provider.to_string(), credentials::present(provider)))
            .collect(),
        native_ready: providers::all()
            .iter()
            .any(|spec| credentials::present_in(&store, spec.id)),
    }
}

/// One row per adapter, for Settings. Names, hints and whether a credential exists — never a
/// key, and never the value of one.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderRow {
    pub id: String,
    pub name: String,
    pub hint: String,
    /// The variable a headless run reads the key from.
    pub env: Option<String>,
    /// The credential is a short-lived token rather than a key, and the copy says so.
    pub access_token: bool,
    pub default_model: Option<String>,
    pub key_url: Option<String>,
    pub key_present: bool,
}

/// Item 15: the launch health pass. The picker reads the same cached rows instead of probing
/// every door a second time.
#[tauri::command]
fn provider_health() -> Result<Vec<health::DoorHealth>, String> {
    tauri::async_runtime::block_on(async {
        let store = credentials::KeychainCredentialStore;
        Ok(health::launch(&store).await)
    })
}

/// Roadmap 26: the effective policy for a project, read before a turn and shown by the route
/// inspector. The file is the policy; this command does not cache a second copy.
#[tauri::command]
fn routing_policy(project: String) -> Result<openrouter::RoutingPolicy, String> {
    Ok(openrouter::RoutingPolicy::load(std::path::Path::new(
        &project,
    )))
}

/// Roadmap 28: the saved custom door, with header values masked.
#[tauri::command]
fn custom_provider_get() -> Result<Option<custom::MaskedDoor>, String> {
    Ok(custom::load()?.map(|door| door.masked()))
}

/// Roadmap 28: ask a candidate door for its models without saving anything.
#[tauri::command]
fn custom_provider_probe(door: custom::CustomDoor) -> Result<Vec<String>, String> {
    tauri::async_runtime::block_on(custom::probe(&door.base_url, &door.headers))
}

/// Roadmap 28: validate the probe result and write the whole record to the keychain. No header
/// value is ever written to settings or returned to the renderer.
#[tauri::command]
fn custom_provider_save(door: custom::CustomDoor) -> Result<custom::MaskedDoor, String> {
    credentials::set(custom::ID, &door.encode()?).map_err(message)?;
    Ok(door.masked())
}

/// Item 18: capabilities for the rows the picker is already holding.
#[tauri::command]
fn model_capabilities(
    models: Vec<capabilities::ModelRef>,
) -> Result<Vec<capabilities::CapabilityRow>, String> {
    Ok(capabilities::for_models(models))
}

#[tauri::command]
fn providers_list() -> Result<Vec<ProviderRow>, String> {
    let store = credentials::KeychainCredentialStore;
    Ok(providers::all()
        .iter()
        .map(|spec| ProviderRow {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            hint: spec.hint.to_string(),
            env: credentials::provider_environment_variable(spec.id).map(str::to_string),
            access_token: spec.access_token,
            default_model: spec.default_model.map(str::to_string),
            key_url: spec.key_url.map(str::to_string),
            key_present: providers::present(&store, spec),
        })
        .collect())
}

/// The route decision both the inspector and the turn path read. One function means the two
/// surfaces cannot drift: the turn uses `resolved`, the inspector prints the same struct.
fn explain_route(
    settings: &Settings,
    requested_provider: Option<&str>,
    requested_model: Option<&str>,
) -> Result<RouteExplanation, String> {
    rigel_core::agent::routing::explain(
        &settings.provider,
        &settings.model,
        requested_provider,
        requested_model,
    )
}

#[tauri::command]
fn route_explain(
    provider: Option<String>,
    model: Option<String>,
) -> Result<RouteExplanation, String> {
    explain_route(&load_settings(), provider.as_deref(), model.as_deref())
}

/// The window-driver bootstrap: which project to open before the first human click. Shipped
/// builds return `None`; only the E2E runner sets `RIGEL_E2E_PROJECT`.
#[derive(Debug, Clone, Serialize)]
pub struct E2eConfig {
    pub project: String,
}

#[tauri::command]
fn e2e_config() -> Option<E2eConfig> {
    std::env::var("RIGEL_E2E_PROJECT")
        .ok()
        .map(|project| project.trim().to_string())
        .filter(|project| !project.is_empty())
        .map(|project| E2eConfig { project })
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnReply {
    pub turn: Turn,
    pub budget_cents: Option<i64>,
    /// The exact explanation the inspector reads for the same inputs.
    pub route: RouteExplanation,
}

struct AppRuntime {
    cancels: Arc<Cancels>,
}

/// Forwards normalised events to the one channel the renderer subscribes to.
struct WindowSink {
    app: tauri::AppHandle,
}

impl EventSink for WindowSink {
    fn emit(&self, event: AgentEvent) {
        let _ = self.app.emit("rigel://events", event);
    }
}

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn settings_path() -> PathBuf {
    rigel_core::default_index_dir().join("settings.toml")
}

/// Where the global index lives. `RIGEL_INDEX_DIR` exists so the window can be pointed at a
/// test index, the same way the CLI's `--data-dir` works.
fn index_dir() -> PathBuf {
    std::env::var_os("RIGEL_INDEX_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(rigel_core::default_index_dir)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// The two stores that cover every harness Rigel currently launches: the shared `.agents`
/// directory for Codex/Cursor/Cline and Claude's own directory for Claude Code.
fn skill_roots() -> Result<Vec<PathBuf>, String> {
    let home = home_dir().ok_or("no USERPROFILE/HOME to install a skill into")?;
    Ok(vec![
        home.join(".agents").join("skills"),
        home.join(".claude").join("skills"),
    ])
}

fn load_settings() -> Settings {
    let path = settings_path();
    if !path.exists() {
        return Settings::default();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

/* --------------------------------- shelf --------------------------------- */

/// Shelf search. Reads only the local index, so it works offline (§9, Shelf: offline state).
/// `offset` pages through the whole catalogue; a page is capped at 500 rows in the core.
#[tauri::command]
fn skills_search(
    query: String,
    limit: usize,
    offset: Option<usize>,
) -> Result<Vec<SearchHit>, String> {
    tauri::async_runtime::block_on(async move {
        SkillIndex::open(&index_dir())
            .await
            .map_err(message)?
            .search_page(
                &query,
                limit,
                offset.unwrap_or(0),
                rigel_core::skills::search::SearchOrder::DEFAULT,
            )
            .await
            .map_err(message)
    })
}

/// How many rows the current query matches (the whole shelf when the query is empty).
/// The list header and the status bar show this instead of the size of one page.
#[tauri::command]
fn skills_count(query: String) -> Result<i64, String> {
    tauri::async_runtime::block_on(async move {
        SkillIndex::open(&index_dir())
            .await
            .map_err(message)?
            .count(&query)
            .await
            .map_err(message)
    })
}

/// Everything the detail pane shows: metadata, the static review, evidence and variants.
#[tauri::command]
fn skills_load(id: String) -> Result<SkillDetail, String> {
    tauri::async_runtime::block_on(async move {
        match SkillIndex::open(&index_dir())
            .await
            .map_err(message)?
            .get(&id)
            .await
            .map_err(message)?
        {
            Some(detail) => Ok(detail),
            None => Err(message(SkillsError::NotIndexed(id))),
        }
    })
}

/// Source health for the status bar. Never makes a network call.
#[tauri::command]
fn skills_status() -> Result<Vec<rigel_core::SourceStatus>, String> {
    tauri::async_runtime::block_on(async move {
        SkillIndex::open(&index_dir())
            .await
            .map_err(message)?
            .status()
            .await
            .map_err(message)
    })
}

/// Walk a repository from the shelf's empty state. The only skill command that uses the
/// network, and the only one that can take seconds.
#[tauri::command]
fn skills_index(repo: String, force: bool) -> Result<rigel_core::IngestReport, String> {
    tauri::async_runtime::block_on(async move {
        let index = SkillIndex::open(&index_dir()).await.map_err(message)?;
        let policy = if force {
            SyncPolicy::forced()
        } else {
            SyncPolicy::normal()
        };
        index.index_repo(&repo, policy).await.map_err(message)
    })
}

/// Re-sync every skill source that is not fresh (`SkillIndex::refresh_stale`). The Shelf page's
/// Refresh button and the launch-time auto-update both call this; it makes network calls, so it
/// is never on the path of a search.
#[tauri::command]
fn skills_refresh() -> Result<rigel_core::skills::RefreshReport, String> {
    tauri::async_runtime::block_on(async move {
        SkillIndex::open(&index_dir())
            .await
            .map_err(message)?
            .refresh_stale()
            .await
            .map_err(message)
    })
}

/// Copy the exact indexed body into the harness stores. Nothing is fetched here.
#[tauri::command]
fn skills_install(id: String) -> Result<rigel_core::skills::InstallReport, String> {
    let roots = skill_roots()?;
    tauri::async_runtime::block_on(async move {
        SkillIndex::open(&index_dir())
            .await
            .map_err(message)?
            .install(&id, &roots)
            .await
            .map_err(message)
    })
}

/// Whether the selected row is already in either harness store.
#[tauri::command]
fn skills_installed(id: String) -> Result<bool, String> {
    let roots = skill_roots()?;
    tauri::async_runtime::block_on(async move {
        SkillIndex::open(&index_dir())
            .await
            .map_err(message)?
            .installed(&id, &roots)
            .await
            .map_err(message)
    })
}

/* ---------------------------------- MCPs ---------------------------------- */

#[tauri::command]
fn mcp_catalog_search(
    query: String,
    limit: usize,
    offset: Option<usize>,
) -> Result<Vec<mcp_catalog::McpServer>, String> {
    mcp_catalog::search(&query, limit, offset.unwrap_or(0))
}

#[tauri::command]
fn mcp_catalog_count(query: String) -> Result<i64, String> {
    mcp_catalog::count(&query)
}

#[tauri::command]
fn mcp_install(project: String, id: String) -> Result<mcp_catalog::InstallReport, String> {
    mcp_catalog::install(&project, &id)
}

#[tauri::command]
fn mcp_installed(project: String, id: String) -> Result<bool, String> {
    mcp_catalog::installed(&project, &id)
}

/* -------------------------------- project -------------------------------- */

/// Open (creating if needed) a project and report where its session database lives.
#[tauri::command]
fn project_open(path: String) -> Result<ProjectInfo, String> {
    tauri::async_runtime::block_on(async move {
        let root = PathBuf::from(&path);
        let store = Store::open_project(&root).await.map_err(message)?;

        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| root.to_string_lossy().into_owned());

        remember_project(&root);
        Ok(ProjectInfo {
            root: root.to_string_lossy().into_owned(),
            name,
            db: store.path().to_string_lossy().into_owned(),
        })
    })
}

/// Where "New workspace" creates folders: `%USERPROFILE%\Rigel\<name>` (or `~/Rigel/<name>`).
fn workspace_root() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Rigel")
}

/// Create an empty folder and open it as a project. The name is a single path segment: no
/// separators, no `.`/`..`, nothing that could escape the workspace root.
#[tauri::command]
fn project_create(name: String) -> Result<ProjectInfo, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the workspace a name.".to_string());
    }
    if name.len() > 64
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        || name.chars().any(|c| c.is_control())
    {
        return Err("Use a short name with no path separators or reserved characters.".to_string());
    }
    let root = workspace_root().join(name);
    if root.exists() && !root.is_dir() {
        return Err(format!("{} exists and is not a folder.", root.display()));
    }
    fs::create_dir_all(&root).map_err(message)?;
    project_open(root.to_string_lossy().into_owned())
}

/// The default directory new workspaces are created under, so the UI can show it before
/// asking for a name (never a silent location).
#[tauri::command]
fn project_workspace_root() -> String {
    workspace_root().to_string_lossy().into_owned()
}

/* ------------------------------- attachments ------------------------------ */

/// Copy files chosen in the native picker into `.rigel/attachments` and return project-relative
/// paths. `.rigel` is outside the checkpoint tree, so an attachment never makes the session look
/// dirty; the paths are what the composer adds to the next turn.
#[tauri::command]
fn attachment_import(project: String, paths: Vec<String>) -> Result<Vec<String>, String> {
    let root = PathBuf::from(&project);
    if !root.is_dir() {
        return Err("Open a project folder before attaching files.".to_string());
    }
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let dir = root.join(".rigel").join("attachments");
    fs::create_dir_all(&dir).map_err(message)?;
    // Attachments are local context, not source: keep the directory out of the project's diff
    // even when the project does not ignore `.rigel` itself.
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        fs::write(
            &ignore,
            "# Written by Rigel. Files attached from the composer are local to this machine.\n*\n",
        )
        .map_err(message)?;
    }

    let mut imported = Vec::with_capacity(paths.len());
    for source in paths {
        let source = PathBuf::from(source);
        if !source.is_file() {
            return Err(format!("{} is not a file.", source.display()));
        }
        let name = source
            .file_name()
            .ok_or_else(|| "the picked file has no name".to_string())?;
        let target = free_attachment_path(&dir, name)?;
        fs::copy(&source, &target).map_err(message)?;
        let relative = target.strip_prefix(&root).map_err(message)?;
        imported.push(relative.to_string_lossy().replace('\\', "/"));
    }
    Ok(imported)
}

/// Keep the original filename unless that name is already taken, then add the smallest suffix
/// that is free. Two files with the same name in one pick coexist instead of one overwriting the
/// other.
fn free_attachment_path(dir: &Path, name: &std::ffi::OsStr) -> Result<PathBuf, String> {
    let mut target = dir.join(name);
    if !target.exists() {
        return Ok(target);
    }

    let original = Path::new(name);
    let stem = original
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "attachment".to_string());
    let extension = original
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    for suffix in 1..=9_999 {
        target = dir.join(format!("{stem}-{suffix}{extension}"));
        if !target.exists() {
            return Ok(target);
        }
    }
    Err(format!(
        "could not find a free name for {}",
        original.display()
    ))
}

#[tauri::command]
fn session_start(
    project: String,
    title: Option<String>,
    budget_cents: Option<i64>,
) -> Result<Session, String> {
    tauri::async_runtime::block_on(async move {
        // An explicit cap still wins; otherwise the saved default is the one place that choice
        // lives, and zero means no cap.
        let budget_cents = budget_cents.or_else(|| {
            let saved = load_settings().default_budget_cents;
            (saved > 0).then_some(saved)
        });
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .start(title.as_deref(), budget_cents)
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn session_list(project: String) -> Result<Vec<Session>, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .list()
            .await
            .map_err(message)
    })
}

/// D148: the session list's three edits. Each returns the row as stored, so the window renders
/// the database's answer instead of its own optimism.
#[tauri::command]
fn session_rename(project: String, id: String, title: Option<String>) -> Result<Session, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .rename(&id, title.as_deref())
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn session_pin(project: String, id: String, pinned: bool) -> Result<Session, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .set_pinned(&id, pinned)
            .await
            .map_err(message)
    })
}

/// Q270: the window's skill pins. The body hash is resolved from the local index unless a
/// template supplied one, so a template can restore the exact body its session read.
#[tauri::command]
fn session_skill_pins(
    project: String,
    session: String,
) -> Result<Vec<rigel_core::SkillPinView>, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let index = SkillIndex::open(&index_dir()).await.map_err(message)?;
        rigel_core::pins::list(&service, &index, &session)
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn session_skill_pin(
    project: String,
    session: String,
    skill_id: String,
    body_sha: Option<String>,
) -> Result<rigel_core::SkillPinView, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        service.get(&session).await.map_err(message)?;
        let index = SkillIndex::open(&index_dir()).await.map_err(message)?;
        let sha = match body_sha.filter(|sha| !sha.trim().is_empty()) {
            Some(sha) => sha,
            None => index
                .body(&skill_id)
                .await
                .map_err(message)?
                .map(|(_, sha)| sha)
                .ok_or_else(|| format!("{skill_id} has no indexed body to pin"))?,
        };
        service
            .pin_skill(&session, &skill_id, &sha, false)
            .await
            .map_err(message)?;
        rigel_core::pins::one(&service, &index, &session, &skill_id)
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn session_skill_unpin(project: String, session: String, skill_id: String) -> Result<bool, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .unpin_skill(&session, &skill_id)
            .await
            .map_err(message)
    })
}

/// D141: the full changed-pin review. Bodies stay in the index's blob store; this read only
/// returns them for the open card, never writes them into the project database.
#[tauri::command]
fn session_skill_review(
    project: String,
    session: String,
    skill_id: String,
) -> Result<rigel_core::SkillPinReview, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let index = SkillIndex::open(&index_dir()).await.map_err(message)?;
        rigel_core::pins::review(&service, &index, &session, &skill_id)
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn session_skill_keep(
    project: String,
    session: String,
    skill_id: String,
    reason: String,
) -> Result<rigel_core::SkillPinView, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let index = SkillIndex::open(&index_dir()).await.map_err(message)?;
        rigel_core::pins::keep(&service, &index, &session, &skill_id, &reason)
            .await
            .map_err(message)
    })
}

/// D141: one reason for keeping every changed pin. The pinned hashes stay put; the core records
/// a single `skill_pin` intervention with each pin's pinned and current hash.
#[tauri::command]
fn session_skill_keep_all(
    project: String,
    session: String,
    reason: String,
) -> Result<Vec<rigel_core::SkillPinView>, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let index = SkillIndex::open(&index_dir()).await.map_err(message)?;
        rigel_core::pins::keep_all(&service, &index, &session, &reason)
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn session_skill_update(
    project: String,
    session: String,
    skill_id: String,
    reason: String,
) -> Result<rigel_core::SkillPinView, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let index = SkillIndex::open(&index_dir()).await.map_err(message)?;
        rigel_core::pins::update(&service, &index, &session, &skill_id, &reason)
            .await
            .map_err(message)
    })
}

/// D141: one reason for every changed pin. The core resolves each current hash and records a
/// single `skill_pin` intervention with all old/new pairs.
#[tauri::command]
fn session_skill_update_all(
    project: String,
    session: String,
    reason: String,
) -> Result<Vec<rigel_core::SkillPinView>, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let index = SkillIndex::open(&index_dir()).await.map_err(message)?;
        rigel_core::pins::update_all(&service, &index, &session, &reason)
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn session_archive(project: String, id: String, archived: bool) -> Result<Session, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .set_archived(&id, archived)
            .await
            .map_err(message)
    })
}

/// Q274: the window's only destructive session action. The core refuses a session that has not
/// been archived; the renderer asks for the id again before it gets this far.
#[tauri::command]
fn session_delete(project: String, id: String) -> Result<(), String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .delete_permanently(&id)
            .await
            .map_err(message)
    })
}

/// D154: the project's unresolved work. Open approvals lead the list as one block; their rows
/// carry the session, agent and turn the asking surface recorded, so the view can open the
/// transcript row. Everything below it is grouped by session.
#[tauri::command]
fn attention_list(project: String) -> Result<Attention, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let view = rigel_core::attention::attention(service.pool(), rigel_core::migrate::now_ms())
            .await
            .map_err(message)?;
        let open = ApprovalBus::new(index_dir()).pending_for(Path::new(&project));
        Ok(view.with_approvals(&open, Path::new(&project)))
    })
}

/// D152: project-local session templates. The file lives beside the session database and is
/// never committed; the three commands are the only way the window touches it.
#[tauri::command]
fn template_list(project: String) -> Result<Vec<SessionTemplate>, String> {
    SessionTemplates::open(Path::new(&project))
        .list()
        .map_err(message)
}

#[tauri::command]
fn template_save(project: String, template: SessionTemplate) -> Result<SessionTemplate, String> {
    SessionTemplates::open(Path::new(&project))
        .save(template)
        .map_err(message)
}

#[tauri::command]
fn template_delete(project: String, id: String) -> Result<(), String> {
    SessionTemplates::open(Path::new(&project))
        .delete(&id)
        .map_err(message)
}

/// D156: the recorded chain behind one patch, read-only. A missing link comes back empty; the
/// window prints "unknown" for it rather than a guess.
#[tauri::command]
fn patch_provenance(project: String, patch_id: String) -> Result<Provenance, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        rigel_core::provenance::patch(service.pool(), &patch_id)
            .await
            .map_err(message)
    })
}

/// D150: one list per session, and a toggle so the star is the same command on and off.
#[tauri::command]
fn bookmark_list(project: String, session: String) -> Result<Vec<Bookmark>, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .bookmarks(&session)
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn bookmark_toggle(
    project: String,
    session: String,
    kind: String,
    target_id: String,
    label: String,
) -> Result<Option<Bookmark>, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .toggle_bookmark(&session, &kind, &target_id, &label)
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn session_get(project: String, id: String) -> Result<Session, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .get(&id)
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn session_transcript(project: String, id: String) -> Result<Vec<TurnTranscript>, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .transcript(&id)
            .await
            .map_err(message)
    })
}

fn image_data_url(bytes: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// One rendered page for the session inspector. This is a fresh Rigel b render of the URL an
/// external harness visited; the fork normally writes the exact frame itself, and this remains
/// the fallback for a harness configured without `--capture`.
#[derive(Debug, Clone, Serialize)]
pub struct BrowserCapture {
    pub image: String,
    pub url: String,
    pub bytes: usize,
}

#[tauri::command]
async fn browser_capture(url: String) -> Result<BrowserCapture, String> {
    let parsed = reqwest::Url::parse(&url)
        .map_err(|_| "Browser captures need an absolute http or https URL.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Browser captures need an absolute http or https URL.".to_string());
    }
    let exe = rigel_core::agent::obscura::locate().ok_or_else(|| {
        format!(
            "Rigel b is not available. {}",
            rigel_core::agent::obscura::INSTALL_HINT
        )
    })?;
    let path = rigel_core::agent::obscura::capture_path(&index_dir(), "snapshots");
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(message)?;
    }

    let mut command = tokio::process::Command::new(exe);
    command
        .arg("fetch")
        .arg(parsed.as_str())
        .arg("--screenshot")
        .arg(&path)
        .args(["--wait-until", "networkidle0", "--timeout", "30", "--quiet"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        command.creation_flags(0x0800_0000);
    }

    let output = match tokio::time::timeout(Duration::from_secs(60), command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(format!("could not start Rigel b: {error}"));
        }
        Err(_) => {
            let _ = tokio::fs::remove_file(&path).await;
            return Err("Rigel b did not finish within 60 seconds".to_string());
        }
    };
    if !output.status.success() {
        let _ = tokio::fs::remove_file(&path).await;
        let stderr = String::from_utf8_lossy(&output.stderr)
            .trim()
            .chars()
            .take(2_000)
            .collect::<String>();
        return Err(if stderr.is_empty() {
            format!("Rigel b exited with {}", output.status)
        } else {
            stderr
        });
    }

    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(format!("Rigel b returned no screenshot: {error}"));
        }
    };
    let _ = tokio::fs::remove_file(&path).await;
    if bytes.is_empty() {
        return Err("Rigel b returned an empty screenshot.".to_string());
    }
    Ok(BrowserCapture {
        image: image_data_url(&bytes),
        url: parsed.to_string(),
        bytes: bytes.len(),
    })
}

/// The exact frame the forked MCP server wrote after an external harness action.
#[derive(Debug, Clone, Serialize)]
pub struct BrowserHarnessCapture {
    pub image: String,
    pub url: Option<String>,
    pub tool: Option<String>,
    pub captured_at: u64,
    pub bytes: usize,
}

#[tauri::command]
async fn browser_harness_capture(
    session: String,
    agent: String,
) -> Result<BrowserHarnessCapture, String> {
    let path = rigel_core::agent::obscura::harness_capture_path(&index_dir(), &session, &agent);
    let base = fs::canonicalize(index_dir().join("browser").join("external"))
        .map_err(|_| "Rigel b has not captured a harness page yet.".to_string())?;
    let file = fs::canonicalize(&path)
        .map_err(|_| "Rigel b has not captured this page yet.".to_string())?;
    if !file.starts_with(&base) {
        return Err("That path is not a Rigel b harness capture.".to_string());
    }
    let metadata = fs::metadata(&file).map_err(message)?;
    if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 {
        return Err("That Rigel b harness capture is not displayable.".to_string());
    }
    let bytes = tokio::fs::read(&file).await.map_err(message)?;
    let meta: serde_json::Value = fs::read_to_string(file.with_extension("json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(serde_json::Value::Null);
    let captured_at = meta["at"]
        .as_u64()
        .or_else(|| {
            metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|elapsed| elapsed.as_millis() as u64)
        })
        .unwrap_or_default();
    Ok(BrowserHarnessCapture {
        image: image_data_url(&bytes),
        url: meta["url"].as_str().map(str::to_string),
        tool: meta["tool"].as_str().map(str::to_string),
        captured_at,
        bytes: bytes.len(),
    })
}

/// Read a screenshot `browser_open` saved. Only paths below this machine's Rigel browser
/// directory are readable, so a transcript cannot turn the image reader into a file oracle.
#[tauri::command]
async fn browser_image(path: String) -> Result<String, String> {
    let base = fs::canonicalize(index_dir().join("browser"))
        .map_err(|_| "No browser capture has been saved yet.".to_string())?;
    let file = fs::canonicalize(PathBuf::from(&path))
        .map_err(|_| "That browser capture is no longer available.".to_string())?;
    if !file.starts_with(&base) {
        return Err("That path is not a Rigel browser capture.".to_string());
    }
    let metadata = fs::metadata(&file).map_err(message)?;
    if !metadata.is_file() {
        return Err("That browser capture is not a file.".to_string());
    }
    if metadata.len() > 16 * 1024 * 1024 {
        return Err("That browser capture is too large to display.".to_string());
    }
    let bytes = tokio::fs::read(&file).await.map_err(message)?;
    Ok(image_data_url(&bytes))
}

/// D147: the palette's one read. Skills come from the local index; sessions, turns and patches
/// from this project's database. The current session sorts first, and every section is capped so
/// a one-letter query cannot return the whole history. A missing index is not a failed search —
/// the workspace half still answers.
#[derive(Debug, Clone, Serialize)]
pub struct PaletteSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub grade: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PaletteResults {
    pub skills: Vec<PaletteSkill>,
    pub sessions: Vec<rigel_core::SessionHit>,
    pub turns: Vec<rigel_core::TurnHit>,
    pub patches: Vec<rigel_core::PatchHit>,
}

#[tauri::command]
fn search_all(
    project: Option<String>,
    query: String,
    current_session: Option<String>,
) -> Result<PaletteResults, String> {
    tauri::async_runtime::block_on(async move {
        let mut skills = Vec::new();
        if !query.trim().is_empty()
            && let Ok(index) = SkillIndex::open(&index_dir()).await
            && let Ok(hits) = index.search(&query, 6).await
        {
            skills = hits
                .into_iter()
                .map(|hit| PaletteSkill {
                    id: hit.id,
                    name: hit.name,
                    description: hit.description,
                    grade: hit.grade,
                    source: hit.source,
                })
                .collect();
        }

        let workspace = match project.as_deref().filter(|root| !root.is_empty()) {
            Some(root) => {
                let service = SessionService::open(Path::new(root))
                    .await
                    .map_err(message)?;
                rigel_core::search::workspace(service.pool(), &query, current_session.as_deref(), 6)
                    .await
                    .map_err(message)?
            }
            None => rigel_core::WorkspaceHits::default(),
        };

        Ok(PaletteResults {
            skills,
            sessions: workspace.sessions,
            turns: workspace.turns,
            patches: workspace.patches,
        })
    })
}

/// Reconcile a session with the tree and the patch journal (P3.11).
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsBundle {
    pub path: String,
    pub body: String,
}

/// D82: assemble a diagnostics bundle only when the user clicks the action. It gathers the
/// doctor-shaped facts this window can check, the newest turn and the local logs; every string
/// is redacted with the provider keys before it is written, and nothing is sent anywhere.
#[tauri::command]
fn diagnostics_export(
    project: String,
    session: Option<String>,
) -> Result<DiagnosticsBundle, String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        let service = SessionService::open(&root).await.map_err(message)?;
        let (integrity, schema) = diagnostics::database_health(service.pool()).await;

        let mut chosen = match session {
            Some(id) => service.get(&id).await.ok(),
            None => None,
        };
        if chosen.is_none() {
            chosen = service.list().await.ok().and_then(|mut all| {
                all.sort_by_key(|session| std::cmp::Reverse(session.created_at));
                all.into_iter().next()
            });
        }
        let last_turn: Option<TurnTranscript> = match &chosen {
            Some(session) => service
                .transcript(&session.id)
                .await
                .ok()
                .and_then(|mut turns| turns.pop()),
            None => None,
        };

        let data_dir = index_dir();
        let keys: serde_json::Map<String, serde_json::Value> = credentials::PROVIDERS
            .iter()
            .map(|provider| {
                (
                    (*provider).to_string(),
                    serde_json::json!(credentials::present(provider)),
                )
            })
            .collect();
        let doctor = serde_json::json!({
            "app": {
                "version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
            },
            "project": root.display().to_string(),
            "database": {
                "integrity": integrity,
                "schema": schema.iter().map(|(version, name)| serde_json::json!({
                    "version": version, "name": name
                })).collect::<Vec<_>>(),
            },
            "keychain": credentials::available().is_ok(),
            "keys_present": keys,
            "crash_log": diagnostics::crash_log_path(&data_dir).display().to_string(),
        });

        let mut log = diagnostics::recent_crash(&data_dir);
        if let Some(tail) = diagnostics::log_tail(&data_dir.join("logs/rigel.log"), 64 * 1024) {
            if !log.is_empty() {
                log.push('\n');
            }
            log.push_str(&tail);
        }
        if log.is_empty() {
            log.push_str("No local log or crash record has been written yet.");
        }

        let secrets = diagnostics::secrets();
        let body = diagnostics::assemble(&doctor, last_turn.as_ref(), &log, &secrets);
        let path = diagnostics::write_bundle(&data_dir, &body).map_err(message)?;
        Ok(DiagnosticsBundle {
            path: path.display().to_string(),
            body,
        })
    })
}

/// D91: which rule files a turn would read, for Settings. Read-only; no file is changed.
#[tauri::command]
fn rules_sources(project: String) -> Result<Vec<rigel_core::rules::RuleSource>, String> {
    let root = std::fs::canonicalize(&project).map_err(message)?;
    Ok(rigel_core::rules::load(&root).sources)
}

/// D91: the committed memory rendered as one copyable `AGENTS.md` block.
#[tauri::command]
fn rules_export_memory(project: String) -> Result<String, String> {
    let root = std::fs::canonicalize(&project).map_err(message)?;
    Ok(rigel_core::rules::export_memory(&root))
}

/// D97: the exact title and body the next click would send to gh.
#[tauri::command]
fn pull_request_plan(
    project: String,
    session: String,
) -> Result<rigel_core::forge::PullRequestPlan, String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        let service = SessionService::open(&root).await.map_err(message)?;
        let session = service.get(&session).await.map_err(message)?;
        let turns = service.transcript(&session.id).await.map_err(message)?;
        rigel_core::forge::plan(&root, &session, &turns).map_err(message)
    })
}

/// D97: the explicit click. gh authenticates itself; Rigel never sees a forge token.
#[tauri::command]
fn pull_request_create(project: String, session: String) -> Result<String, String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        let service = SessionService::open(&root).await.map_err(message)?;
        let session = service.get(&session).await.map_err(message)?;
        let turns = service.transcript(&session.id).await.map_err(message)?;
        let plan = rigel_core::forge::plan(&root, &session, &turns).map_err(message)?;
        rigel_core::forge::create(&root, &plan).map_err(message)
    })
}

#[tauri::command]
fn checkpoint_list(
    project: String,
    session: String,
) -> Result<Vec<rigel_core::Checkpoint>, String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        SessionService::open(&root)
            .await
            .map_err(message)?
            .checkpoints(&session)
            .await
            .map_err(message)
    })
}

/// D89: put the tree back to the snapshot a turn recorded. Explicit only; the preview and the
/// journal stay available, so this is a reviewed restore rather than a rollback of history.
#[tauri::command]
fn checkpoint_restore(
    project: String,
    id: String,
) -> Result<rigel_core::snapshot::RestoreReport, String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        let service = SessionService::open(&root).await.map_err(message)?;
        let journal = service
            .checkpoint_journal(&id)
            .await
            .map_err(message)?
            .ok_or_else(|| "checkpoint not found".to_string())?;
        let snapshot = journal
            .get("snapshot_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                "this checkpoint predates per-turn snapshots; resume it instead".to_string()
            })?;
        SnapshotService::new(&root, root.join(".rigel"))
            .restore(snapshot, &root)
            .map_err(message)
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionExport {
    pub markdown_path: String,
    pub json_path: String,
    pub markdown: String,
}

/// D95: one Markdown bundle per session, with the JSON beside it. The same redaction path as
/// diagnostics; writes under the app data folder so a session's machine paths never land in a
/// project that might be committed.
#[tauri::command]
fn session_export(project: String, id: String) -> Result<SessionExport, String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        let service = SessionService::open(&root).await.map_err(message)?;
        let session = service.get(&id).await.map_err(message)?;
        let agents = service.agents(&id).await.map_err(message)?;
        let turns = service.transcript(&id).await.map_err(message)?;
        let patches = PatchPipeline::new(&root, service.pool().clone())
            .list(&id)
            .await
            .unwrap_or_default();
        let (markdown, json) = diagnostics::session_bundle(
            &session,
            &agents,
            &turns,
            &patches,
            &diagnostics::secrets(),
        );
        let (md_path, json_path) =
            diagnostics::write_session_bundle(&index_dir(), &id, &markdown, &json)
                .map_err(message)?;
        Ok(SessionExport {
            markdown_path: md_path.display().to_string(),
            json_path: json_path.display().to_string(),
            markdown,
        })
    })
}

/// Q268: run the one test command Rigel can recognise for this project. The action is explicit,
/// local, and outside the agent loop; the core bounds the captured output.
#[tauri::command]
async fn tests_run(project: String) -> Result<rigel_core::testrun::TestRun, String> {
    let root = PathBuf::from(project);
    if !root.is_dir() {
        return Err("That project folder is not available.".to_string());
    }
    rigel_core::testrun::run(&root).await.map_err(message)
}

#[tauri::command]
fn session_resume(project: String, id: String, force: bool) -> Result<ResumeOutcome, String> {
    tauri::async_runtime::block_on(async move {
        let root = PathBuf::from(&project);
        let service = SessionService::open(&root).await.map_err(message)?;
        rigel_core::resume::resume(&service, &root, &id, force)
            .await
            .map_err(message)
    })
}

/// Roadmap 37: the exact turns a dead process left running.
#[tauri::command]
fn session_recovery(project: String, id: String) -> Result<Option<RecoveryOffer>, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .recovery_offer(&id)
            .await
            .map_err(message)
    })
}

/// Roadmap 37: write the tombstone and close the discarded turn in one transaction.
#[tauri::command]
fn turn_discard(project: String, turn: String) -> Result<Turn, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .discard_turn(&turn, "discarded after crash")
            .await
            .map_err(message)
    })
}

/* --------------------------------- agents -------------------------------- */

#[tauri::command]
fn agent_list(project: String, session: String) -> Result<Vec<Agent>, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .agents(&session)
            .await
            .map_err(message)
    })
}

/// The session work board: which agent holds which files, so the roster can show it.
#[tauri::command]
fn agent_team(project: String, session: String) -> Result<Vec<TeamAgent>, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .team(&session)
            .await
            .map_err(message)
    })
}

/// Register an agent. Other harnesses arrive in Phase 4; saying so is better than a button
/// that quietly does nothing.
#[tauri::command]
fn agent_spawn(
    project: String,
    session: String,
    harness: Option<String>,
    role: Option<String>,
    model: Option<String>,
) -> Result<Agent, String> {
    let harness = harness.unwrap_or_else(|| "native".to_string());
    if !matches!(harness.as_str(), "native" | "claude") {
        return Err(format!(
            "{harness} is not wired in the window yet; native and claude are"
        ));
    }
    let model = model.unwrap_or_else(|| load_settings().model);
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .register_agent_with(&session, &harness, role.as_deref(), Some(&model))
            .await
            .map_err(message)
    })
}

/// D94: the four role presets, for the roster picker.
#[tauri::command]
fn role_presets() -> Vec<rigel_core::agent::RolePreset> {
    rigel_core::agent::ROLE_PRESETS.to_vec()
}

/// D94: apply a preset from the roster. The next turn reads the new role.
#[tauri::command]
fn role_apply(
    project: String,
    session: String,
    agent: String,
    preset: String,
) -> Result<Agent, String> {
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .apply_role_preset(&session, &agent, &preset)
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn agent_stop(state: State<'_, AppRuntime>, project: String, agent: String) -> Result<(), String> {
    state.cancels.cancel_agent(&agent);
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .set_agent_status(&agent, "exited")
            .await
            .map_err(message)
    })
}

/* --------------------------------- turns --------------------------------- */

/// Create a turn and start it. Returns as soon as the row exists; the answer streams on
/// `rigel://events`.
///
/// Two injected handles plus six caller fields. `invoke` matches arguments by name, so the
/// signature has to stay flat; `rigel-core::agent::tools` carries the same allow.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn turn_send(
    app: tauri::AppHandle,
    state: State<'_, AppRuntime>,
    project: String,
    session: String,
    agent: String,
    prompt: String,
    model: Option<String>,
    // The provider the renderer picked *with* the model. Without it a save that has not landed
    // yet (or an agent row written on spawn) would send the model through the previous door.
    provider: Option<String>,
    effort: Option<String>,
) -> Result<TurnReply, String> {
    let root = PathBuf::from(&project);
    let service = tauri::async_runtime::block_on(SessionService::open(&root)).map_err(message)?;
    let settings = load_settings();

    // A role preset (D94) carries its own model/effort pair. It wins over the composer's
    // defaults, because applying the preset is the explicit choice; a custom role changes
    // nothing and the caller's values stand.
    let agent_row = tauri::async_runtime::block_on(service.agent(&agent)).map_err(message)?;
    let preset = rigel_core::agent::roles::preset(agent_row.role.as_deref());
    let effort = preset
        .map(|preset| preset.effort.to_string())
        .or(effort)
        .unwrap_or_else(|| settings.effort.clone());
    if !valid_effort(&effort) {
        return Err(format!(
            "unknown effort `{effort}`; use low, medium or high"
        ));
    }
    // The provider picks the default model; the effort is applied on the wire, never folded
    // into the id the provider chose.
    // One decision, in the core: the model and its door travel together. A preset owns the model,
    // so its door is the saved one; otherwise the renderer's pair is the pick. Neither a stale
    // agent row nor an unlanded save can move the request to another provider, and a model id can
    // never select Vertex — its namespace is also OpenRouter's spelling of Gemini.
    let requested_model = preset
        .map(|preset| preset.model.to_string())
        .or(model)
        .or_else(|| agent_row.model.clone());
    let requested_provider = if preset.is_some() { None } else { provider };
    let explanation = explain_route(
        &settings,
        requested_provider.as_deref(),
        requested_model.as_deref(),
    )?;
    let provider = explanation.resolved.provider.clone();
    let model_name = wire_model(&explanation.resolved.model, &effort, &provider);

    // A Claude turn is not the native loop: the harness runs it and the window's approval card
    // answers its questions. The row is created here so the renderer gets it back at once, and
    // `claude::start` adopts that same row (P4.6).
    if agent_row.harness == "claude" {
        let turn = tauri::async_runtime::block_on(service.append_turn(&agent, Some(&prompt)))
            .map_err(message)?;
        let cap = tauri::async_runtime::block_on(service.get(&session))
            .ok()
            .and_then(|stored| stored.budget_cents);
        claude::start(
            app,
            claude::ClaudeTurn {
                service,
                root,
                session_id: session,
                agent: agent_row,
                turn: turn.clone(),
                prompt,
                model: model_name,
                publish_after_turn: settings.cloud_sync_sessions,
            },
        )?;
        return Ok(TurnReply {
            turn,
            budget_cents: cap,
            route: explanation,
        });
    }

    // E2E: a scripted provider replaces the network door. Everything after this point — the
    // approvals, tools, patches, transcript and bill — is the production turn loop. The seam is
    // an environment variable, so a shipped build never takes it unless a test sets it.
    // Roadmap 26: the project's OpenRouter preferences are read once, here, and travel with the
    // request. A default policy adds no field, so a project without the file sends what it did.
    let routing_policy = openrouter::RoutingPolicy::load(&root);
    let model: Arc<dyn Model> = match std::env::var_os("RIGEL_E2E_SCRIPT") {
        Some(script) => Arc::new(ScriptedModel::from_path(Path::new(&script)).map_err(message)?),
        None => {
            // A provider with no default (its roster is read live by the picker) refuses an empty
            // model id: the provider's own 400 would say less than this does.
            if model_name.trim().is_empty() {
                return Err(format!(
                    "no model chosen for `{provider}`; pick one in the model picker"
                ));
            }

            // A saved id can outlive its roster. Check it while the launch pass has it cached.
            guard_live_model(&provider, &model_name)?;

            // One row of the adapter table says where the turn goes: the key, the base URL and
            // the request profile all come from it. A local router that is down is a process to
            // launch, not a configuration error, and `build` starts it before it sends anything.
            tauri::async_runtime::block_on(providers::build_with_policy(
                &provider,
                &credentials::KeychainCredentialStore,
                Some(routing_policy),
            ))?
        }
    };

    let cancels = state.cancels.clone();
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels.clone() as Arc<dyn InterruptSink>,
    ));

    tauri::async_runtime::block_on(budget.reserve(&agent)).map_err(message)?;
    let turn = tauri::async_runtime::block_on(service.append_turn(&agent, Some(&prompt)))
        .map_err(message)?;

    // The window is the approval surface (P4.10e): a command is published to the channel the card
    // reads, and the turn waits for the user's scope. Nothing is allowed by default, and a card
    // nobody answers is a refusal.
    let tools = Tools::new(
        root.clone(),
        service.clone(),
        budget.clone(),
        index_dir(),
        StopAt::Never,
    )
    .with_scopes(Scopes::new(&root, index_dir()))
    .with_command_approval(Arc::new(WindowApprover {
        bus: ApprovalBus::new(index_dir()),
        session_id: session.clone(),
        agent_id: agent.clone(),
        turn_id: turn.id.clone(),
        project_root: root.clone(),
    }))
    .with_plan_approval(Arc::new(WindowApprover {
        bus: ApprovalBus::new(index_dir()),
        session_id: session.clone(),
        agent_id: agent.clone(),
        turn_id: turn.id.clone(),
        project_root: root.clone(),
    }))
    .with_mail_approval(Arc::new(WindowApprover {
        bus: ApprovalBus::new(index_dir()),
        session_id: session.clone(),
        agent_id: agent.clone(),
        turn_id: turn.id.clone(),
        project_root: root.clone(),
    }));
    let coordinator = Coordinator {
        session: service.clone(),
        budget,
        tools,
        model,
        sink: Arc::new(WindowSink { app: app.clone() }),
        cancels,
        project_root: root,
        model_name,
        effort: Some(effort),
        max_steps: usize::from(settings.max_steps),
        subagents: subagent_policy(&settings),
        depth: 0,
        parent_cancel: None,
        worktree_dir: Some(index_dir()),
        chat_parent: None,
    };

    let returned = turn.clone();
    let session_id = session;
    let agent_id = agent;
    let prompt = prompt.clone();
    let publish_after_turn = settings.cloud_sync_sessions;
    let publish_service = service.clone();
    let publish_session_id = session_id.clone();
    let cap = tauri::async_runtime::block_on(service.get(&session_id))
        .ok()
        .and_then(|stored| stored.budget_cents);

    // A native turn runs on its own thread with a current-thread runtime. It is a blocking
    // job, and keeping it off Tauri's runtime means a long model call cannot starve the
    // window's own tasks. `block_on` also sidesteps the higher-ranked future lifetime that
    // `spawn` cannot prove `Send` for (sqlx's executor bound).
    std::thread::Builder::new()
        .name("rigel-turn".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = app.emit(
                        "rigel://events",
                        AgentEvent::Error {
                            turn_id: None,
                            message: format!("could not start the turn runtime: {error}"),
                        },
                    );
                    return;
                }
            };
            if let Err(error) =
                runtime.block_on(coordinator.run_existing(&session_id, &agent_id, turn, &prompt))
            {
                let _ = app.emit(
                    "rigel://events",
                    AgentEvent::Error {
                        turn_id: None,
                        message: error.to_string(),
                    },
                );
            }
            // The switch is captured when the turn starts. A failure is logged, not shown on the
            // card: the turn itself succeeded, and the next publish can catch up.
            if publish_after_turn {
                let publish = async {
                    let cloud = rigel_core::cloud::Cloud::open(&index_dir())
                        .map_err(|error| error.to_string())?;
                    cloud
                        .publish_session(&publish_service, &publish_session_id)
                        .await
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                };
                if let Err(error) = runtime.block_on(publish) {
                    eprintln!("rigel: session not published: {error}");
                }
            }
        })
        .map_err(|error| format!("could not start the turn thread: {error}"))?;

    Ok(TurnReply {
        turn: returned,
        budget_cents: cap,
        route: explanation,
    })
}

/* -------------------------------- approvals ------------------------------- */

/// Answers `run_command` from the window instead of from a terminal.
///
/// The question goes onto the approval channel, the card lists it, and this waits for the scope
/// the user picked. Tools already holds the grants, so a "session" answer is checked there before
/// this is ever called; here every question is new. End of turn, a closed window or ten minutes of
/// silence all end as a refusal.
struct WindowApprover {
    bus: ApprovalBus,
    /// D154: the turn this approver answers for. One is built per `turn_send`, so every
    /// question it publishes can point back at the transcript row that asked.
    session_id: String,
    agent_id: String,
    turn_id: String,
    /// D154: the project this question belongs to; Attention only shows its own.
    project_root: PathBuf,
}

#[async_trait::async_trait]
impl CommandApprover for WindowApprover {
    async fn approve(&self, request: &CommandRequest) -> CommandDecision {
        let key = format!("command:{}", request.command);
        let pending = PendingApproval::now("command", &key, &request.command)
            .in_project(&self.project_root)
            .in_session(&self.session_id, &self.agent_id)
            .on_turn(&self.turn_id)
            .with_detail(serde_json::json!({
                "cwd": request.cwd.display().to_string(),
                "project_root": request.project_root.display().to_string(),
                "timeout_secs": request.timeout_secs,
                "destructive": request.destructive.iter().map(|finding| {
                    serde_json::json!({ "id": finding.id, "summary": finding.summary })
                }).collect::<Vec<_>>(),
                // Q239: resolved targets and counts computed from the same classifier. This
                // is part of the question, not a separate approval path.
                "blast_radius": request.blast_radius,
                // Q73: the tool's own preview output (or an explicit no-preview note) is part
                // of the question, so the card never guesses at the effect.
                "dry_run": request.dry_run,
                // Q243: a script contributed by a loaded skill is part of the question, and
                // its own grant is required even when the command text is already approved.
                "skill_scripts": request.skill_scripts,
                "deliberation": request.deliberation.as_ref().map(|round| {
                    serde_json::json!({
                        "note": round.note,
                        "positions": round.positions.iter().map(|position| {
                            serde_json::json!({ "stance": position.stance, "text": position.text })
                        }).collect::<Vec<_>>(),
                    })
                }),
            }));
        // Q36: one silent deadline earns exactly one republish. Both attempts are still
        // refusals when nobody answers; the retry never turns silence into an approval.
        match self
            .bus
            .ask_with_retry(pending, std::time::Duration::from_secs(600), 1)
            .await
        {
            Ok(scope) => CommandDecision::Allow(scope),
            Err(_) => CommandDecision::Deny,
        }
    }
}

/// Q251: a committed-memory write is a question on the same card. It is project-local but
/// permanent, so it is never approved by a command grant; the card shows the exact file and
/// content, and silence is a refusal.
struct WindowMemoryApprover {
    bus: ApprovalBus,
    project_root: PathBuf,
}

#[async_trait::async_trait]
impl MemoryApprover for WindowMemoryApprover {
    async fn approve(&self, request: &MemoryRequest) -> MemoryDecision {
        let key = format!("memory:{}:{}", request.agent_id, request.topic);
        let pending = PendingApproval::now("memory", &key, format!("write {}", request.path))
            .in_project(&self.project_root)
            .with_detail(serde_json::json!({
                "agent_id": request.agent_id,
                "topic": request.topic,
                "mode": request.mode,
                "path": request.path,
                "content": request.content,
                "preview": request.preview,
            }));
        match self
            .bus
            .ask(pending, std::time::Duration::from_secs(600))
            .await
        {
            Ok(_) => MemoryDecision::Allow,
            Err(_) => MemoryDecision::Deny,
        }
    }
}

/// One outgoing email is a question on the same card. A sent message cannot be recalled, so the
/// recipient, subject and body are all put in front of the user; silence is a refusal, exactly
/// as it is for a command.
#[async_trait::async_trait]
impl MailApprover for WindowApprover {
    async fn approve(&self, request: &MailRequest) -> MailDecision {
        let key = format!("mail:{}:{}", request.to, request.subject);
        let title = format!("Send email to {}", request.to);
        let pending = PendingApproval::now("mail", &key, &title)
            .in_project(&self.project_root)
            .in_session(&self.session_id, &self.agent_id)
            .on_turn(&self.turn_id)
            .with_detail(serde_json::json!({
                "from": request.from,
                "to": request.to,
                "subject": request.subject,
                "text": request.text,
            }));
        match self
            .bus
            .ask(pending, std::time::Duration::from_secs(600))
            .await
        {
            Ok(_) => MailDecision::Allow,
            Err(_) => MailDecision::Deny,
        }
    }
}

/// D114: a writable fan-out is a plan question on the same card, with its own verbs.
///
/// The card lists each child's role, files and plan; approving starts the fan-out, skipping
/// records the user's reason, and anything else refuses. The same ten-minute silence rule
/// applies: no answer is no permission.
#[async_trait::async_trait]
impl PlanApprover for WindowApprover {
    async fn approve_plan(&self, request: &PlanRequest) -> PlanDecision {
        let roles = request
            .agents
            .iter()
            .map(|agent| agent.role.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let pending = PendingApproval::now(
            "plan",
            format!("plan:{}:{}", request.session_id, request.agents.len()),
            format!("writable agents: {roles}"),
        )
        .in_project(&self.project_root)
        .in_session(&self.session_id, &self.agent_id)
        .on_turn(&self.turn_id)
        .with_detail(serde_json::json!({
            "session_id": request.session_id,
            "agents": request.agents,
        }));
        self.bus
            .ask_plan(pending, std::time::Duration::from_secs(600))
            .await
    }
}

/// D154: every open question for this project, oldest first. The card polls this after a turn
/// starts; another project's question is not this window's to answer.
#[tauri::command]
fn approval_pending(project: String) -> Vec<PendingApproval> {
    ApprovalBus::new(index_dir()).pending_for(Path::new(&project))
}

/// How often the window looks for a change in the approval channel. The channel is a directory
/// of small files, and the surfaces keep their own poll as the fallback.
const APPROVAL_POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// D154: tell the window when the shared approval channel changes, so the Session card and the
/// Attention count refresh on the question itself instead of on their next poll.
///
/// The channel is a directory — one JSON file per open question — so watching its entries covers
/// every publisher, including the MCP sidecar, which has no window handle and publishes from
/// another process. A file's name and length are the whole signal: a new question, an answer
/// that removed one, and the last write completing a file all register as a change.
fn spawn_approval_watcher(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("rigel-approvals".to_string())
        .spawn(move || {
            let dir = ApprovalBus::new(index_dir()).pending_dir();
            let mut seen: Option<Vec<(String, u64)>> = None;
            loop {
                let mut now = std::fs::read_dir(&dir)
                    .map(|entries| {
                        entries
                            .flatten()
                            .map(|entry| {
                                let length = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                                (entry.file_name().to_string_lossy().into_owned(), length)
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                now.sort();
                if seen.as_ref() != Some(&now) {
                    // The first look is only the baseline: a question already waiting is read
                    // when a surface mounts. After that, every change is worth a refresh.
                    if seen.is_some() {
                        let _ = app.emit("rigel://approval", ());
                    }
                    seen = Some(now);
                }
                std::thread::sleep(APPROVAL_POLL);
            }
        })
        .ok();
}

/// Answer one question. `None` is a refusal, which is also what an unknown scope means: an
/// unreadable answer never becomes an allow.
#[tauri::command]
fn approval_answer(
    id: String,
    scope: Option<String>,
    skip: Option<bool>,
    note: Option<String>,
) -> Result<(), String> {
    // A plan skip is an answer in its own right; it needs no scope.
    let skip = skip.unwrap_or(false);
    let scope = match scope.as_deref() {
        None | Some("deny") | Some("reject") => None,
        Some(_) if skip => None,
        Some(text) => match ApprovalScope::parse(text) {
            Some(scope) => Some(scope),
            None => {
                return Err(format!(
                    "`{text}` is not a scope: once, session, project, always"
                ));
            }
        },
    };
    ApprovalBus::new(index_dir()).answer_with(&id, scope, skip, note)
}

/* ---------------------------- claims and conflicts ------------------------ */

/// The project the window is watching. A global so the watcher thread can start at app boot,
/// before any window has opened a session, and simply idle until one does.
static WATCHED_ROOT: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Everything the Diff view needs about coordination in one call.
#[derive(Debug, Clone, Serialize)]
pub struct CoordinationView {
    pub claims: Vec<rigel_core::Claim>,
    pub unverified_changes: Vec<UnverifiedChange>,
    pub conflicts: Vec<ClaimConflict>,
}

fn remember_project(root: &Path) {
    if let Ok(mut watched) = WATCHED_ROOT.lock() {
        *watched = Some(root.to_path_buf());
    }
    persist_current_project(root);
    persist_app_state(true);
}

/// Publish the project the window has open so a headless `rigel serve` can follow it. The
/// server polls this file and reopens its session database when the window switches projects;
/// a temp file plus rename means a reader never sees half a path. Failure is not fatal to the
/// window, which still has its project in memory.
fn persist_current_project(root: &Path) {
    let path = index_dir().join("current-workspace.json");
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let body = serde_json::json!({ "project": root.to_string_lossy() });
    let temp = path.with_extension("json.tmp");
    if std::fs::write(&temp, body.to_string()).is_ok() {
        let _ = std::fs::rename(&temp, &path);
    }
}

/// The window's liveness file. `updated_at` is refreshed every ten seconds; a stale file is a
/// crashed window, not an open one. The API reads this to decide whether Muse should wait.
fn persist_app_state(open: bool) {
    let path = index_dir().join("app-state.json");
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let body = serde_json::json!({
        "open": open,
        "project": current_project_root().map(|root| root.to_string_lossy().into_owned()),
        "updated_at": rigel_core::migrate::now_ms(),
    });
    let temp = path.with_extension("json.tmp");
    if std::fs::write(&temp, body.to_string()).is_ok() {
        let _ = std::fs::rename(&temp, &path);
    }
}

/// Restart a room harness this window started when it stays busy without posting a reply.
/// The API runs the same watcher for the agents it started; each process can only restart its
/// own wrappers, because the room status carries no pid.
fn watch_room_agents() {
    if std::env::var("RIGEL_ROOM_WATCHDOG")
        .map(|value| value.trim() == "0")
        .unwrap_or(false)
    {
        return;
    }
    let stuck_after = std::env::var("RIGEL_ROOM_STUCK_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds >= 30)
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(900));
    let state = Arc::new(Mutex::new(rigel_core::agent::agentchat::Watchdog::default()));
    rigel_core::agent::agentchat::spawn_watchdog(
        state,
        || current_project_root().map(|path| path.to_string_lossy().into_owned()),
        stuck_after,
        Duration::from_secs(30),
    );
}

pub(crate) fn current_project_root() -> Option<PathBuf> {
    WATCHED_ROOT.lock().ok().and_then(|root| root.clone())
}

/// Tell the watcher which project to follow. Called when a session is opened.
#[tauri::command]
fn watch_project(project: String) {
    remember_project(Path::new(&project));
}

/// Who leads a Swarm. Rigel is the Muse coordinator; a model override asks that same coordinator
/// to think with a particular model. Broadcast has no boss and is sent by the window directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum SwarmLead {
    Rigel,
    Broadcast,
    Model { provider: String, model: String },
}

/// Queue one Swarm task for Rigel, the Muse coordinator. The API holds the JSON file until Muse
/// claims it, so the window does not need to know whether Muse is connected right now. The job
/// names the project, the room agents the preflight chose, and the big-boss choice; Muse decides
/// how to split the work.
#[tauri::command]
fn swarm_enqueue(
    project: String,
    task: String,
    targets: Vec<String>,
    lead: Option<SwarmLead>,
) -> Result<String, String> {
    let root = PathBuf::from(project.trim());
    if !root.is_dir() {
        return Err("Open a project before starting a swarm.".to_string());
    }
    let task = task.trim().to_string();
    if task.is_empty() {
        return Err("Type the swarm task first.".to_string());
    }
    if task.chars().count() > 20_000 {
        return Err("The swarm task is longer than 20,000 characters.".to_string());
    }
    let mut unique: Vec<String> = Vec::new();
    for target in targets {
        let target = target.trim().to_string();
        if target.is_empty() || unique.iter().any(|known| known == &target) {
            continue;
        }
        if target.chars().count() > 40 || target.chars().any(char::is_control) {
            return Err(format!("{target} is not a valid room agent name"));
        }
        unique.push(target);
    }
    if unique.is_empty() {
        return Err("Pick at least one room agent for the swarm.".to_string());
    }

    let lead = match lead.unwrap_or(SwarmLead::Rigel) {
        SwarmLead::Broadcast => {
            return Err(
                "A broadcast swarm is sent directly to the room, not queued for Rigel.".to_string(),
            );
        }
        SwarmLead::Rigel => SwarmLead::Rigel,
        SwarmLead::Model { provider, model } => {
            let provider = provider.trim().to_ascii_lowercase();
            if providers::find(&provider).is_none() {
                return Err(providers::unknown_provider(&provider));
            }
            let model = model.trim().to_string();
            if model.is_empty() {
                return Err("Pick the model Rigel should use as the big boss.".to_string());
            }
            if model.chars().count() > 200 {
                return Err("The big boss model id is longer than 200 characters.".to_string());
            }
            SwarmLead::Model { provider, model }
        }
    };

    let inbox = index_dir().join("swarm").join("inbox");
    fs::create_dir_all(&inbox).map_err(message)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let id = format!("swarm-{stamp}-{}", std::process::id());
    let body = serde_json::json!({
        "id": id,
        "project": root.to_string_lossy(),
        "prompt": task,
        "targets": unique,
        "lead": lead,
        "created_at": rigel_core::migrate::now_ms(),
    });
    let path = inbox.join(format!("{id}.json"));
    let temp = inbox.join(format!("{id}.json.tmp"));
    fs::write(&temp, body.to_string()).map_err(message)?;
    fs::rename(&temp, &path).map_err(message)?;
    // The room bridge forwards every agent's subsequent output. Queueing the job itself remains
    // local and durable; Muse is woken by the domain request the operator/bridge sends.
    Ok(id)
}

#[tauri::command]
fn claim_list(project: String) -> Result<CoordinationView, String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        let service = SessionService::open(&root).await.map_err(message)?;
        let claims = ClaimService::new(service.pool().clone(), &root);
        Ok(CoordinationView {
            claims: claims.held(None).await.map_err(message)?,
            unverified_changes: claims.unverified().await.map_err(message)?,
            conflicts: claims.conflicts().await.map_err(message)?,
        })
    })
}

#[tauri::command]
fn conflict_resolve(project: String, id: String, resolution: String) -> Result<(), String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        let service = SessionService::open(&root).await.map_err(message)?;
        ClaimService::new(service.pool().clone(), &root)
            .resolve_conflict(&id, &resolution)
            .await
            .map_err(message)
    })
}

/// The three rendered versions of a conflict (D79). Read once, when the row is expanded; the
/// two-second coordination poll must not carry whole files.
#[tauri::command]
fn conflict_sides(project: String, id: String) -> Result<rigel_core::ConflictSides, String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        let service = SessionService::open(&root).await.map_err(message)?;
        ClaimService::new(service.pool().clone(), &root)
            .sides(&id)
            .await
            .map_err(message)
    })
}

/// Merge the three sides and return the text, writing nothing. The Diff view edits this, then
/// hands it back through `conflict_resolve_merged`.
#[tauri::command]
fn conflict_merge(project: String, id: String) -> Result<rigel_core::MergedConflict, String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        let service = SessionService::open(&root).await.map_err(message)?;
        ClaimService::new(service.pool().clone(), &root)
            .merge(&id)
            .await
            .map_err(message)
    })
}

/// Write the merged text through the CAS patch pipeline and resolve the conflict (D79). The
/// patch is attributed to `user`, and the row records which patch carried the file.
#[tauri::command]
fn conflict_resolve_merged(
    project: String,
    session: String,
    id: String,
    merged: String,
) -> Result<rigel_core::MergeResolution, String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        let service = SessionService::open(&root).await.map_err(message)?;
        let pipeline = PatchPipeline::new(&root, service.pool().clone());
        ClaimService::new(service.pool().clone(), &root)
            .resolve_with_merge(&pipeline, &session, &id, &merged)
            .await
            .map_err(message)
    })
}

/// One writable subagent, as the Diff view lists it (D80). The pending diff is deliberately
/// not here: this is read by the two-second coordination poll, and a worktree diff can be a
/// whole file.
#[derive(Debug, Clone, Serialize)]
pub struct WritableView {
    pub agent_id: String,
    pub role: Option<String>,
    pub model: Option<String>,
    pub status: String,
    pub worktree: Option<String>,
    pub plan: Option<String>,
    pub plan_note: Option<String>,
    /// The latest merge verdict, merged or not, approved or overridden.
    pub review: Option<MergeReview>,
}

#[tauri::command]
fn writable_list(project: String, session: String) -> Result<Vec<WritableView>, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let reviews = MergeReviews::new(service.pool().clone());
        let mut out = Vec::new();
        for row in service.writable_agents(&session).await.map_err(message)? {
            let review = reviews.latest(&row.agent.id).await.map_err(message)?;
            out.push(WritableView {
                agent_id: row.agent.id,
                role: row.agent.role,
                model: row.agent.model,
                status: row.agent.status,
                worktree: row.agent.worktree,
                plan: row.plan,
                plan_note: row.plan_note,
                review,
            });
        }
        Ok(out)
    })
}

/// The worktree's pending changes as one unified diff. Read once, when the row is expanded.
#[derive(Debug, Clone, Serialize)]
pub struct WritableDiff {
    pub agent_id: String,
    pub files: Vec<String>,
    pub diff: String,
}

#[tauri::command]
fn writable_diff(project: String, agent: String) -> Result<WritableDiff, String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        let service = SessionService::open(&root).await.map_err(message)?;
        if !service.is_writable(&agent).await.map_err(message)? {
            return Err(format!("{agent} is not a writable subagent"));
        }
        let data = index_dir();
        let worktrees = WorktreeService::new(&root, &data, service.pool().clone());
        let (diff, files) = worktrees.diff(&agent).await.map_err(message)?;
        Ok(WritableDiff {
            agent_id: agent,
            files,
            diff,
        })
    })
}

/// The Diff view's decision about a recorded review: merge an approved one that a conflict
/// interrupted, or cross a block with a typed reason (D115). No model is called — the verdict
/// stands as written, the reason is stored beside it, and the worktree must still hold exactly
/// the diff the verdict was about.
#[tauri::command]
fn writable_merge(
    project: String,
    session: String,
    agent: String,
    reason: Option<String>,
) -> Result<AppliedReview, String> {
    tauri::async_runtime::block_on(async move {
        let root = std::fs::canonicalize(&project).map_err(message)?;
        let service = SessionService::open(&root).await.map_err(message)?;
        let data = index_dir();
        let worktrees = WorktreeService::new(&root, &data, service.pool().clone());
        let reviews = MergeReviews::new(service.pool().clone());
        apply_review(
            &service,
            &worktrees,
            &reviews,
            &session,
            &agent,
            reason.as_deref(),
        )
        .await
        .map_err(message)
    })
}

/// Watch the open project for writes by agents other than the claimant. The watcher ticks every
/// [`POLL_INTERVAL`], which is also the worst-case detection latency (P5.8: within two seconds).
fn spawn_conflict_watcher(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("rigel-conflicts".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(_) => return,
            };
            runtime.block_on(async move {
                let mut watcher: Option<(PathBuf, ConflictWatcher)> = None;
                loop {
                    tokio::time::sleep(POLL_INTERVAL).await;
                    let root = match WATCHED_ROOT.lock() {
                        Ok(watched) => watched.clone(),
                        Err(_) => None,
                    };
                    let Some(root) = root else {
                        watcher = None;
                        continue;
                    };
                    if watcher.as_ref().map(|(path, _)| path) != Some(&root) {
                        watcher = Some((root.clone(), ConflictWatcher::new(&root)));
                    }
                    let Some((_, conflicts)) = watcher.as_mut() else {
                        continue;
                    };
                    let Ok(service) = SessionService::open(&root).await else {
                        continue;
                    };
                    let Ok(sessions) = service.list().await else {
                        continue;
                    };
                    let Some(session) = sessions
                        .iter()
                        .find(|session| session.status == rigel_core::SessionStatus::Active)
                        .or_else(|| sessions.first())
                    else {
                        continue;
                    };
                    let Ok(agents) = service.agents(&session.id).await else {
                        continue;
                    };
                    let running: Vec<Agent> = agents
                        .into_iter()
                        .filter(|agent| agent.status == "running")
                        .collect();
                    if running.is_empty() {
                        continue;
                    }
                    let claims = ClaimService::new(service.pool().clone(), &root);
                    if let Ok(recorded) = conflicts.step(&claims, &running).await {
                        for conflict in recorded {
                            let _ = app.emit("rigel://conflict", &conflict);
                        }
                    }
                }
            });
        })
        .ok();
}

#[tauri::command]
fn turn_cancel(state: State<'_, AppRuntime>, turn: String) -> Result<(), String> {
    state.cancels.cancel_turn(&turn);
    Ok(())
}

/// Q254: pause at the next step boundary. The same turn row and message list are kept.
#[tauri::command]
fn turn_pause(state: State<'_, AppRuntime>, turn: String) -> Result<(), String> {
    state.cancels.pause_turn(&turn);
    Ok(())
}

#[tauri::command]
fn turn_resume(state: State<'_, AppRuntime>, turn: String) -> Result<(), String> {
    state.cancels.resume_turn(&turn);
    Ok(())
}

/// Q39: queue a user note for the running turn; it lands at the next step boundary.
#[tauri::command]
fn turn_steer(state: State<'_, AppRuntime>, turn: String, text: String) -> Result<bool, String> {
    Ok(state.cancels.steer(&turn, &text))
}

/* --------------------------------- network -------------------------------- */

/// Q240: the project-local kill switch and origin allowlist. The same file is read by
/// `Tools::new`, so the window and CLI cannot disagree about the policy.
#[tauri::command]
fn network_get(project: String) -> Result<NetworkPolicy, String> {
    Ok(NetworkPolicy::load(Path::new(&project)))
}

#[tauri::command]
fn network_set(project: String, policy: NetworkPolicy) -> Result<NetworkPolicy, String> {
    let root = Path::new(&project);
    if !root.is_dir() {
        return Err("open a project before changing its network policy".to_string());
    }
    policy.save(root)?;
    Ok(policy)
}

/* --------------------------------- budget -------------------------------- */

/// Price the first step of a turn before it is sent. Reads only: no turn row, no spend.
///
/// The model defaults to the settings model, because that is what `turn_send` will use
/// when the renderer passes no model.
#[tauri::command]
fn turn_estimate(
    project: String,
    session: String,
    prompt: String,
    model: Option<String>,
    provider: Option<String>,
    effort: Option<String>,
) -> Result<TurnQuote, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let settings = load_settings();
        let route = rigel_core::agent::routing::resolve(
            &settings.provider,
            &settings.model,
            provider.as_deref(),
            model.as_deref(),
        )?;
        let effort = effort.unwrap_or_else(|| settings.effort.clone());
        let model_name = wire_model(&route.model, &effort, &route.provider);
        let context_chars = fixed_context_chars(&model_name);
        BudgetService::with_null_interrupt(service.pool().clone())
            .quote_session(&session, &model_name, context_chars, &prompt)
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn budget_get(project: String, agent: String) -> Result<rigel_core::BudgetSnapshot, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        BudgetService::with_null_interrupt(service.pool().clone())
            .snapshot(&agent)
            .await
            .map_err(message)
    })
}

/// Set the cap without counting an intervention (settings editing). The transcript's 80%
/// prompt uses `budget_raise`, which records one (§8.8).
#[tauri::command]
fn budget_set(project: String, session: String, cap_cents: Option<i64>) -> Result<Session, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let budget = BudgetService::with_null_interrupt(service.pool().clone());
        budget.set_cap(&session, cap_cents).await.map_err(message)?;
        service.get(&session).await.map_err(message)
    })
}

#[tauri::command]
fn budget_raise(project: String, session: String, cap_cents: i64) -> Result<Session, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let budget = BudgetService::with_null_interrupt(service.pool().clone());
        budget
            .raise_cap(&session, cap_cents)
            .await
            .map_err(message)?;
        service.get(&session).await.map_err(message)
    })
}

/* --------------------------------- safety -------------------------------- */

/// D113 and D116 in one read: how many agents may write at once, what the day may cost, and
/// what it has cost so far.
#[derive(Debug, Clone, Serialize)]
pub struct SafetyView {
    pub writable_agents: i64,
    pub daily_cap_micros: Option<i64>,
    pub day: String,
    pub today_micros: i64,
}

#[tauri::command]
fn safety_get(project: String) -> Result<SafetyView, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let budget = BudgetService::with_null_interrupt(service.pool().clone());
        let settings = budget.safety_settings().await.map_err(message)?;
        let daily = budget.daily_status().await.map_err(message)?;
        Ok(SafetyView {
            writable_agents: settings.writable_agents,
            daily_cap_micros: settings.daily_cap_micros,
            day: daily.day,
            today_micros: daily.spend_micros,
        })
    })
}

/// Change one knob. The daily cap takes dollars, which is what the user types; the ledger
/// stores micro-dollars, which is what every other amount uses. `clear_daily_cap` turns the
/// cap off without pretending it was set to zero.
#[tauri::command]
fn safety_set(
    project: String,
    session: String,
    writable_agents: Option<i64>,
    daily_cap_cents: Option<i64>,
    clear_daily_cap: Option<bool>,
) -> Result<SafetyView, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let budget = BudgetService::with_null_interrupt(service.pool().clone());
        if let Some(count) = writable_agents {
            budget.set_writable_agents(count).await.map_err(message)?;
        }
        if clear_daily_cap.unwrap_or(false) {
            budget
                .set_daily_cap(&session, None)
                .await
                .map_err(message)?;
        } else if let Some(cents) = daily_cap_cents {
            budget
                .set_daily_cap(&session, Some(cents * 10_000))
                .await
                .map_err(message)?;
        }
        let settings = budget.safety_settings().await.map_err(message)?;
        let daily = budget.daily_status().await.map_err(message)?;
        Ok(SafetyView {
            writable_agents: settings.writable_agents,
            daily_cap_micros: settings.daily_cap_micros,
            day: daily.day,
            today_micros: daily.spend_micros,
        })
    })
}

/* ---------------------------------- plan --------------------------------- */

/// D98: whether this session is planning, and when a plan was last approved.
#[tauri::command]
fn plan_get(project: String, session: String) -> Result<rigel_core::plan::PlanState, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        rigel_core::plan::state(service.pool(), &session)
            .await
            .map_err(message)
    })
}

/// D98: turn plan mode on, or approve the plan by turning it off (which records the hand-off).
#[tauri::command]
fn plan_set(
    project: String,
    session: String,
    enabled: bool,
) -> Result<rigel_core::plan::PlanState, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        rigel_core::plan::set(service.pool(), &session, enabled)
            .await
            .map_err(message)
    })
}

/// Roadmap 42: every subagent graph in this session, with its edges and states.
#[tauri::command]
fn subagent_graph(
    project: String,
    session: String,
) -> Result<Vec<rigel_core::SubagentGraph>, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        service.dag(&session).await.map_err(message)
    })
}

/// D92: spend across every session in this project.
#[tauri::command]
fn spend_report(project: String) -> Result<rigel_core::spend::SpendReport, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        rigel_core::spend::report(service.pool())
            .await
            .map_err(message)
    })
}

/// D92: cap one task (agent) below the session cap. `None` removes it.
#[tauri::command]
fn task_cap_set(
    project: String,
    session: String,
    agent: String,
    cap_cents: Option<i64>,
) -> Result<rigel_core::BudgetSnapshot, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let budget = BudgetService::with_null_interrupt(service.pool().clone());
        let cap = cap_cents.map(|cents| cents * 10_000);
        budget
            .set_task_cap(&session, &agent, cap)
            .await
            .map_err(message)?;
        budget.snapshot(&agent).await.map_err(message)
    })
}

/* --------------------------------- patches ------------------------------- */

#[tauri::command]
fn patch_list(project: String, session: String) -> Result<Vec<Patch>, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        PatchPipeline::new(Path::new(&project), service.pool().clone())
            .list(&session)
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn patch_hunks(project: String, session: String) -> Result<Vec<AttributedHunk>, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        PatchPipeline::new(Path::new(&project), service.pool().clone())
            .attributed_hunks(&session)
            .await
            .map_err(message)
    })
}

/* ----------------------------- hunk comments ----------------------------- */

#[tauri::command]
fn hunk_comment_add(
    project: String,
    hunk: String,
    body: String,
    author: Option<String>,
) -> Result<HunkComment, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let comments = HunkComments::new(service.pool().clone());
        comments
            .add(&hunk, &body, author.as_deref().unwrap_or("user"))
            .await
            .map_err(message)
    })
}

#[tauri::command]
fn hunk_comments(project: String, session: String) -> Result<Vec<HunkComment>, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let comments = HunkComments::new(service.pool().clone());
        comments.for_session(&session).await.map_err(message)
    })
}

/// Send a comment to the agent that wrote the hunk, as a turn. The comment is marked
/// delivered with the turn's id, so a failure to record that leaves it pending and visible
/// rather than silently answered.
#[tauri::command]
fn hunk_comment_deliver(
    app: tauri::AppHandle,
    state: State<'_, AppRuntime>,
    project: String,
    session: String,
    id: String,
) -> Result<TurnReply, String> {
    let service = tauri::async_runtime::block_on(SessionService::open(Path::new(&project)))
        .map_err(message)?;
    let comments = HunkComments::new(service.pool().clone());
    let comment = tauri::async_runtime::block_on(comments.get(&id))
        .map_err(message)?
        .ok_or_else(|| format!("no comment {id}"))?;

    let reply = turn_send(
        app,
        state,
        project,
        session,
        comment.agent_id.clone(),
        comment.prompt(),
        None,
        None,
        None,
    )?;

    tauri::async_runtime::block_on(comments.deliver(&id, &reply.turn.id)).map_err(message)?;
    Ok(reply)
}

#[tauri::command]
fn patch_revert(project: String, id: String) -> Result<Patch, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        PatchPipeline::new(Path::new(&project), service.pool().clone())
            .revert(&id)
            .await
            .map_err(message)
    })
}

/* --------------------------------- memory -------------------------------- */

#[tauri::command]
fn memory_read(project: String, agent: String, topic: String) -> Result<Option<String>, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        MemoryService::new(Path::new(&project), service.pool().clone())
            .read(&agent, &topic)
            .map_err(message)
    })
}

#[tauri::command]
fn memory_write(
    project: String,
    agent: String,
    topic: String,
    content: String,
    mode: Option<String>,
) -> Result<MemoryFile, String> {
    let mode = mode
        .as_deref()
        .and_then(WriteMode::parse)
        .unwrap_or(WriteMode::Append);
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let root = PathBuf::from(&project);
        MemoryService::new(&root, service.pool().clone())
            .with_approval(Arc::new(WindowMemoryApprover {
                bus: ApprovalBus::new(index_dir()),
                project_root: root,
            }))
            .write(&agent, &topic, &content, mode)
            .await
            .map_err(message)
    })
}

/* --------------------------------- keys ---------------------------------- */

#[tauri::command]
fn key_status() -> Result<KeyStatus, String> {
    Ok(current_key_status())
}

/// Store a key from the settings form. A chat provider's candidate is checked against its
/// roster before the keychain is touched, so a rejected key leaves the previous one in place
/// (Q244). Harness credentials are stored as before and probed by their own program.
#[tauri::command]
fn key_set(provider: String, value: String) -> Result<KeySetResult, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("paste a key first".to_string());
    }
    let check = if providers::find(&provider).is_some() {
        Some(tauri::async_runtime::block_on(catalog::validate_key(
            &provider, value,
        ))?)
    } else {
        None
    };
    credentials::set(&provider, value).map_err(message)?;
    clear_model_cache();
    // A harness can become ready the moment its key lands; do not keep the old verdict cached.
    rigel_core::agent::agentchat::clear_readiness_cache();
    Ok(KeySetResult {
        status: current_key_status(),
        check,
    })
}

#[tauri::command]
fn key_delete(provider: String) -> Result<KeyStatus, String> {
    credentials::delete(&provider).map_err(message)?;
    clear_model_cache();
    rigel_core::agent::agentchat::clear_readiness_cache();
    Ok(current_key_status())
}

/* -------------------------------- gravity -------------------------------- */

/// The opt-in Gravity integration, as Settings sees it. The broker URL, never a publisher key.
#[derive(Debug, Clone, Serialize)]
pub struct GravityStatus {
    pub enabled: bool,
    pub test_ads: bool,
    pub placement_id: String,
    pub endpoint: String,
}

#[tauri::command]
fn gravity_settings() -> GravityStatus {
    let settings = load_settings();
    GravityStatus {
        enabled: settings.gravity_ads,
        test_ads: settings.gravity_test_ads,
        placement_id: rigel_core::gravity::PLACEMENT_ID.to_string(),
        endpoint: rigel_core::gravity::shared_endpoint(),
    }
}

/// One ad request for the Home card. Disabled means **no request at all**: the renderer gets an
/// empty list and draws no slot. The request goes to the shared Firebase broker, so no publisher
/// key is stored on this machine.
#[tauri::command]
fn gravity_ad(
    session: String,
    messages: Vec<rigel_core::gravity::Message>,
    device: Option<rigel_core::gravity::Device>,
) -> Result<Vec<rigel_core::gravity::Ad>, String> {
    let settings = load_settings();
    if !settings.gravity_ads {
        return Ok(Vec::new());
    }
    let mut request = rigel_core::gravity::AdRequest::new(
        session,
        messages,
        rigel_core::gravity::install_id(&index_dir()),
    );
    if let Some(device) = device {
        request = request.with_device(device);
    }
    let answer = tauri::async_runtime::block_on(
        rigel_core::gravity::SharedGravity::configured().get_ads(&request),
    )
    .map_err(message)?;
    if answer.test && !settings.gravity_test_ads {
        return Ok(Vec::new());
    }
    Ok(answer.ads)
}

/// Load the impression URL once, from Rust. The webview CSP forbids third-party images and
/// connections, and the signed tracking URL should not be in the renderer anyway.
#[tauri::command]
fn gravity_impression(url: String) -> Result<(), String> {
    tauri::async_runtime::block_on(rigel_core::gravity::track(&url)).map_err(message)
}

/// Record the click, then open the advertiser's landing page in the system browser. Returns the
/// landing URL for the caller's receipt; the window itself never navigates cross-origin.
#[tauri::command]
fn gravity_click(url: String) -> Result<String, String> {
    let landing = tauri::async_runtime::block_on(rigel_core::gravity::click_through(&url))
        .map_err(message)?;
    open_in_browser(&landing)?;
    Ok(landing)
}

fn open_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not open the browser: {error}"))
}

/* ------------------------------- harnesses -------------------------------- */

/// The four harness probes, cached in `adapters.json` and refreshed when a binary changes.
/// Missing binaries come back as `installed: false` with an install hint, never as an error.
#[tauri::command]
fn harness_probe(force: bool) -> Result<Vec<rigel_adapters::HarnessInfo>, String> {
    let path = index_dir().join(rigel_adapters::registry::CACHE_FILE);
    let cache = tauri::async_runtime::block_on(rigel_adapters::probe_cached(
        &path,
        &credentials::KeychainCredentialStore,
        force,
    ))
    .map_err(message)?;
    Ok(cache.infos())
}

/* ------------------------------- omniroute -------------------------------- */

/// Whether the local router answers, and where it lives. This is a short health probe, not a
/// key test: Settings needs to know if the server is up, not whether the key is right.
#[tauri::command]
fn omniroute_status() -> Result<rigel_core::ServerStatus, String> {
    Ok(tauri::async_runtime::block_on(
        rigel_core::agent::omniroute::status(),
    ))
}

/// Start the router when it is down. `omniroute serve --daemon` is launched and health is
/// polled, so a success here means the next turn can actually reach it.
#[tauri::command]
fn omniroute_start() -> Result<rigel_core::ServerStatus, String> {
    tauri::async_runtime::block_on(rigel_core::agent::omniroute::ensure_server())
}

/// The picker's last answer, held for five minutes. The read is network-bound and the two pickers
/// (Session and Settings) would otherwise each pay for it on mount.
type ModelCache = std::sync::Mutex<Option<(std::time::Instant, String, Vec<ModelOption>)>>;
static MODEL_CACHE: std::sync::OnceLock<ModelCache> = std::sync::OnceLock::new();

fn clear_model_cache() {
    if let Some(cache) = MODEL_CACHE.get()
        && let Ok(mut guard) = cache.lock()
    {
        *guard = None;
    }
}

/// The model picker's rows. The catalogue itself lives in the core (`agent::catalog`) so the
/// window and `rigel models` cannot disagree about what a provider offers.
#[tauri::command]
fn models_list() -> Result<Vec<ModelOption>, String> {
    models_cached(false)
}

/// Read every roster again, ignoring the cache. The launch pass and Free mode use it so a model
/// that disappeared (or appeared) upstream is seen the same day rather than after the next restart.
#[tauri::command]
fn models_refresh() -> Result<Vec<ModelOption>, String> {
    models_cached(true)
}

fn models_cached(force: bool) -> Result<Vec<ModelOption>, String> {
    let provider = load_settings().provider.trim().to_lowercase();
    let cache = MODEL_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    if !force
        && let Ok(guard) = cache.lock()
        && let Some((read_at, cached_for, rows)) = guard.as_ref()
        && cached_for == &provider
        // A minute, not five: free rows churn, and a picker that shows a row the door has
        // already dropped sends a 400 the user cannot explain.
        && read_at.elapsed() < std::time::Duration::from_secs(60)
    {
        return Ok(rows.clone());
    }
    let rows = tauri::async_runtime::block_on(catalog::roster(
        &provider,
        &credentials::KeychainCredentialStore,
    ))?;
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((std::time::Instant::now(), provider, rows.clone()));
    }
    Ok(rows)
}

/// Effort is its own setting (D28). OmniRoute happens to encode the same value in the model
/// id, so when a picked id already carries an effort suffix, keep the two consistent on the
/// wire rather than sending a request that contradicts itself. A `-tiered` id names the family,
/// not a tier: it stays intact, and the tier travels as `reasoning_effort`.
fn wire_model(model: &str, effort: &str, provider: &str) -> String {
    if provider != "omniroute" {
        return model.to_string();
    }
    // Do not invent `...-tiered-low`; the effort field is the tier.
    if model.ends_with("-tiered") {
        return model.to_string();
    }
    for suffix in ["-minimal", "-low", "-medium", "-high", "-xhigh"] {
        if let Some(base) = model.strip_suffix(suffix) {
            return format!("{base}-{effort}");
        }
    }
    model.to_string()
}

/* ------------------------------- agent chat ------------------------------- */

/// Chat tab status. `project` is the open project root (or path input); the vendor
/// checkout is discovered next to it, or via RIGEL_AGENTCHAT_DIR in the core.
#[tauri::command]
fn chat_status(
    project: Option<String>,
) -> Result<rigel_core::agent::agentchat::ChatStatus, String> {
    Ok(tauri::async_runtime::block_on(
        rigel_core::agent::agentchat::status(project.as_deref()),
    ))
}

/// Boot the vendor chat server when it is down. A room already answering is taken over through
/// its landing-page token, so one left behind by an earlier window stays usable.
#[tauri::command]
fn chat_start(project: Option<String>) -> Result<rigel_core::agent::agentchat::ChatStatus, String> {
    tauri::async_runtime::block_on(rigel_core::agent::agentchat::ensure_server(
        project.as_deref(),
    ))
}

#[tauri::command]
fn chat_channels() -> Result<Vec<String>, String> {
    tauri::async_runtime::block_on(rigel_core::agent::agentchat::channels())
}

/// Presence per instance; the vendor shape is passed through untouched.
#[tauri::command]
fn chat_agents() -> Result<serde_json::Value, String> {
    tauri::async_runtime::block_on(rigel_core::agent::agentchat::agents())
}

#[tauri::command]
fn chat_messages(
    channel: Option<String>,
    since_id: Option<u64>,
    limit: Option<u64>,
) -> Result<Vec<rigel_core::agent::agentchat::ChatMessage>, String> {
    tauri::async_runtime::block_on(rigel_core::agent::agentchat::messages(
        channel.as_deref(),
        since_id.unwrap_or(0),
        limit.unwrap_or(100),
    ))
}

/// One room upload as a `data:` URL. The window draws the bytes it is handed and never learns
/// the chat server's origin or token; only images under `/uploads/` are served.
#[tauri::command]
fn chat_attachment(url: String) -> Result<String, String> {
    tauri::async_runtime::block_on(rigel_core::agent::agentchat::attachment(&url))
}

/// A user message over the vendor websocket, exactly like the browser does it.
#[tauri::command]
fn chat_send(channel: String, text: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::block_on(async move {
        rigel_core::agent::agentchat::send(&text, &channel).await?;
        let Some(project) = current_project_root() else {
            return Ok(serde_json::json!({
                "sent": true,
                "kicked": false,
                "reason": "no project is open",
            }));
        };
        let kickoff = rigel_core::swarm::kick_off_muse(&index_dir(), &project, &text).await?;
        Ok(serde_json::json!({
            "sent": true,
            "kicked": kickoff.job_id.is_some(),
            "job_id": kickoff.job_id,
            "targets": kickoff.targets,
            "missing": kickoff.missing,
            "duplicates": kickoff.duplicates,
        }))
    })
}

/// Start the room's agents on a hidden console — they work, nothing opens on screen.
/// Returns the labels launched; an empty list means every agent was already in the room.
#[tauri::command]
fn chat_start_agents(project: Option<String>) -> Result<Vec<String>, String> {
    let free = free_swarm_agent(&load_settings());
    tauri::async_runtime::block_on(rigel_core::agent::agentchat::start_agents_with_free(
        project.as_deref(),
        free.as_ref(),
    ))
}

/// Swarm, Free mode: put the model Free mode chose in the room as `@rigel-free`. Returns `None`
/// when Free mode is off or the chosen door cannot be reached over an OpenAI-compatible API.
#[tauri::command]
fn chat_start_free(project: Option<String>) -> Result<Option<String>, String> {
    let Some(free) = free_swarm_agent(&load_settings()) else {
        return Ok(None);
    };
    tauri::async_runtime::block_on(rigel_core::agent::agentchat::start_free_agent(
        project.as_deref(),
        &free,
    ))
    .map(Some)
}

/// The harness processes Rigel started and still holds. The roster offers Stop only for these;
/// anything else in the room was launched by hand and is the user's to close.
#[tauri::command]
fn chat_launched() -> Vec<rigel_core::agent::agentchat::ChatProcess> {
    rigel_core::agent::agentchat::launched()
}

/// The harnesses the checkout configures, ready or not, so the roster can offer a per-agent start
/// and say why one cannot start.
#[tauri::command]
fn chat_harnesses(
    project: Option<String>,
    force: Option<bool>,
) -> Vec<rigel_core::agent::agentchat::ChatHarness> {
    if force.unwrap_or(false) {
        rigel_core::agent::agentchat::configured_fresh(project.as_deref())
    } else {
        rigel_core::agent::agentchat::configured(project.as_deref())
    }
}

#[tauri::command]
fn chat_start_agent(project: Option<String>, name: String) -> Result<String, String> {
    tauri::async_runtime::block_on(rigel_core::agent::agentchat::start_agent(
        project.as_deref(),
        &name,
    ))
}

/// Stop one agent Rigel started: the wrapper, its CLI and that CLI's children.
#[tauri::command]
fn chat_stop_agent(name: String) -> Result<String, String> {
    tauri::async_runtime::block_on(rigel_core::agent::agentchat::stop_agent(&name))
}

/* ------------------------------ prompt refine ----------------------------- */

/// Refinement is a fixed, small job: a cheap Antigravity model on the local router. Keeping it
/// off the turn's selected door means tidying a draft for the `rigel` agent never spends a relay
/// round-trip.
const REFINE_PROVIDER: &str = credentials::OMNIROUTE;
const REFINE_MODEL: &str = "antigravity/gemini-3.8-flash-tiered";

/// Tighten a composer draft before it is sent, on that fixed cheap, tiered model. It returns questions
/// and a proposed brief and never sends anything: the window shows both as a review card and the
/// send button stays the user's. This call is outside any session budget, so the card shows what
/// it cost on its own.
#[tauri::command]
fn prompt_refine(
    project: Option<String>,
    request: rigel_core::agent::RefineRequest,
) -> Result<rigel_core::agent::Refinement, String> {
    let model_name = REFINE_MODEL.to_string();
    // The composer's effort is the tier a tiered model runs at; it stays on the wire, not in
    // the id.
    let effort = load_settings().effort;
    let model: Arc<dyn Model> = tauri::async_runtime::block_on(providers::build(
        REFINE_PROVIDER,
        &credentials::KeychainCredentialStore,
    ))
    .map_err(|error| {
        format!("the refiner could not start on {REFINE_PROVIDER}/{REFINE_MODEL}: {error}")
    })?;

    let refiner = rigel_core::agent::PromptRefiner::new(model, model_name).with_effort(effort);
    let root = project.map(PathBuf::from);
    let outcome = tauri::async_runtime::block_on(async move {
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            refiner.refine(&request, root.as_deref()),
        )
        .await
        .map_err(|_| "the refiner took too long; send the draft as it is".to_string())?
    })?;
    // A retired Antigravity client answers with an upgrade notice instead of JSON. Treat it as
    // a failed refinement: auto-refine must send the draft the user typed, never that notice.
    let unavailable = outcome.refined.to_ascii_lowercase();
    if unavailable.contains("antigravity") && unavailable.contains("no longer supported") {
        return Err(
            "the installed Antigravity version is no longer supported; update it at \
             https://antigravity.google/download"
                .to_string(),
        );
    }
    Ok(outcome)
}

/* -------------------------------- settings ------------------------------- */

#[tauri::command]
fn settings_get() -> Result<Settings, String> {
    Ok(load_settings())
}

/// Write settings, creating the app data directory if this is a first run.
#[tauri::command]
fn settings_set(mut settings: Settings) -> Result<Settings, String> {
    if providers::find(&settings.provider).is_none() {
        return Err(providers::unknown_provider(&settings.provider));
    }
    // `Vertex` and ` vertex ` name the same door; the file should say so, because a hand-edited
    // settings file is otherwise reported as an unknown provider on the next note.
    settings.provider = settings.provider.trim().to_ascii_lowercase();
    settings.model = settings.model.trim().to_string();
    if !valid_effort(&settings.effort) {
        return Err(format!(
            "unknown effort `{}`; use low, medium or high",
            settings.effort
        ));
    }
    settings.swarm_lead = settings.swarm_lead.trim().to_ascii_lowercase();
    if !matches!(settings.swarm_lead.as_str(), "muse" | "broadcast") {
        return Err(format!(
            "unknown swarm lead `{}`; use muse or broadcast",
            settings.swarm_lead
        ));
    }
    // A settings file is editable by hand, so the numbers are clamped here rather than trusted:
    // a 3-pixel terminal or a 10 000-step turn is a bug report, not a preference.
    settings.terminal_font_size = settings.terminal_font_size.clamp(10, 20);
    settings.terminal_scrollback = settings.terminal_scrollback.clamp(1_000, 100_000);
    settings.terminal_shell = settings.terminal_shell.trim().to_string();
    settings.max_steps = settings.max_steps.clamp(1, 50);
    // D159: four steps, so a hand-edited 117 is a mistake rather than a new mode.
    if !matches!(settings.ui_scale, 90 | 100 | 110 | 125) {
        settings.ui_scale = default_ui_scale();
    }
    settings.vertex_project = settings.vertex_project.trim().to_string();

    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(message)?;
    }
    let text = toml::to_string_pretty(&settings).map_err(message)?;
    fs::write(&path, text).map_err(message)?;
    // The next turn reads this from the core, not from the file, so a save takes effect at once.
    vertex::set_project_override(Some(&settings.vertex_project));
    Ok(settings)
}

/// Free mode: test the best-benchmarked tool-capable free models and save the best one that
/// answers.
///
/// The result carries every attempt, so the window can say what it skipped and why (a 429 from a
/// shared pool reads differently from a 401 on a key). Saving goes through `settings_set`, which
/// is the same path a manual pick uses, so the composer and the next turn cannot disagree.
#[tauri::command]
fn free_mode() -> Result<rigel_core::agent::FreeChoice, String> {
    let current = load_settings().provider.trim().to_lowercase();
    let choice = tauri::async_runtime::block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(285),
            rigel_core::agent::freemode::pick(&current, &credentials::KeychainCredentialStore),
        )
        .await
        .map_err(|_| "Free mode reached its five-minute limit; no model was changed".to_string())?
    })?;
    let mut settings = load_settings();
    settings.provider = choice.provider.clone();
    settings.model = choice.model.clone();
    settings.free_mode = true;
    settings_set(settings)?;
    // The picker must show the same fresh free/paid labels the selection pass just used. Clear the
    // old minute cache; its next open performs a live read with the persisted winning provider first.
    clear_model_cache();
    Ok(choice)
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub current: String,
    pub enabled: bool,
    pub auto_install: bool,
    pub manifest_url: String,
    pub key_present: bool,
}

fn update_status_now() -> Result<UpdateStatus, String> {
    let dir = index_dir();
    let settings = rigel_core::update::load_settings(&dir);
    Ok(UpdateStatus {
        current: env!("CARGO_PKG_VERSION").to_string(),
        enabled: settings.enabled,
        auto_install: settings.auto_install,
        manifest_url: rigel_core::update::manifest_url(&settings).to_string(),
        key_present: rigel_core::update::public_key(&dir).is_ok(),
    })
}

#[tauri::command]
fn update_status() -> Result<UpdateStatus, String> {
    update_status_now()
}

/// The two switches and an optional manifest override, saved beside the app data. The manifest
/// URL is https-only; the default is the latest GitHub release's signed `latest.json`.
#[tauri::command]
fn update_set(
    enabled: bool,
    auto_install: bool,
    manifest_url: Option<String>,
) -> Result<UpdateStatus, String> {
    if let Some(url) = manifest_url.as_deref()
        && !url.trim().is_empty()
        && !url.starts_with("https://")
    {
        return Err("the update manifest must be an https URL".to_string());
    }
    // The window has no URL field, so `null` means "keep whatever the CLI configured". An
    // explicit empty string clears it and returns to the GitHub default.
    let manifest_url = match manifest_url {
        Some(url) if url.trim().is_empty() => None,
        Some(url) => Some(url),
        None => rigel_core::update::load_settings(&index_dir()).manifest_url,
    };
    let settings = rigel_core::update::UpdateSettings {
        enabled,
        auto_install,
        manifest_url,
    };
    rigel_core::update::save_settings(&index_dir(), &settings).map_err(message)?;
    update_status_now()
}

/// One check, made only when the window asks. The core verifies the Ed25519 signature before it
/// reads the version, then compares the signed version with this build.
#[tauri::command]
async fn update_check() -> Result<rigel_core::UpdateCheck, String> {
    rigel_core::update::check_latest(&index_dir(), env!("CARGO_PKG_VERSION"))
        .await
        .map_err(message)
}

/// Download the signed installer, verify its hash, start it and close the window so the installer
/// can replace the executable. A per-machine MSI install uses the MSI twin when the manifest has
/// one; a per-user NSIS install uses the setup executable.
#[tauri::command]
async fn update_install(app: tauri::AppHandle) -> Result<String, String> {
    let dir = index_dir();
    let check = rigel_core::update::check_latest(&dir, env!("CARGO_PKG_VERSION"))
        .await
        .map_err(message)?;
    if !check.available {
        return Err("this build is already current".to_string());
    }
    let (url, sha256) = rigel_core::update::artifact_for(&check, installed_per_machine());
    let path = rigel_core::update::download_artifact(url, sha256, &dir)
        .await
        .map_err(message)?;
    rigel_core::update::install_artifact(&path).map_err(message)?;

    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(1));
        quit(&handle);
    });
    Ok(format!(
        "Installing {} — Rigel will close while the installer runs.",
        check.offered
    ))
}

/// A per-machine MSI lives under Program Files; a per-user NSIS install does not. The updater
/// uses this only to choose between the two signed URLs in the same manifest.
fn installed_per_machine() -> bool {
    let exe = std::env::current_exe()
        .map(|path| path.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|root| PathBuf::from(root).to_string_lossy().to_lowercase())
        .any(|root| !root.is_empty() && exe.starts_with(&root))
}

/* ----------------------------------- mail ----------------------------------- */

/// The inbox and whether a key is stored. No network call: the page has to render an
/// unconfigured app rather than fail to open.
#[tauri::command]
fn mail_status() -> Result<rigel_core::MailStatus, String> {
    Ok(rigel_core::Mail::status_at(&index_dir()))
}

/// Save the inbox address in `agentmail.json`. The address is not a secret.
#[tauri::command]
fn mail_configure(inbox: String) -> Result<rigel_core::MailStatus, String> {
    let config = rigel_core::MailConfig::new(inbox).map_err(message)?;
    rigel_core::MailConfig::save(&index_dir(), &config).map_err(message)?;
    Ok(rigel_core::Mail::status_at(&index_dir()))
}

/// The API key is written to the OS keychain and never read back into this window.
#[tauri::command]
fn mail_set_key(api_key: String) -> Result<rigel_core::MailStatus, String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err("the AgentMail API key is empty".to_string());
    }
    credentials::set(credentials::AGENTMAIL, api_key).map_err(|error| error.to_string())?;
    Ok(rigel_core::Mail::status_at(&index_dir()))
}

/// The conversations in the inbox. Subjects and counts only: no body is fetched until one is
/// opened, so checking the mailbox costs a single request.
#[tauri::command]
async fn mail_threads(limit: Option<usize>) -> Result<Vec<rigel_core::ThreadSummary>, String> {
    let mail = rigel_core::Mail::open(&index_dir()).map_err(message)?;
    mail.threads(limit.unwrap_or(10)).await.map_err(message)
}

/// One conversation, oldest message first. This is the read path the page shows, and the one
/// that fills in any body the thread listing left out.
#[tauri::command]
async fn mail_thread(thread_id: String) -> Result<rigel_core::ThreadView, String> {
    let mail = rigel_core::Mail::open(&index_dir()).map_err(message)?;
    mail.thread(&thread_id).await.map_err(message)
}

/// A send the user asked for from the page. An agent's send goes through the approval card
/// instead; this is the case where the click itself is the consent.
#[tauri::command]
async fn mail_send(
    to: String,
    subject: String,
    text: String,
) -> Result<rigel_core::SentMail, String> {
    let mail = rigel_core::Mail::open(&index_dir()).map_err(message)?;
    mail.send(&to, &subject, &text).await.map_err(message)
}

/* ---------------------------------- cloud ---------------------------------- */

/// The project and the account, read without a network call. An unconfigured app answers with
/// `configured: false` so the page can say what to set instead of failing to open.
#[tauri::command]
fn cloud_status() -> Result<rigel_core::cloud::CloudStatus, String> {
    rigel_core::cloud::Cloud::status_at(&index_dir()).map_err(message)
}

/// Save the Firebase project id and web API key in `cloud.json`. The key is public by design;
/// the Firestore rules, not this file, decide who may read a document.
#[tauri::command]
fn cloud_configure(
    project: String,
    api_key: String,
) -> Result<rigel_core::cloud::CloudStatus, String> {
    let config = rigel_core::cloud::CloudConfig::new(project, api_key).map_err(message)?;
    rigel_core::cloud::CloudConfig::save(&index_dir(), &config).map_err(message)?;
    rigel_core::cloud::Cloud::status_at(&index_dir()).map_err(message)
}

/// One sign-in. The password is used for this call and is never stored; only the refresh token
/// reaches the OS keychain.
#[tauri::command]
async fn cloud_login(
    email: String,
    password: String,
) -> Result<rigel_core::cloud::CloudStatus, String> {
    rigel_core::cloud::Cloud::open(&index_dir())
        .map_err(message)?
        .sign_in(&email, &password)
        .await
        .map_err(message)
}

/// Create the account and keep the session Firebase issues for it — a new account is signed in,
/// not sent back to the form.
#[tauri::command]
async fn cloud_signup(
    email: String,
    password: String,
) -> Result<rigel_core::cloud::CloudStatus, String> {
    rigel_core::cloud::Cloud::open(&index_dir())
        .map_err(message)?
        .sign_up(&email, &password)
        .await
        .map_err(message)
}

/// Ask Firebase to email a reset link. The answer is the same for an unknown address, so the
/// page cannot be used to probe for accounts.
#[tauri::command]
async fn cloud_reset_password(email: String) -> Result<String, String> {
    rigel_core::cloud::Cloud::open(&index_dir())
        .map_err(message)?
        .send_password_reset(&email)
        .await
        .map_err(message)
}

/// Ask Firebase to email a verification link to the signed-in account. Sending again is the
/// resend control; the link, its template and its expiry belong to Firebase.
#[tauri::command]
async fn cloud_send_verification() -> Result<String, String> {
    rigel_core::cloud::Cloud::open(&index_dir())
        .map_err(message)?
        .send_verification_email()
        .await
        .map_err(message)
}

/// Re-read the verification state from Firebase and keep it in the keychain session.
#[tauri::command]
async fn cloud_check_verification() -> Result<rigel_core::cloud::CloudStatus, String> {
    rigel_core::cloud::Cloud::open(&index_dir())
        .map_err(message)?
        .check_email_verification()
        .await
        .map_err(message)
}

/// Point App Check at the user's attestation endpoint, or turn it off with `None` or an empty
/// string. The endpoint is not secret; the tokens it returns are, and never reach the renderer.
#[tauri::command]
fn cloud_app_check(url: Option<String>) -> Result<rigel_core::cloud::CloudStatus, String> {
    let mut config = rigel_core::cloud::CloudConfig::load(&index_dir())
        .map_err(message)?
        .ok_or_else(|| message(rigel_core::cloud::CloudError::NotConfigured))?;
    config.app_check = url
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .map(|attest_url| rigel_core::cloud::AppCheckConfig { attest_url });
    rigel_core::cloud::CloudConfig::save(&index_dir(), &config).map_err(message)?;
    rigel_core::cloud::Cloud::status_at(&index_dir()).map_err(message)
}

/// Store this installation's App Check client credential. The endpoint receives it as a bearer
/// token; the renderer never reads it back, and it is not written to `cloud.json`.
#[tauri::command]
fn cloud_app_check_client_token(token: String) -> Result<rigel_core::cloud::CloudStatus, String> {
    let token = token.trim();
    if token.is_empty() || token.len() > 512 || token.chars().any(char::is_whitespace) {
        return Err(
            "The App Check client token is empty, too long, or contains whitespace.".to_string(),
        );
    }
    rigel_core::credentials::set(rigel_core::credentials::APP_CHECK, token)
        .map_err(|error| error.to_string())?;
    rigel_core::cloud::Cloud::status_at(&index_dir()).map_err(message)
}

#[tauri::command]
fn cloud_app_check_clear_client_token() -> Result<rigel_core::cloud::CloudStatus, String> {
    rigel_core::credentials::delete(rigel_core::credentials::APP_CHECK)
        .map_err(|error| error.to_string())?;
    rigel_core::cloud::Cloud::status_at(&index_dir()).map_err(message)
}

#[tauri::command]
fn cloud_logout() -> Result<rigel_core::cloud::CloudStatus, String> {
    rigel_core::cloud::Cloud::open(&index_dir())
        .map_err(message)?
        .sign_out()
        .map_err(message)
}

/// Mirror the shelf's metadata into the user's project. Bodies stay in the local blob store.
///
/// Sync rather than `async fn`: the SQLite future's `&Path` borrow is not `Send` enough for the
/// command macro's boxed future, the same reason the session commands block on their runtime.
#[tauri::command]
fn cloud_push() -> Result<rigel_core::cloud::PushReport, String> {
    let dir = index_dir();
    tauri::async_runtime::block_on(async move {
        let cloud = rigel_core::cloud::Cloud::open(&dir).map_err(message)?;
        let index = rigel_core::SkillIndex::open(&dir).await.map_err(message)?;
        cloud.push(&index).await.map_err(message)
    })
}

/// Publish one session's prompts and assistant text to the user's Firestore project, where the
/// phone lists and reads them. Tool calls, file changes and the project path never leave here.
#[tauri::command]
fn cloud_publish_session(
    project: String,
    session: String,
) -> Result<rigel_core::cloud::PublishReport, String> {
    let dir = index_dir();
    tauri::async_runtime::block_on(async move {
        let cloud = rigel_core::cloud::Cloud::open(&dir).map_err(message)?;
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        cloud
            .publish_session(&service, &session)
            .await
            .map_err(message)
    })
}

/// Publish every session in the open project in one action, for a phone that should see the
/// whole history rather than only the session that happened to be open.
#[tauri::command]
fn cloud_publish_all(project: String) -> Result<rigel_core::cloud::PublishAllReport, String> {
    let dir = index_dir();
    tauri::async_runtime::block_on(async move {
        let cloud = rigel_core::cloud::Cloud::open(&dir).map_err(message)?;
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        cloud.publish_all(&service).await.map_err(message)
    })
}

/// Bring down rows this machine does not have. A local body is never replaced.
#[tauri::command]
fn cloud_pull() -> Result<rigel_core::cloud::PullReport, String> {
    let dir = index_dir();
    tauri::async_runtime::block_on(async move {
        let cloud = rigel_core::cloud::Cloud::open(&dir).map_err(message)?;
        let index = rigel_core::SkillIndex::open(&dir).await.map_err(message)?;
        cloud.pull(&index).await.map_err(message)
    })
}

/// Replay-ready usage for the status bar: one place computes the money and tokens.
#[tauri::command]
fn usage_summary(project: String, session: String) -> Result<Usage, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let turns = service.transcript(&session).await.map_err(message)?;
        let mut total = Usage::default();
        for turn in turns {
            if let Some(usage) = turn.turn.usage {
                total.add(&usage);
            }
        }
        Ok(total)
    })
}

/// Launch the local router as the window opens, so the first turn does not pay for a cold
/// start. Lazy start in `turn_send` stays as the fallback: this is an optimisation and must
/// never block the window, which is why it runs on its own thread and its error is ignored.
fn warm_router() {
    if load_settings().provider != "omniroute" || !credentials::present(credentials::OMNIROUTE) {
        return;
    }
    std::thread::Builder::new()
        .name("rigel-omniroute".to_string())
        .spawn(|| {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            let _ = runtime.block_on(rigel_core::agent::omniroute::ensure_server());
        })
        .ok();
}

/// The bundled sidecar, when this build has one. An install reads `$RESOURCE/sidecar`; only a
/// debug build may fall back to the repository copy. Baking the build machine's path into a
/// release would let a packaged app read a sidecar from wherever it happened to be compiled.
fn chat_sidecar_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    let packaged = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("sidecar"))
        .filter(|dir| dir.join("agentchattr/run.py").is_file());
    if packaged.is_some() {
        return packaged;
    }
    #[cfg(debug_assertions)]
    {
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/sidecar");
        if dev.join("agentchattr/run.py").is_file() {
            return Some(dev);
        }
    }
    None
}

/// Hand the bundled chat sidecar to the core. The source is copied into app data on first launch,
/// so the room can write while `python.exe` stays in the read-only install directory.
///
/// Absent is not an error: a developer without the sidecar keeps the `vendor/agentchattr` fallback,
/// and the Chat tab explains that case.
fn install_chat_sidecar(app: &tauri::AppHandle) -> Result<String, String> {
    let Some(source) = chat_sidecar_dir(app) else {
        return Ok(String::new());
    };
    rigel_core::agent::agentchat::install_sidecar(source, index_dir().join("agentchat"))
}

/// The release publisher's public key ships beside the executable. Copy it into app data so the
/// CLI and the window verify the same manifest; a rotation replaces the old copy on next launch.
fn install_update_pubkey(app: &tauri::AppHandle) -> Result<(), String> {
    let source = app
        .path()
        .resource_dir()
        .map_err(message)?
        .join("update-pubkey.txt");
    if !source.is_file() {
        return Ok(());
    }
    let target = index_dir().join(rigel_core::UPDATE_PUBKEY_FILE);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(message)?;
    }
    if fs::read_to_string(&source).ok() != fs::read_to_string(&target).ok() {
        fs::copy(&source, &target).map_err(message)?;
    }
    Ok(())
}

/// Launch the chat server when the open project carries its checkout and nothing is answering.
/// Same contract as `warm_router`: a thread, a dropped error, and the Chat tab as the place the
/// reason appears. `ensure_server` already returns without spawning when the checkout is not
/// there, so a fresh clone costs one failed lookup rather than a hanging launch.
fn warm_chat(project: Option<String>) {
    std::thread::Builder::new()
        .name("rigel-agentchat".to_string())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            let _ = runtime.block_on(rigel_core::agent::agentchat::ensure_server(
                project.as_deref(),
            ));
        })
        .ok();
}

/// Re-sync every skill source that is not fresh, in the background, when the shelf's auto-update
/// switch is on (it defaults to on). Same contract as `warm_router`: a thread, a dropped error,
/// and the shelf as the place the result appears — a source that fails is marked `broken` with
/// its reason rather than interrupting the launch. A source inside the stale window is not
/// touched, so this is one tree call per source per day, not a request per launch.
fn warm_skills() {
    if !load_settings().skills_auto_update {
        return;
    }
    std::thread::Builder::new()
        .name("rigel-skills".to_string())
        .spawn(|| {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            let _ = runtime.block_on(async move {
                let index = SkillIndex::open(&index_dir()).await?;
                // Forced, GitHub repositories included: a shelf that was fresh on the last
                // launch is still checked against its remote on this one.
                index.refresh_all().await
            });
        })
        .ok();
}

/// Warm the picker's roster off the interaction path. The read is three vendor calls over the
/// network; doing it at launch means the first open is served from the cache instead of
/// waiting on them. A failure is dropped: the picker retries when it opens.
fn warm_models() {
    std::thread::Builder::new()
        .name("rigel-models".to_string())
        .spawn(|| {
            // Forced: this pass exists to replace whatever the cache held, not to trust it.
            let _ = models_refresh();
        })
        .ok();
}

/// Every local service a session can need, started as the window opens and again once the
/// remembered project is restored (the chat checkout sits beside it). The window calls this
/// once; none of the starts blocks the launch, and a service that cannot start is a status the
/// relevant tab explains rather than an error at boot.
#[tauri::command]
fn servers_start(project: Option<String>) {
    warm_router();
    warm_chat(project);
    warm_skills();
    warm_models();
}

/// A close request hides the window rather than quitting while the tray exists. The flag flips
/// only when the tray was built, so a platform that refuses a tray keeps a working close button
/// instead of stranding a hidden process with no way back.
static TRAY_READY: AtomicBool = AtomicBool::new(false);
/// The window heartbeat keeps `app-state.json` fresh while this process is alive. A crashed
/// window stops refreshing it, so the API can report the app closed without an exit hook.
static APP_OPEN: AtomicBool = AtomicBool::new(false);
/// Set by the tray's Quit item and by an update that needs the executable replaced. A close
/// request is allowed through once this is true, so shutdown is never trapped by the hide.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

fn hide_tray_panel(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("tray") {
        let _ = window.hide();
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    hide_tray_panel(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn show_tray_panel(app: &tauri::AppHandle, x: f64, y: f64) {
    let Some(window) = app.get_webview_window("tray") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    let width = window
        .outer_size()
        .map(|size| size.width as i32)
        .unwrap_or(380);
    let height = window
        .outer_size()
        .map(|size| size.height as i32)
        .unwrap_or(560);
    let x = (x.round() as i32 - width + 12).max(8);
    let y = (y.round() as i32 - height - 12).max(8);
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    let _ = window.show();
    let _ = window.set_focus();
}

#[tauri::command]
fn tray_snapshot() -> Result<tray_status::TraySnapshot, String> {
    tauri::async_runtime::block_on(tray_status::snapshot())
}

#[tauri::command]
fn tray_open(app: tauri::AppHandle) {
    show_main_window(&app);
}

#[tauri::command]
fn tray_hide(app: tauri::AppHandle) {
    hide_tray_panel(&app);
}

#[tauri::command]
fn tray_quit(app: tauri::AppHandle) {
    quit(&app);
}

/// Quit through the same path the tray uses, so an updater-triggered exit is not swallowed by
/// the close-to-tray handler.
fn quit(app: &tauri::AppHandle) {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
    app.exit(0);
}

/// The tray is the way back to a hidden window and the only way to stop the agents while the
/// window is closed. It opens a themed panel instead of an OS-owned menu.
fn install_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id("rigel")
        .tooltip("Rigel — agents and spend live here while the window is closed")
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => show_main_window(tray.app_handle()),
            TrayIconEvent::Click {
                button: MouseButton::Right,
                button_state: MouseButtonState::Up,
                position,
                ..
            } => show_tray_panel(tray.app_handle(), position.x, position.y),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    TRAY_READY.store(true, Ordering::SeqCst);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // D82: a panic writes a redacted record beside the app data before the window disappears.
    diagnostics::install_crash_hook(index_dir());
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // The project the window saved is read after the environment and before the Cloud
            // CLI; load it before the window can start a turn so it survives every launch.
            vertex::set_project_override(Some(&load_settings().vertex_project));
            // The sidecar must be in place before `servers_start` looks for one. The project is
            // not known until the window restores it; `servers_start` runs again from there, and
            // the copied config is pointed at whichever project is open. The router needs none.
            if let Err(problem) = install_chat_sidecar(app.handle()) {
                eprintln!("rigel: chat sidecar not installed: {problem}");
            }
            if let Err(problem) = install_update_pubkey(app.handle()) {
                eprintln!("rigel: update public key not installed: {problem}");
            }
            if let Err(problem) = install_tray(app.handle()) {
                eprintln!("rigel: tray icon not installed: {problem}");
            }
            // The window driver attaches to a single webview, and the tray is a second one; it
            // would be the target it lands on. A shipped build keeps both windows.
            if std::env::var_os("RIGEL_E2E_PROJECT").is_some()
                && let Some(tray) = app.get_webview_window("tray")
            {
                let _ = tray.destroy();
            }
            std::thread::spawn(tray_status::warm);
            relay::start(index_dir(), current_project_root);
            // The window is the supervisor the user actually keeps running: it starts the API on
            // launch and again after a crash, so a reboot no longer leaves Muse with a refusal.
            control::start(index_dir(), current_project_root);
            watch_room_agents();
            warm_router();
            APP_OPEN.store(true, Ordering::SeqCst);
            persist_app_state(true);
            std::thread::spawn(|| {
                while APP_OPEN.load(Ordering::SeqCst) {
                    persist_app_state(true);
                    std::thread::sleep(Duration::from_secs(10));
                }
            });
            spawn_approval_watcher(app.handle().clone());
            spawn_conflict_watcher(app.handle().clone());
            Ok(())
        })
        .manage(AppRuntime {
            cancels: Arc::new(Cancels::new()),
        })
        .on_window_event(|window, event| {
            if window.label() == "tray" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                } else if let tauri::WindowEvent::Focused(false) = event {
                    let _ = window.hide();
                }
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && TRAY_READY.load(Ordering::SeqCst)
                && !SHUTTING_DOWN.load(Ordering::SeqCst)
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            project_open,
            project_create,
            project_workspace_root,
            attachment_import,
            session_start,
            session_list,
            session_rename,
            session_pin,
            session_skill_pins,
            session_skill_pin,
            session_skill_unpin,
            session_skill_review,
            session_skill_keep,
            session_skill_keep_all,
            session_skill_update,
            session_skill_update_all,
            session_archive,
            session_delete,
            bookmark_list,
            bookmark_toggle,
            patch_provenance,
            template_list,
            template_save,
            template_delete,
            attention_list,
            session_get,
            session_transcript,
            browser_capture,
            browser_harness_capture,
            browser_image,
            search_all,
            session_resume,
            session_recovery,
            turn_discard,
            diagnostics_export,
            session_export,
            tests_run,
            rules_sources,
            pull_request_plan,
            pull_request_create,
            rules_export_memory,
            checkpoint_list,
            checkpoint_restore,
            agent_list,
            agent_team,
            agent_spawn,
            agent_stop,
            turn_send,
            turn_cancel,
            turn_pause,
            turn_resume,
            turn_steer,
            network_get,
            network_set,
            approval_pending,
            approval_answer,
            watch_project,
            swarm_enqueue,
            claim_list,
            writable_list,
            writable_diff,
            writable_merge,
            conflict_resolve,
            conflict_sides,
            conflict_merge,
            conflict_resolve_merged,
            turn_estimate,
            budget_get,
            budget_set,
            budget_raise,
            safety_get,
            safety_set,
            patch_list,
            patch_hunks,
            hunk_comment_add,
            hunk_comments,
            hunk_comment_deliver,
            patch_revert,
            memory_read,
            memory_write,
            key_status,
            key_set,
            key_delete,
            gravity_settings,
            gravity_ad,
            gravity_impression,
            gravity_click,
            plan_get,
            plan_set,
            subagent_graph,
            role_presets,
            role_apply,
            spend_report,
            task_cap_set,
            harness_probe,
            providers_list,
            routing_policy,
            custom_provider_get,
            custom_provider_probe,
            custom_provider_save,
            route_explain,
            e2e_config,
            servers_start,
            omniroute_status,
            omniroute_start,
            models_list,
            models_refresh,
            provider_health,
            model_capabilities,
            free_mode,
            chat_status,
            chat_start,
            chat_channels,
            chat_agents,
            chat_messages,
            chat_attachment,
            chat_send,
            chat_start_agents,
            chat_start_free,
            chat_launched,
            chat_harnesses,
            chat_start_agent,
            chat_stop_agent,
            concord::concord_inbox,
            concord::concord_reply,
            terminal::terminal_open,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_close,
            prompt_refine,
            settings_get,
            settings_set,
            update_status,
            update_set,
            update_check,
            update_install,
            cloud_status,
            cloud_configure,
            cloud_login,
            cloud_signup,
            cloud_reset_password,
            cloud_send_verification,
            cloud_check_verification,
            cloud_app_check,
            cloud_app_check_client_token,
            cloud_app_check_clear_client_token,
            cloud_logout,
            mail_status,
            mail_configure,
            mail_set_key,
            mail_threads,
            mail_thread,
            mail_send,
            cloud_push,
            cloud_pull,
            cloud_publish_session,
            cloud_publish_all,
            usage_summary,
            tray_snapshot,
            tray_open,
            tray_hide,
            tray_quit,
            skills_search,
            skills_count,
            skills_load,
            skills_status,
            skills_index,
            skills_refresh,
            skills_install,
            skills_installed,
            mcp_catalog_search,
            mcp_catalog_count,
            mcp_install,
            mcp_installed
        ])
        .build(tauri::generate_context!())
        .expect("rigel: failed to build the window")
        .run(|_, event| {
            // The agents are this app's children; without this, closing the window leaves hidden
            // wrappers holding room slots that the next launch cannot clean up.
            if matches!(event, tauri::RunEvent::Exit) {
                APP_OPEN.store(false, Ordering::SeqCst);
                persist_app_state(false);
                rigel_core::agent::agentchat::stop_all();
                terminal::close_all();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_refiner_is_the_cheap_antigravity_model_on_the_local_router() {
        let spec = providers::find(REFINE_PROVIDER).expect("the local router is a provider");
        assert!(REFINE_MODEL.starts_with("antigravity/"));
        assert!(REFINE_MODEL.contains("3.8-flash-tiered"));
        assert!(providers::is_model(spec, REFINE_MODEL));
    }

    #[test]
    fn a_tiered_id_keeps_its_id_and_effort_is_sent_separately() {
        assert_eq!(
            wire_model("antigravity/gemini-3.8-flash-tiered", "low", "omniroute"),
            "antigravity/gemini-3.8-flash-tiered"
        );
        assert_eq!(
            wire_model("antigravity/gemini-3.7-flash-low", "high", "omniroute"),
            "antigravity/gemini-3.7-flash-high"
        );
        assert_eq!(
            wire_model("antigravity/gemini-3.8-flash-tiered", "high", "openrouter"),
            "antigravity/gemini-3.8-flash-tiered"
        );
    }

    #[test]
    fn a_swarm_lead_records_who_the_big_boss_is() {
        assert_eq!(
            serde_json::to_value(SwarmLead::Rigel).unwrap(),
            serde_json::json!({ "kind": "rigel" })
        );
        assert_eq!(
            serde_json::to_value(SwarmLead::Model {
                provider: "meta".to_string(),
                model: "muse-spark-1.3".to_string(),
            })
            .unwrap(),
            serde_json::json!({
                "kind": "model",
                "provider": "meta",
                "model": "muse-spark-1.3"
            })
        );
        let parsed: SwarmLead = serde_json::from_value(serde_json::json!({
            "kind": "model",
            "provider": "meta",
            "model": "muse-spark-1.3"
        }))
        .unwrap();
        assert!(matches!(parsed, SwarmLead::Model { .. }));
    }

    fn path_string(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn attachment_import_copies_into_the_project() {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = outside.path().join("notes.txt");
        fs::write(&source, "hello").unwrap();

        let imported = attachment_import(path_string(project.path()), vec![path_string(&source)])
            .expect("the file should import");

        assert_eq!(imported, vec![".rigel/attachments/notes.txt"]);
        assert_eq!(
            fs::read_to_string(project.path().join(".rigel/attachments/notes.txt")).unwrap(),
            "hello"
        );
        assert!(
            project
                .path()
                .join(".rigel/attachments/.gitignore")
                .is_file()
        );
    }

    #[test]
    fn a_second_file_with_the_same_name_gets_a_suffix() {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let first = outside.path().join("one").join("notes.txt");
        let second = outside.path().join("two").join("notes.txt");
        fs::create_dir_all(first.parent().unwrap()).unwrap();
        fs::create_dir_all(second.parent().unwrap()).unwrap();
        fs::write(&first, "one").unwrap();
        fs::write(&second, "two").unwrap();

        let imported = attachment_import(
            path_string(project.path()),
            vec![path_string(&first), path_string(&second)],
        )
        .expect("both files should import");

        assert_eq!(
            imported,
            vec![
                ".rigel/attachments/notes.txt",
                ".rigel/attachments/notes-1.txt"
            ]
        );
        assert_eq!(
            fs::read_to_string(project.path().join(".rigel/attachments/notes-1.txt")).unwrap(),
            "two"
        );
    }

    #[test]
    fn a_directory_is_not_an_attachment() {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let directory = outside.path().join("folder");
        fs::create_dir_all(&directory).unwrap();

        let error = attachment_import(path_string(project.path()), vec![path_string(&directory)])
            .expect_err("a directory must be refused");

        assert!(error.contains("is not a file"), "{error}");
    }

    #[test]
    fn an_older_settings_file_gains_the_chat_filter() {
        // A settings file written before D124 has no `chat_models_only`. The picker must still
        // open on chat rows, and the missing field must not cost the rest of the file.
        let old = "theme = \"dark\"\ndefault_budget_cents = 500\nunattended_default = false\n";
        let settings: Settings = toml::from_str(old).expect("an older file should still parse");
        assert!(settings.chat_models_only);
        assert_eq!(settings.default_budget_cents, 500);
    }

    #[test]
    fn cloud_publishing_is_off_in_fresh_and_older_settings() {
        assert!(
            !Settings::default().cloud_sync_sessions,
            "publishing a transcript must be opt-in"
        );
        let old = "theme = \"dark\"\ndefault_budget_cents = 500\nunattended_default = false\n";
        let settings: Settings = toml::from_str(old).expect("an older file should still parse");
        assert!(
            !settings.cloud_sync_sessions,
            "a file written before cloud sync existed must not silently turn it on"
        );
    }

    #[test]
    fn a_desktop_template_restores_a_real_pin_at_its_original_body() {
        use rigel_core::skills::{FetchedSkill, RepoSnapshot, SkillRecord};
        use rigel_core::{SessionTemplate, SkillIndex, TemplateSkill};

        const ID: &str = "github:acme/skills:pdf/SKILL.md";
        const OLD: &str = "---\nname: pdf-reader\ndescription: Read scanned PDFs.\n---\n# PDF reader\n\nAsk for the file, then summarise it.\n";
        const NEW: &str = "---\nname: pdf-reader\ndescription: Read scanned PDFs.\n---\n# PDF reader\n\nAsk for the file, wait for the scan, then summarise it.\n";

        fn snapshot(body: &str) -> RepoSnapshot {
            RepoSnapshot {
                source: "github".to_string(),
                repo: "acme/skills".to_string(),
                branch: "main".to_string(),
                stars: 0,
                license: None,
                truncated: false,
                found: 1,
                http_requests: 0,
                skills: vec![FetchedSkill {
                    record: SkillRecord {
                        id: ID.to_string(),
                        name: "pdf-reader".to_string(),
                        description: "Read scanned PDFs.".to_string(),
                        source: "github".to_string(),
                        repo: Some("acme/skills".to_string()),
                        path: Some("pdf/SKILL.md".to_string()),
                        url: None,
                        body_sha: None,
                        grade: None,
                        grade_scope: None,
                        license: None,
                        stars: None,
                        installs: None,
                        tokens: None,
                        capabilities: Vec::new(),
                        simhash: None,
                    },
                    body: body.to_string(),
                }],
            }
        }

        let tmp = tempfile::tempdir().unwrap();
        let index_root = tmp.path().join("index");
        let project_path = tmp.path().join("project");
        fs::create_dir_all(&project_path).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let index = runtime.block_on(SkillIndex::open(&index_root)).unwrap();
        runtime.block_on(index.ingest(&snapshot(OLD))).unwrap();
        let (_, old_sha) = runtime.block_on(index.body(ID)).unwrap().unwrap();

        let previous = std::env::var_os("RIGEL_INDEX_DIR");
        // SAFETY: this test is the only app test that reads `RIGEL_INDEX_DIR`, and it restores
        // the value before returning. The desktop command wrappers are the subject here.
        unsafe { std::env::set_var("RIGEL_INDEX_DIR", &index_root) };
        let project = path_string(&project_path);

        let original = session_start(project.clone(), Some("original".to_string()), None).unwrap();
        let pinned =
            session_skill_pin(project.clone(), original.id.clone(), ID.to_string(), None).unwrap();
        assert_eq!(pinned.body_sha, old_sha);

        let saved = template_save(
            project.clone(),
            SessionTemplate {
                id: String::new(),
                name: "PDF review".to_string(),
                model: None,
                effort: None,
                max_steps: None,
                agents: Vec::new(),
                skills: vec![TemplateSkill {
                    skill_id: ID.to_string(),
                    body_sha: old_sha.clone(),
                }],
                prompt: "Read the scan.".to_string(),
                created_at: 0,
            },
        )
        .unwrap();

        // The shelf moves after the template was saved. The start loop in SessionView pins each
        // template skill by its recorded hash, so the new session must read the old body.
        runtime.block_on(index.ingest(&snapshot(NEW))).unwrap();
        let started = session_start(project.clone(), Some(saved.name.clone()), None).unwrap();
        for skill in &saved.skills {
            session_skill_pin(
                project.clone(),
                started.id.clone(),
                skill.skill_id.clone(),
                Some(skill.body_sha.clone()),
            )
            .unwrap();
        }

        let pins = session_skill_pins(project.clone(), started.id.clone()).unwrap();
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].body_sha, old_sha);
        assert!(pins[0].changed);
        let review = session_skill_review(project, started.id, ID.to_string()).unwrap();
        assert_eq!(review.pinned_body.as_deref(), Some(OLD));
        assert_eq!(review.current_body.as_deref(), Some(NEW));

        match previous {
            Some(value) => unsafe { std::env::set_var("RIGEL_INDEX_DIR", value) },
            None => unsafe { std::env::remove_var("RIGEL_INDEX_DIR") },
        }
    }
}

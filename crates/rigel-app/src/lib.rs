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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rigel_core::agent::approval::{
    ApprovalBus, MailApprover, MailDecision, MailRequest, PendingApproval,
};
pub use rigel_core::agent::catalog::ModelOption;
use rigel_core::agent::{Cancels, Coordinator, DEFAULT_MAX_STEPS, fixed_context_chars};
use rigel_core::agent::{catalog, providers};
use rigel_core::{
    Agent, AgentEvent, AppliedReview, ApprovalScope, Attention, AttributedHunk, Bookmark,
    BudgetService, ClaimConflict, ClaimService, CommandApprover, CommandDecision, CommandRequest,
    ConflictWatcher, EventSink, HunkComment, HunkComments, InterruptSink, MemoryFile,
    MemoryService, MergeReview, MergeReviews, Model, POLL_INTERVAL, Patch, PatchPipeline,
    PlanApprover, PlanDecision, PlanRequest, Provenance, ResumeOutcome, Scopes, Session,
    SessionService, SessionTemplate, SessionTemplates, SkillDetail, SkillIndex, SkillsError,
    SnapshotService, StopAt, Store, SubagentPolicy, SyncPolicy, TeamAgent, Tools, Turn, TurnQuote,
    TurnTranscript, UnverifiedChange, Usage, WorktreeService, WriteMode, apply_review, credentials,
    diagnostics, skills::search::SearchHit,
};
use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, State};

mod claude;
mod terminal;
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

/// The model a turn will actually ask for. Settings keep one model across a provider switch, so
/// an id another provider owns is never sent; the provider's own default is the honest fallback,
/// and a provider with no default answers `""` so the turn says "pick a model" instead of
/// sending an empty id.
fn configured_model(settings: &Settings, provider: &str) -> String {
    match providers::find(provider) {
        Some(spec) => providers::configured_model(spec, &settings.model),
        // `settings_set` refuses an unknown provider, so this is a settings file edited by hand;
        // the saved model is the honest answer there.
        None => settings.model.trim().to_string(),
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub theme: String,
    pub default_budget_cents: i64,
    pub unattended_default: bool,
    /// `omniroute` (local router), `openrouter`, or `gemini`. Old settings files gain the
    /// default.
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
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
    /// D159: the window's scale, one of 90/100/110/125. Applied to the shell, not the terminal's
    /// own font, so the PTY keeps its measured cell size.
    #[serde(default = "default_ui_scale")]
    pub ui_scale: u16,
    /// D159: tighter rows in the transcript, list and diff. Layout only; no content is hidden.
    #[serde(default)]
    pub compact_density: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            default_budget_cents: 500,
            unattended_default: false,
            provider: default_provider(),
            model: default_model(),
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
            ui_scale: default_ui_scale(),
            compact_density: false,
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
            key_present: credentials::present_in(&store, spec.id),
        })
        .collect())
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnReply {
    pub turn: Turn,
    pub budget_cents: Option<i64>,
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
        if !query.trim().is_empty() {
            if let Ok(index) = SkillIndex::open(&index_dir()).await {
                if let Ok(hits) = index.search(&query, 6).await {
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
            }
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
    effort: Option<String>,
) -> Result<TurnReply, String> {
    let root = PathBuf::from(&project);
    let service = tauri::async_runtime::block_on(SessionService::open(&root)).map_err(message)?;
    let settings = load_settings();
    let provider = settings.provider.trim().to_lowercase();

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
    let model_name = wire_model(
        &preset
            .map(|preset| preset.model.to_string())
            .or(model)
            .or_else(|| agent_row.model.clone())
            .unwrap_or_else(|| configured_model(&settings, &provider)),
        &effort,
        &provider,
    );

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
            },
        )?;
        return Ok(TurnReply {
            turn,
            budget_cents: cap,
        });
    }

    // A provider with no default (its roster is read live by the picker) refuses an empty model
    // id: the provider's own 400 would say less than this does.
    if model_name.trim().is_empty() {
        return Err(format!(
            "no model chosen for `{provider}`; pick one in the model picker"
        ));
    }

    // One row of the adapter table says where the turn goes: the key, the base URL and the
    // request profile all come from it. A local router that is down is a process to launch, not
    // a configuration error, and `build` starts it before it sends anything.
    let model: Arc<dyn Model> = tauri::async_runtime::block_on(providers::build(
        &provider,
        &credentials::KeychainCredentialStore,
    ))?;

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
        app: app.clone(),
        bus: ApprovalBus::new(index_dir()),
        session_id: session.clone(),
        agent_id: agent.clone(),
        turn_id: turn.id.clone(),
        project_root: root.clone(),
    }))
    .with_plan_approval(Arc::new(WindowApprover {
        app: app.clone(),
        bus: ApprovalBus::new(index_dir()),
        session_id: session.clone(),
        agent_id: agent.clone(),
        turn_id: turn.id.clone(),
        project_root: root.clone(),
    }))
    .with_mail_approval(Arc::new(WindowApprover {
        app: app.clone(),
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
        subagents: SubagentPolicy::default(),
        depth: 0,
        parent_cancel: None,
        worktree_dir: Some(index_dir()),
        chat_parent: None,
    };

    let returned = turn.clone();
    let session_id = session;
    let agent_id = agent;
    let prompt = prompt.clone();
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
        })
        .map_err(|error| format!("could not start the turn thread: {error}"))?;

    Ok(TurnReply {
        turn: returned,
        budget_cents: cap,
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
    app: tauri::AppHandle,
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
                "deliberation": request.deliberation.as_ref().map(|round| {
                    serde_json::json!({
                        "note": round.note,
                        "positions": round.positions.iter().map(|position| {
                            serde_json::json!({ "stance": position.stance, "text": position.text })
                        }).collect::<Vec<_>>(),
                    })
                }),
            }));
        // Nudge the card immediately; it also polls, so a missed event is not a lost question.
        let _ = self.app.emit("rigel://approval", &pending);
        match self
            .bus
            .ask(pending, std::time::Duration::from_secs(600))
            .await
        {
            Ok(scope) => CommandDecision::Allow(scope),
            Err(_) => CommandDecision::Deny,
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
        let _ = self.app.emit("rigel://approval", &pending);
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
        let _ = self.app.emit("rigel://approval", &pending);
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

/// Tell the watcher which project to follow. Called when a session is opened.
#[tauri::command]
fn watch_project(project: String) {
    if let Ok(mut watched) = WATCHED_ROOT.lock() {
        *watched = Some(PathBuf::from(project));
    }
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
    effort: Option<String>,
) -> Result<TurnQuote, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let settings = load_settings();
        let provider = settings.provider.trim().to_lowercase();
        let effort = effort.unwrap_or_else(|| settings.effort.clone());
        let model_name = wire_model(
            &model.unwrap_or_else(|| configured_model(&settings, &provider)),
            &effort,
            &provider,
        );
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
        MemoryService::new(Path::new(&project), service.pool().clone())
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

/// Store a key from the settings form. The renderer sends it once, gets a boolean back, and
/// clears its field; no command ever returns the secret.
#[tauri::command]
fn key_set(provider: String, value: String) -> Result<KeyStatus, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("paste a key first".to_string());
    }
    credentials::set(&provider, value).map_err(message)?;
    Ok(current_key_status())
}

#[tauri::command]
fn key_delete(provider: String) -> Result<KeyStatus, String> {
    credentials::delete(&provider).map_err(message)?;
    Ok(current_key_status())
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

/// The model picker's rows. The catalogue itself lives in the core (`agent::catalog`) so the
/// window and `rigel models` cannot disagree about what a provider offers.
#[tauri::command]
fn models_list() -> Result<Vec<ModelOption>, String> {
    let provider = load_settings().provider.trim().to_lowercase();
    tauri::async_runtime::block_on(catalog::roster(
        &provider,
        &credentials::KeychainCredentialStore,
    ))
}

/// Effort is its own setting (D28). OmniRoute happens to encode the same value in the model
/// id, so when a picked id already carries an effort suffix, keep the two consistent on the
/// wire rather than sending a request that contradicts itself.
fn wire_model(model: &str, effort: &str, provider: &str) -> String {
    if provider != "omniroute" {
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

/// A user message over the vendor websocket, exactly like the browser does it.
#[tauri::command]
fn chat_send(channel: String, text: String) -> Result<(), String> {
    tauri::async_runtime::block_on(rigel_core::agent::agentchat::send(&text, &channel))
}

/// Start the room's agents on a hidden console — they work, nothing opens on screen.
/// Returns the labels launched; an empty list means every agent was already in the room.
#[tauri::command]
fn chat_start_agents(project: Option<String>) -> Result<Vec<String>, String> {
    tauri::async_runtime::block_on(rigel_core::agent::agentchat::start_agents(
        project.as_deref(),
    ))
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
fn chat_harnesses(project: Option<String>) -> Vec<rigel_core::agent::agentchat::ChatHarness> {
    rigel_core::agent::agentchat::configured(project.as_deref())
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

/// Tighten a composer draft before it is sent, on the cheap subagent model. It returns
/// questions and a proposed brief and never sends anything: the window shows both as a review
/// card and the send button stays the user's. This call is outside any session budget, so the
/// card shows what it cost on its own.
#[tauri::command]
fn prompt_refine(
    project: Option<String>,
    request: rigel_core::agent::RefineRequest,
) -> Result<rigel_core::agent::Refinement, String> {
    let settings = load_settings();
    let provider = settings.provider.trim().to_lowercase();
    // Use the model the session already uses, on the same wire shape. The old hard-coded
    // subagent model was often an OpenAI id on an OmniRoute install with no OpenAI credentials,
    // so every send opened the review card with a 401 before the user could reach the agent.
    let model_name = wire_model(
        &configured_model(&settings, &provider),
        &settings.effort,
        &provider,
    );
    if model_name.trim().is_empty() {
        return Err(format!(
            "no model chosen for `{provider}`; pick one in the model picker"
        ));
    }
    let model: Arc<dyn Model> = tauri::async_runtime::block_on(providers::build(
        &provider,
        &credentials::KeychainCredentialStore,
    ))?;

    let refiner = rigel_core::agent::PromptRefiner::new(model, model_name);
    let root = project.map(PathBuf::from);
    tauri::async_runtime::block_on(async move {
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            refiner.refine(&request, root.as_deref()),
        )
        .await
        .map_err(|_| "the refiner took too long; send the draft as it is".to_string())?
    })
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
    if !valid_effort(&settings.effort) {
        return Err(format!(
            "unknown effort `{}`; use low, medium or high",
            settings.effort
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

    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(message)?;
    }
    let text = toml::to_string_pretty(&settings).map_err(message)?;
    fs::write(&path, text).map_err(message)?;
    Ok(settings)
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
    if let Some(url) = manifest_url.as_deref() {
        if !url.trim().is_empty() && !url.starts_with("https://") {
            return Err("the update manifest must be an https URL".to_string());
        }
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
                index.refresh_stale().await
            });
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
}

/// A close request hides the window rather than quitting while the tray exists. The flag flips
/// only when the tray was built, so a platform that refuses a tray keeps a working close button
/// instead of stranding a hidden process with no way back.
static TRAY_READY: AtomicBool = AtomicBool::new(false);
/// Set by the tray's Quit item and by an update that needs the executable replaced. A close
/// request is allowed through once this is true, so shutdown is never trapped by the hide.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Quit through the same path the tray uses, so an updater-triggered exit is not swallowed by
/// the close-to-tray handler.
fn quit(app: &tauri::AppHandle) {
    SHUTTING_DOWN.store(true, Ordering::SeqCst);
    app.exit(0);
}

/// The tray is the way back to a hidden window and the only way to stop the agents while the
/// window is closed. Its menu is deliberately two items: reopen, or really quit.
fn install_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Rigel", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Rigel", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id("rigel")
        .tooltip("Rigel — closing the window keeps agents running")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
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
            warm_router();
            spawn_conflict_watcher(app.handle().clone());
            Ok(())
        })
        .manage(AppRuntime {
            cancels: Arc::new(Cancels::new()),
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if TRAY_READY.load(Ordering::SeqCst) && !SHUTTING_DOWN.load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.hide();
                }
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
            session_archive,
            bookmark_list,
            bookmark_toggle,
            patch_provenance,
            template_list,
            template_save,
            template_delete,
            attention_list,
            session_get,
            session_transcript,
            search_all,
            session_resume,
            diagnostics_export,
            session_export,
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
            approval_pending,
            approval_answer,
            watch_project,
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
            plan_get,
            plan_set,
            role_presets,
            role_apply,
            spend_report,
            task_cap_set,
            harness_probe,
            providers_list,
            servers_start,
            omniroute_status,
            omniroute_start,
            models_list,
            chat_status,
            chat_start,
            chat_channels,
            chat_agents,
            chat_messages,
            chat_send,
            chat_start_agents,
            chat_launched,
            chat_harnesses,
            chat_start_agent,
            chat_stop_agent,
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
            usage_summary,
            skills_search,
            skills_count,
            skills_load,
            skills_status,
            skills_index,
            skills_refresh
        ])
        .build(tauri::generate_context!())
        .expect("rigel: failed to build the window")
        .run(|_, event| {
            // The agents are this app's children; without this, closing the window leaves hidden
            // wrappers holding room slots that the next launch cannot clean up.
            if matches!(event, tauri::RunEvent::Exit) {
                rigel_core::agent::agentchat::stop_all();
                terminal::close_all();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

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

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rigel_core::agent::approval::{ApprovalBus, PendingApproval};
use rigel_core::agent::{Cancels, Coordinator, DEFAULT_MAX_STEPS, fixed_context_chars};
use rigel_core::{
    Agent, AgentEvent, AppliedReview, ApprovalScope, AttributedHunk, BudgetService, ClaimConflict,
    ClaimService, CommandApprover, CommandDecision, CommandRequest, ConflictWatcher, EventSink,
    HunkComment, HunkComments, InterruptSink, MemoryFile, MemoryService, MergeReview, MergeReviews,
    Model, OpenRouterModel, POLL_INTERVAL, Patch, PatchPipeline, PlanApprover, PlanDecision,
    PlanRequest, ResumeOutcome, Scopes, Session, SessionService, SkillDetail, SkillIndex,
    SkillsError, SnapshotService, StopAt, Store, SubagentPolicy, SyncPolicy, TeamAgent, Tools,
    Turn, TurnQuote, TurnTranscript, UnverifiedChange, Usage, WorktreeService, WriteMode,
    apply_review, credentials, diagnostics, skills::search::SearchHit,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

mod claude;
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

/// The model a turn will actually ask for. Settings keep one model across a provider switch,
/// so an OmniRoute id left behind by `provider = "gemini"` would be rejected by Google; the
/// provider's own default is the honest fallback.
fn configured_model(settings: &Settings, provider: &str) -> String {
    if provider == "gemini" && !rigel_core::agent::gemini::is_model(&settings.model) {
        return rigel_core::agent::gemini::DEFAULT_MODEL.to_string();
    }
    settings.model.clone()
}

fn default_provider() -> String {
    "omniroute".to_string()
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
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyStatus {
    pub openrouter: bool,
    pub omniroute: bool,
    pub openai: bool,
    pub anthropic: bool,
    pub cursor: bool,
    pub gemini: bool,
}

fn current_key_status() -> KeyStatus {
    KeyStatus {
        openrouter: credentials::present(credentials::OPENROUTER),
        omniroute: credentials::present(credentials::OMNIROUTE),
        openai: credentials::present(credentials::OPENAI),
        anthropic: credentials::present(credentials::ANTHROPIC),
        cursor: credentials::present(credentials::CURSOR),
        gemini: credentials::present(credentials::GEMINI),
    }
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

    // A local router that is down is a process to launch, not a configuration error: the
    // first turn after a reboot starts OmniRoute before it sends anything.
    let model: Arc<dyn Model> = match provider.as_str() {
        "openrouter" => {
            let key = credentials::get(credentials::OPENROUTER)
                .map_err(|error| format!("{error}. Add an OpenRouter key in Settings."))?;
            Arc::new(OpenRouterModel::new(key))
        }
        "gemini" => {
            let key = credentials::get(credentials::GEMINI)
                .map_err(|error| format!("{error}. Add a Gemini key in Settings."))?;
            Arc::new(rigel_core::agent::gemini::model(key))
        }
        "omniroute" => {
            let key = credentials::get(credentials::OMNIROUTE)
                .map_err(|error| format!("{error}. Add an OmniRoute key in Settings."))?;
            tauri::async_runtime::block_on(rigel_core::agent::omniroute::ensure_server())
                .map_err(|error| format!("{error} Start it from Settings."))?;
            Arc::new(rigel_core::agent::omniroute::model(key))
        }
        other => {
            return Err(format!(
                "unknown provider `{other}`; use omniroute, openrouter or gemini"
            ));
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
        app: app.clone(),
        bus: ApprovalBus::new(index_dir()),
    }))
    .with_plan_approval(Arc::new(WindowApprover {
        app: app.clone(),
        bus: ApprovalBus::new(index_dir()),
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
        max_steps: DEFAULT_MAX_STEPS,
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
}

#[async_trait::async_trait]
impl CommandApprover for WindowApprover {
    async fn approve(&self, request: &CommandRequest) -> CommandDecision {
        let key = format!("command:{}", request.command);
        let pending = PendingApproval::now("command", &key, &request.command).with_detail(
            serde_json::json!({
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
            }),
        );
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

/// Every open question, oldest first. The card polls this after a turn starts.
#[tauri::command]
fn approval_pending() -> Vec<PendingApproval> {
    ApprovalBus::new(index_dir()).pending()
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

/// The model picker's catalogue, read through the same provider the agent uses. The key is
/// read in Rust and only becomes a bearer header; the window never sees it.
async fn fetch_models(provider: &str) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(message)?;

    let response = match provider {
        "openrouter" => {
            let mut request = client.get("https://openrouter.ai/api/v1/models");
            if let Ok(key) = credentials::get(credentials::OPENROUTER) {
                request = request.bearer_auth(key);
            }
            request.send().await
        }
        "omniroute" => {
            let key = credentials::get(credentials::OMNIROUTE)
                .map_err(|error| format!("{error}. Add an OmniRoute key in Settings."))?;
            rigel_core::agent::omniroute::ensure_server().await?;
            let base = rigel_core::agent::omniroute::base_url();
            client
                .get(format!("{base}/models"))
                .bearer_auth(key)
                .send()
                .await
        }
        other => return Err(format!("unknown provider `{other}`")),
    }
    .map_err(|error| format!("could not read the model catalogue: {error}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "the model catalogue answered {}",
            response.status()
        ));
    }

    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("the model catalogue returned unreadable data: {error}"))?;
    let mut models: Vec<String> = value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("id").and_then(serde_json::Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    models.dedup();
    if models.is_empty() {
        return Err("the provider returned no models".to_string());
    }
    Ok(models)
}

#[tauri::command]
fn models_list() -> Result<Vec<String>, String> {
    let provider = load_settings().provider;
    tauri::async_runtime::block_on(fetch_models(&provider))
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

/// Boot the vendor chat server when it is down. Spawning is how Rigel earns the session
/// token, so "already running externally" stays unauthenticated (`authed: false`).
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
    let (model, model_name): (Arc<dyn Model>, String) = match provider.as_str() {
        "openrouter" => {
            let key = credentials::get(credentials::OPENROUTER)
                .map_err(|error| format!("{error}. Add an OpenRouter key in Settings."))?;
            (
                Arc::new(OpenRouterModel::new(key)),
                rigel_core::agent::DEFAULT_SUBAGENT_MODEL.to_string(),
            )
        }
        "omniroute" => {
            let key = credentials::get(credentials::OMNIROUTE)
                .map_err(|error| format!("{error}. Add an OmniRoute key in Settings."))?;
            tauri::async_runtime::block_on(rigel_core::agent::omniroute::ensure_server())
                .map_err(|error| format!("{error} Start it from Settings."))?;
            (
                Arc::new(rigel_core::agent::omniroute::model(key)),
                rigel_core::agent::DEFAULT_SUBAGENT_MODEL.to_string(),
            )
        }
        "gemini" => {
            let key = credentials::get(credentials::GEMINI)
                .map_err(|error| format!("{error}. Add a Gemini key in Settings."))?;
            (
                Arc::new(rigel_core::agent::gemini::model(key)),
                "gemini-2.5-flash-lite".to_string(),
            )
        }
        other => {
            return Err(format!(
                "unknown provider `{other}`; use omniroute, openrouter or gemini"
            ));
        }
    };

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
fn settings_set(settings: Settings) -> Result<Settings, String> {
    if !matches!(
        settings.provider.as_str(),
        "omniroute" | "openrouter" | "gemini"
    ) {
        return Err(format!(
            "unknown provider `{}`; use omniroute, openrouter or gemini",
            settings.provider
        ));
    }
    if !valid_effort(&settings.effort) {
        return Err(format!(
            "unknown effort `{}`; use low, medium or high",
            settings.effort
        ));
    }
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(message)?;
    }
    let text = toml::to_string_pretty(&settings).map_err(message)?;
    fs::write(&path, text).map_err(message)?;
    Ok(settings)
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

/// Hand the bundled chat sidecar to the core: `$RESOURCE/sidecar` in an install, the repository's
/// `resources/sidecar` in a dev build. The source is copied into app data on first launch, so the
/// room can write while `python.exe` stays in the read-only install directory.
///
/// Absent is not an error: a developer without the sidecar keeps the `vendor/agentchattr` fallback,
/// and the Chat tab explains that case.
fn install_chat_sidecar(app: &tauri::AppHandle) -> Result<String, String> {
    let packaged = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("sidecar"))
        .filter(|dir| dir.join("agentchattr/run.py").is_file());
    let source =
        packaged.unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/sidecar"));
    if !source.join("agentchattr/run.py").is_file() {
        return Ok(String::new());
    }
    rigel_core::agent::agentchat::install_sidecar(source, index_dir().join("agentchat"))
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

/// Every local service a session can need, started as the window opens and again once the
/// remembered project is restored (the chat checkout sits beside it). The window calls this
/// once; neither start blocks the launch, and a service that cannot start is a status the
/// relevant tab explains rather than an error at boot.
#[tauri::command]
fn servers_start(project: Option<String>) {
    warm_router();
    warm_chat(project);
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
            warm_router();
            spawn_conflict_watcher(app.handle().clone());
            Ok(())
        })
        .manage(AppRuntime {
            cancels: Arc::new(Cancels::new()),
        })
        .invoke_handler(tauri::generate_handler![
            project_open,
            project_create,
            project_workspace_root,
            session_start,
            session_list,
            session_get,
            session_transcript,
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
            prompt_refine,
            settings_get,
            settings_set,
            usage_summary,
            skills_search,
            skills_count,
            skills_load,
            skills_status,
            skills_index
        ])
        .build(tauri::generate_context!())
        .expect("rigel: failed to build the window")
        .run(|_, event| {
            // The agents are this app's children; without this, closing the window leaves hidden
            // wrappers holding room slots that the next launch cannot clean up.
            if matches!(event, tauri::RunEvent::Exit) {
                rigel_core::agent::agentchat::stop_all();
            }
        });
}

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

use rigel_core::agent::{Cancels, Coordinator, DEFAULT_MAX_STEPS, fixed_context_chars};
use rigel_core::{
    Agent, AgentEvent, BudgetService, EventSink, InterruptSink, MemoryFile, MemoryService, Model,
    OpenRouterModel, Patch, PatchPipeline, ResumeOutcome, Session, SessionService, SkillDetail,
    SkillIndex, SkillsError, StopAt, Store, SubagentPolicy, SyncPolicy, Turn, TurnQuote,
    TurnTranscript, Usage, WriteMode, credentials, skills::search::SearchHit,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

#[derive(Debug, Clone, Serialize)]
pub struct ProjectInfo {
    pub root: String,
    pub name: String,
    pub db: String,
}

fn default_model() -> String {
    rigel_core::agent::omniroute::DEFAULT_MODEL.to_string()
}

fn default_provider() -> String {
    "omniroute".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub theme: String,
    pub default_budget_cents: i64,
    pub unattended_default: bool,
    /// `omniroute` (local router) or `openrouter`. Old settings files gain the default.
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            default_budget_cents: 500,
            unattended_default: false,
            provider: default_provider(),
            model: default_model(),
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
}

fn current_key_status() -> KeyStatus {
    KeyStatus {
        openrouter: credentials::present(credentials::OPENROUTER),
        omniroute: credentials::present(credentials::OMNIROUTE),
        openai: credentials::present(credentials::OPENAI),
        anthropic: credentials::present(credentials::ANTHROPIC),
        cursor: credentials::present(credentials::CURSOR),
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
#[tauri::command]
fn skills_search(query: String, limit: usize) -> Result<Vec<SearchHit>, String> {
    tauri::async_runtime::block_on(async move {
        SkillIndex::open(&index_dir())
            .await
            .map_err(message)?
            .search(&query, limit)
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
    if harness != "native" {
        return Err(format!(
            "{harness} adapters arrive in Phase 4; the native agent is available now"
        ));
    }
    let model = model.unwrap_or_else(|| load_settings().model);
    tauri::async_runtime::block_on(async move {
        SessionService::open(Path::new(&project))
            .await
            .map_err(message)?
            .register_agent_with(&session, "native", role.as_deref(), Some(&model))
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
#[tauri::command]
fn turn_send(
    app: tauri::AppHandle,
    state: State<'_, AppRuntime>,
    project: String,
    session: String,
    agent: String,
    prompt: String,
    model: Option<String>,
) -> Result<TurnReply, String> {
    let root = PathBuf::from(&project);
    let service = tauri::async_runtime::block_on(SessionService::open(&root)).map_err(message)?;
    let settings = load_settings();
    let model_name = model.unwrap_or(settings.model);

    // A local router that is down is a process to launch, not a configuration error: the
    // first turn after a reboot starts OmniRoute before it sends anything.
    let model: Arc<dyn Model> = match settings.provider.trim().to_lowercase().as_str() {
        "openrouter" => {
            let key = credentials::get(credentials::OPENROUTER)
                .map_err(|error| format!("{error}. Add an OpenRouter key in Settings."))?;
            Arc::new(OpenRouterModel::new(key))
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
                "unknown provider `{other}`; use omniroute or openrouter"
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

    let tools = rigel_core::Tools::new(
        root.clone(),
        service.clone(),
        budget.clone(),
        index_dir(),
        StopAt::Never,
    );
    let coordinator = Coordinator {
        session: service.clone(),
        budget,
        tools,
        model,
        sink: Arc::new(WindowSink { app: app.clone() }),
        cancels,
        project_root: root,
        model_name,
        max_steps: DEFAULT_MAX_STEPS,
        subagents: SubagentPolicy::default(),
        depth: 0,
        parent_cancel: None,
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
) -> Result<TurnQuote, String> {
    tauri::async_runtime::block_on(async move {
        let service = SessionService::open(Path::new(&project))
            .await
            .map_err(message)?;
        let model_name = model.unwrap_or_else(|| load_settings().model);
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

/* -------------------------------- settings ------------------------------- */

#[tauri::command]
fn settings_get() -> Result<Settings, String> {
    Ok(load_settings())
}

/// Write settings, creating the app data directory if this is a first run.
#[tauri::command]
fn settings_set(settings: Settings) -> Result<Settings, String> {
    if settings.provider != "omniroute" && settings.provider != "openrouter" {
        return Err(format!(
            "unknown provider `{}`; use omniroute or openrouter",
            settings.provider
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            warm_router();
            Ok(())
        })
        .manage(AppRuntime {
            cancels: Arc::new(Cancels::new()),
        })
        .invoke_handler(tauri::generate_handler![
            project_open,
            session_start,
            session_list,
            session_get,
            session_transcript,
            session_resume,
            agent_list,
            agent_spawn,
            agent_stop,
            turn_send,
            turn_cancel,
            turn_estimate,
            budget_get,
            budget_set,
            budget_raise,
            patch_list,
            patch_revert,
            memory_read,
            memory_write,
            key_status,
            key_set,
            key_delete,
            omniroute_status,
            omniroute_start,
            settings_get,
            settings_set,
            usage_summary,
            skills_search,
            skills_load,
            skills_status,
            skills_index
        ])
        .run(tauri::generate_context!())
        .expect("rigel: failed to start the window");
}

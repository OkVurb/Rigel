//! The Tauri shell: a thin IPC layer over `rigel-core`.
//!
//! Commands are deliberately stateless — each one opens the project database it needs. That
//! costs a connection open, and buys the property that no renderer crash can desynchronise
//! app state (handoff §3.2, S2). Phase 3 introduces a long-lived service once there is a
//! session to keep warm.
//!
//! Two constraints shape the command signatures:
//! * they are private, because `#[tauri::command]` exports a macro for `pub` items and that
//!   collides with the crate-root macro namespace;
//! * they are synchronous and drive the async core with `block_on`, because Tauri's async
//!   command wrapper cannot express sqlx's higher-ranked `Executor` lifetimes.

use std::fs;
use std::path::{Path, PathBuf};

use rigel_core::{
    Session, SessionService, SkillDetail, SkillIndex, SkillsError, Store, SyncPolicy,
    skills::search::SearchHit,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct ProjectInfo {
    pub root: String,
    pub name: String,
    pub db: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub theme: String,
    pub default_budget_cents: i64,
    pub unattended_default: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            default_budget_cents: 500,
            unattended_default: false,
        }
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

/// Read settings from `%APPDATA%\Rigel\settings.toml`, or the defaults.
#[tauri::command]
fn settings_get() -> Result<Settings, String> {
    let path = settings_path();
    if !path.exists() {
        return Ok(Settings::default());
    }
    let text = fs::read_to_string(&path).map_err(message)?;
    toml::from_str(&text).map_err(message)
}

/// Write settings, creating the app data directory if this is a first run.
#[tauri::command]
fn settings_set(settings: Settings) -> Result<Settings, String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(message)?;
    }
    let text = toml::to_string_pretty(&settings).map_err(message)?;
    fs::write(&path, text).map_err(message)?;
    Ok(settings)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            project_open,
            session_start,
            session_list,
            settings_get,
            settings_set,
            skills_search,
            skills_load,
            skills_status,
            skills_index
        ])
        .run(tauri::generate_context!())
        .expect("rigel: failed to start the window");
}

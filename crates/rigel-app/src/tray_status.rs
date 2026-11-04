//! The data behind the themed tray panel.
//!
//! Agent and spend rows are project state. CPU and memory are sampled from Rigel's own process
//! tree, so a tray that says "Rigel + agents" does not quietly include the whole desktop. Windows
//! does not expose per-process wattage, so power is a load band rather than a made-up number.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rigel_core::SessionService;
use serde::Serialize;
use sqlx::SqlitePool;
use sysinfo::System;

static SYSTEM: Mutex<Option<System>> = Mutex::new(None);
static PROJECT_POOL: Mutex<Option<(PathBuf, SqlitePool)>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize)]
pub struct TrayAgent {
    pub id: String,
    pub name: String,
    pub harness: String,
    pub model: Option<String>,
    pub session: String,
    pub status: String,
    pub spend_micros: i64,
    /// `session` for an agent row, `chat` for a harness Rigel launched for the room.
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraySnapshot {
    pub project: Option<String>,
    pub project_name: Option<String>,
    pub sessions: usize,
    pub agents_running: usize,
    /// Running agents not included in the list because the tray caps the rows it draws.
    pub agents_hidden: usize,
    pub spend_micros: i64,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub power: String,
    pub power_note: String,
    pub agents: Vec<TrayAgent>,
    pub updated_at: i64,
}

/// Build the first CPU baseline in the background so the first panel opens with a useful value.
pub fn warm() {
    let _ = sample_processes();
}

fn process_tree(system: &System) -> HashSet<sysinfo::Pid> {
    let mut tree = HashSet::new();
    if let Ok(current) = sysinfo::get_current_pid() {
        tree.insert(current);
    }
    loop {
        let mut added = false;
        for (pid, process) in system.processes() {
            if let Some(parent) = process.parent()
                && tree.contains(&parent)
                && tree.insert(*pid)
            {
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    tree
}

fn sample_processes() -> (f32, u64) {
    let Ok(mut guard) = SYSTEM.lock() else {
        return (0.0, 0);
    };
    let system = guard.get_or_insert_with(System::new_all);
    system.refresh_all();

    let mut cpu = 0.0;
    let mut memory = 0;
    for pid in process_tree(system) {
        if let Some(process) = system.process(pid) {
            cpu += process.cpu_usage();
            memory += process.memory();
        }
    }
    (cpu, memory)
}

fn power_band(cpu: f32) -> (&'static str, &'static str) {
    if cpu < 1.0 {
        ("idle", "near idle")
    } else if cpu < 15.0 {
        ("low", "light load")
    } else if cpu < 45.0 {
        ("moderate", "working")
    } else {
        ("high", "heavy load")
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Opening a project runs its migrations and WAL check. The tray polls, so keep the pool for the
/// current root instead of doing that every two seconds while a session may be writing.
async fn project_pool(root: &Path) -> Result<SqlitePool, String> {
    if let Ok(guard) = PROJECT_POOL.lock()
        && let Some((cached_root, pool)) = guard.as_ref()
        && cached_root == root
    {
        return Ok(pool.clone());
    }

    let service = SessionService::open(root)
        .await
        .map_err(|error| error.to_string())?;
    let pool = service.pool().clone();
    if let Ok(mut guard) = PROJECT_POOL.lock() {
        *guard = Some((root.to_path_buf(), pool.clone()));
    }
    Ok(pool)
}

/// One tray refresh. A project that has never been opened still returns a valid snapshot with
/// zero rows, so the tray can render before the first session exists.
pub async fn snapshot() -> Result<TraySnapshot, String> {
    let project = crate::current_project_root();
    let project_name = project.as_ref().and_then(|root| {
        root.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.is_empty())
    });
    let mut sessions = 0;
    let mut spend_micros = 0;
    let mut agents_running = 0;
    let mut agents = Vec::new();

    if let Some(root) = project.as_ref() {
        let pool = project_pool(root).await?;
        sessions = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .map_err(|error| error.to_string())? as usize;
        spend_micros =
            sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(spend_micros), 0) FROM sessions")
                .fetch_one(&pool)
                .await
                .map_err(|error| error.to_string())?;
        agents_running =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agents WHERE status = 'running'")
                .fetch_one(&pool)
                .await
                .map_err(|error| error.to_string())? as usize;

        type RunningAgentRow = (
            String,
            String,
            Option<String>,
            Option<String>,
            String,
            i64,
            String,
        );
        let rows: Vec<RunningAgentRow> = sqlx::query_as(
            "SELECT a.id, a.harness, a.role, a.model, a.status, a.spend_micros,
                    COALESCE(s.title, s.id)
               FROM agents a
               JOIN sessions s ON s.id = a.session_id
              WHERE a.status = 'running'
              ORDER BY a.started_at DESC
              LIMIT 8",
        )
        .fetch_all(&pool)
        .await
        .map_err(|error| error.to_string())?;

        for (id, harness, role, model, status, spend_micros, session) in rows {
            let name = role
                .clone()
                .or_else(|| model.clone())
                .unwrap_or_else(|| harness.clone());
            agents.push(TrayAgent {
                id,
                name,
                harness,
                model,
                session,
                status,
                spend_micros,
                source: "session".to_string(),
            });
        }
    }

    let chat = rigel_core::agent::agentchat::launched();
    agents_running += chat.len();
    for process in chat.iter().take(4) {
        agents.push(TrayAgent {
            id: format!("chat:{}", process.pid),
            name: process.label.clone(),
            harness: "chat".to_string(),
            model: None,
            session: "Chat room".to_string(),
            status: "running".to_string(),
            spend_micros: 0,
            source: "chat".to_string(),
        });
    }

    let (cpu_percent, memory_bytes) = sample_processes();
    let (power, _) = power_band(cpu_percent);
    Ok(TraySnapshot {
        project: project.map(|root| root.to_string_lossy().into_owned()),
        project_name,
        sessions,
        agents_running,
        agents_hidden: agents_running.saturating_sub(agents.len()),
        spend_micros,
        cpu_percent,
        memory_bytes,
        power: power.to_string(),
        power_note: "Relative CPU load band; Windows does not expose per-process watts."
            .to_string(),
        agents,
        updated_at: now_ms(),
    })
}

#[cfg(test)]
mod tests {
    use super::power_band;

    #[test]
    fn power_bands_follow_load() {
        assert_eq!(power_band(0.0), ("idle", "near idle"));
        assert_eq!(power_band(8.0), ("low", "light load"));
        assert_eq!(power_band(30.0), ("moderate", "working"));
        assert_eq!(power_band(80.0), ("high", "heavy load"));
    }
}

//! Sessions: the project-level thread the whole product is built on (D1).
//!
//! A session never "closes"; it goes `active` → `idle` → `interrupted` → `ended`. Every
//! turn, item and checkpoint written here is what makes replay and resume possible, so the
//! service is deliberately the only writer of those tables (I1).

use std::path::Path;

use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::migrate::now_ms;
use crate::storage::{StorageError, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    Idle,
    Interrupted,
    Ended,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Interrupted => "interrupted",
            Self::Ended => "ended",
        }
    }

    pub fn parse(value: &str) -> Result<Self, SessionError> {
        match value {
            "active" => Ok(Self::Active),
            "idle" => Ok(Self::Idle),
            "interrupted" => Ok(Self::Interrupted),
            "ended" => Ok(Self::Ended),
            other => Err(SessionError::InvalidStatus(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub id: String,
    pub project_root: String,
    pub title: Option<String>,
    pub status: SessionStatus,
    pub budget_cents: Option<i64>,
    pub spend_cents: i64,
    pub interventions: i64,
    pub tasks_done: i64,
    pub created_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Agent {
    pub id: String,
    pub session_id: String,
    pub harness: String,
    pub role: Option<String>,
    pub status: String,
    pub started_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Turn {
    pub id: String,
    pub agent_id: String,
    pub seq: i64,
    pub status: String,
    pub prompt: Option<String>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub id: String,
    pub turn_id: String,
    pub seq: i64,
    pub kind: String,
    pub status: String,
    pub body: serde_json::Value,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Checkpoint {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub tree_hash: String,
    pub created_at: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session {0} does not exist")]
    SessionNotFound(String),

    #[error("agent {0} does not exist")]
    AgentNotFound(String),

    #[error("turn {0} does not exist")]
    TurnNotFound(String),

    #[error("invalid status: {0}")]
    InvalidStatus(String),

    #[error("invalid turn status: {0}")]
    InvalidTurnStatus(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Owns every read and write of session state.
pub struct SessionService {
    pool: SqlitePool,
    project_root: String,
}

type SessionRow = (
    String,
    String,
    Option<String>,
    String,
    Option<i64>,
    i64,
    i64,
    i64,
    i64,
    Option<i64>,
);

impl SessionService {
    pub async fn open(root: &Path) -> Result<Self, SessionError> {
        let root = root.to_path_buf();
        let store = Store::open_project(&root).await?;
        Ok(Self::from_store(&store, &root))
    }

    pub fn from_store(store: &Store, root: &Path) -> Self {
        Self {
            pool: store.pool().clone(),
            project_root: root.to_string_lossy().into_owned(),
        }
    }

    pub fn project_root(&self) -> &str {
        &self.project_root
    }

    /// Begin a session. There is no matching "open" — a project accumulates sessions.
    pub async fn start(
        &self,
        title: Option<&str>,
        budget_cents: Option<i64>,
    ) -> Result<Session, SessionError> {
        let id = Uuid::new_v4().to_string();
        let created_at = now_ms();

        sqlx::query(
            "INSERT INTO sessions (id, project_root, title, status, budget_cents, created_at)
             VALUES (?, ?, ?, 'active', ?, ?)",
        )
        .bind(&id)
        .bind(&self.project_root)
        .bind(title)
        .bind(budget_cents)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        self.get(&id).await
    }

    pub async fn get(&self, id: &str) -> Result<Session, SessionError> {
        let row: Option<SessionRow> = sqlx::query_as(
            "SELECT id, project_root, title, status, budget_cents, spend_cents,
                    interventions, tasks_done, created_at, ended_at
             FROM sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => session_from_row(row),
            None => Err(SessionError::SessionNotFound(id.to_string())),
        }
    }

    /// Newest first: the session list is a log, not an alphabet.
    pub async fn list(&self) -> Result<Vec<Session>, SessionError> {
        let rows: Vec<SessionRow> = sqlx::query_as(
            "SELECT id, project_root, title, status, budget_cents, spend_cents,
                    interventions, tasks_done, created_at, ended_at
             FROM sessions ORDER BY created_at DESC, id DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(session_from_row).collect()
    }

    pub async fn end(&self, id: &str) -> Result<Session, SessionError> {
        let ended_at = now_ms();
        let result = sqlx::query("UPDATE sessions SET status = 'ended', ended_at = ? WHERE id = ?")
            .bind(ended_at)
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(SessionError::SessionNotFound(id.to_string()));
        }
        self.get(id).await
    }

    pub async fn set_status(&self, id: &str, status: SessionStatus) -> Result<(), SessionError> {
        let result = sqlx::query("UPDATE sessions SET status = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(SessionError::SessionNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Register an agent in a session. Turns hang off agents, so this comes first.
    pub async fn register_agent(
        &self,
        session_id: &str,
        harness: &str,
        role: Option<&str>,
    ) -> Result<Agent, SessionError> {
        self.get(session_id).await?;

        let id = Uuid::new_v4().to_string();
        let started_at = now_ms();
        sqlx::query(
            "INSERT INTO agents (id, session_id, harness, role, status, started_at)
             VALUES (?, ?, ?, ?, 'idle', ?)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(harness)
        .bind(role)
        .bind(started_at)
        .execute(&self.pool)
        .await?;

        Ok(Agent {
            id,
            session_id: session_id.to_string(),
            harness: harness.to_string(),
            role: role.map(str::to_string),
            status: "idle".to_string(),
            started_at,
        })
    }

    /// Open a turn for an agent. Sequence numbers are per agent, not global.
    pub async fn append_turn(
        &self,
        agent_id: &str,
        prompt: Option<&str>,
    ) -> Result<Turn, SessionError> {
        let exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM agents WHERE id = ?")
            .bind(agent_id)
            .fetch_optional(&self.pool)
            .await?;
        if exists.is_none() {
            return Err(SessionError::AgentNotFound(agent_id.to_string()));
        }

        let (seq,): (i64,) =
            sqlx::query_as("SELECT COALESCE(MAX(seq), 0) + 1 FROM turns WHERE agent_id = ?")
                .bind(agent_id)
                .fetch_one(&self.pool)
                .await?;

        let id = Uuid::new_v4().to_string();
        let started_at = now_ms();
        sqlx::query(
            "INSERT INTO turns (id, agent_id, seq, status, prompt, started_at)
             VALUES (?, ?, ?, 'running', ?, ?)",
        )
        .bind(&id)
        .bind(agent_id)
        .bind(seq)
        .bind(prompt)
        .bind(started_at)
        .execute(&self.pool)
        .await?;

        Ok(Turn {
            id,
            agent_id: agent_id.to_string(),
            seq,
            status: "running".to_string(),
            prompt: prompt.map(str::to_string),
            started_at,
            completed_at: None,
        })
    }

    /// Close a turn. Exactly one of completed/failed/cancelled is allowed (I1).
    pub async fn finish_turn(&self, turn_id: &str, status: &str) -> Result<Turn, SessionError> {
        if !matches!(status, "completed" | "failed" | "cancelled") {
            return Err(SessionError::InvalidTurnStatus(status.to_string()));
        }

        let completed_at = now_ms();
        let result = sqlx::query("UPDATE turns SET status = ?, completed_at = ? WHERE id = ?")
            .bind(status)
            .bind(completed_at)
            .bind(turn_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(SessionError::TurnNotFound(turn_id.to_string()));
        }

        let row: (
            String,
            String,
            i64,
            String,
            Option<String>,
            i64,
            Option<i64>,
        ) = sqlx::query_as(
            "SELECT id, agent_id, seq, status, prompt, started_at, completed_at
             FROM turns WHERE id = ?",
        )
        .bind(turn_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(Turn {
            id: row.0,
            agent_id: row.1,
            seq: row.2,
            status: row.3,
            prompt: row.4,
            started_at: row.5,
            completed_at: row.6,
        })
    }

    /// Append one normalised event. `raw` is stored untouched (I2).
    pub async fn append_item(
        &self,
        turn_id: &str,
        kind: &str,
        status: &str,
        body: &serde_json::Value,
        raw: &serde_json::Value,
    ) -> Result<Item, SessionError> {
        let exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM turns WHERE id = ?")
            .bind(turn_id)
            .fetch_optional(&self.pool)
            .await?;
        if exists.is_none() {
            return Err(SessionError::TurnNotFound(turn_id.to_string()));
        }

        let (seq,): (i64,) =
            sqlx::query_as("SELECT COALESCE(MAX(seq), 0) + 1 FROM items WHERE turn_id = ?")
                .bind(turn_id)
                .fetch_one(&self.pool)
                .await?;

        let id = Uuid::new_v4().to_string();
        let created_at = now_ms();
        let body_json = serde_json::to_string(body)?;
        let raw_json = serde_json::to_string(raw)?;

        sqlx::query(
            "INSERT INTO items (id, turn_id, seq, kind, status, body_json, raw_json, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(turn_id)
        .bind(seq)
        .bind(kind)
        .bind(status)
        .bind(&body_json)
        .bind(&raw_json)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(Item {
            id,
            turn_id: turn_id.to_string(),
            seq,
            kind: kind.to_string(),
            status: status.to_string(),
            body: body.clone(),
            created_at,
        })
    }

    /// Record a checkpoint after a turn. Resume compares `tree_hash` later (I8).
    pub async fn checkpoint(
        &self,
        session_id: &str,
        turn_id: Option<&str>,
        tree_hash: &str,
        journal: &serde_json::Value,
    ) -> Result<Checkpoint, SessionError> {
        self.get(session_id).await?;

        let id = Uuid::new_v4().to_string();
        let created_at = now_ms();
        let journal_json = serde_json::to_string(journal)?;

        sqlx::query(
            "INSERT INTO checkpoints (id, session_id, turn_id, tree_hash, journal_json, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(turn_id)
        .bind(tree_hash)
        .bind(&journal_json)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(Checkpoint {
            id,
            session_id: session_id.to_string(),
            turn_id: turn_id.map(str::to_string),
            tree_hash: tree_hash.to_string(),
            created_at,
        })
    }

    pub async fn items(&self, turn_id: &str) -> Result<Vec<Item>, SessionError> {
        let rows: Vec<(String, String, i64, String, String, String, i64)> = sqlx::query_as(
            "SELECT id, turn_id, seq, kind, status, body_json, created_at
             FROM items WHERE turn_id = ? ORDER BY seq",
        )
        .bind(turn_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(Item {
                    id: row.0,
                    turn_id: row.1,
                    seq: row.2,
                    kind: row.3,
                    status: row.4,
                    body: serde_json::from_str(&row.5)?,
                    created_at: row.6,
                })
            })
            .collect()
    }
}

fn session_from_row(row: SessionRow) -> Result<Session, SessionError> {
    let status = SessionStatus::parse(&row.3)?;
    Ok(Session {
        id: row.0,
        project_root: row.1,
        title: row.2,
        status,
        budget_cents: row.4,
        spend_cents: row.5,
        interventions: row.6,
        tasks_done: row.7,
        created_at: row.8,
        ended_at: row.9,
    })
}

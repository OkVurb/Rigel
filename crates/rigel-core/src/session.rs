//! Sessions: the project-level thread the whole product is built on (D1).
//!
//! A session never "closes"; it goes `active` → `idle` → `interrupted` → `ended`. Every
//! turn, item and checkpoint written here is what makes replay and resume possible, so the
//! service is deliberately the only writer of those tables (I1).

use std::path::Path;

use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::domain::Usage;
use crate::migrate::now_ms;
use crate::storage::{StorageError, Store};

/// `items` row as SQLite returns it: id, turn, seq, kind, status, body, created_at, raw.
type ItemRow = (String, String, i64, String, String, String, i64, String);

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
    /// The accounting value; `spend_cents` is its rounded projection.
    pub spend_micros: i64,
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
    pub model: Option<String>,
    pub status: String,
    pub started_at: i64,
    pub spend_cents: i64,
    pub spend_micros: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Turn {
    pub id: String,
    pub agent_id: String,
    /// Set when this turn was run by a subagent on behalf of another turn. Child turns are
    /// kept out of the top-level transcript and rendered under their parent.
    pub parent_turn_id: Option<String>,
    pub seq: i64,
    pub status: String,
    pub prompt: Option<String>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub usage: Option<Usage>,
    pub stop_reason: Option<String>,
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
    /// Present only in a replay: the untouched provider JSON (I2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

/// One turn and everything it recorded, in order.
#[derive(Debug, Clone, Serialize)]
pub struct TurnTranscript {
    pub turn: Turn,
    pub items: Vec<Item>,
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

/// Owns every read and write of session state. Cloning shares the pool, not the state:
/// SQLite is the state.
#[derive(Clone)]
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

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn path(&self) -> &Path {
        Path::new(&self.project_root)
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
            "SELECT id, project_root, title, status, budget_cents, spend_cents, spend_micros,
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
            "SELECT id, project_root, title, status, budget_cents, spend_cents, spend_micros,
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
        self.register_agent_with(session_id, harness, role, None)
            .await
    }

    pub async fn register_agent_with(
        &self,
        session_id: &str,
        harness: &str,
        role: Option<&str>,
        model: Option<&str>,
    ) -> Result<Agent, SessionError> {
        self.get(session_id).await?;

        let id = Uuid::new_v4().to_string();
        let started_at = now_ms();
        sqlx::query(
            "INSERT INTO agents (id, session_id, harness, role, model, status, started_at)
             VALUES (?, ?, ?, ?, ?, 'idle', ?)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(harness)
        .bind(role)
        .bind(model)
        .bind(started_at)
        .execute(&self.pool)
        .await?;

        self.agent(&id).await
    }

    pub async fn agents(&self, session_id: &str) -> Result<Vec<Agent>, SessionError> {
        let rows: Vec<AgentRow> = sqlx::query_as(
            "SELECT id, session_id, harness, role, model, status, started_at,
                    spend_cents, spend_micros, tokens_in, tokens_out
             FROM agents WHERE session_id = ? ORDER BY started_at, rowid",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(agent_from_row).collect())
    }

    pub async fn agent(&self, id: &str) -> Result<Agent, SessionError> {
        let row: Option<AgentRow> = sqlx::query_as(
            "SELECT id, session_id, harness, role, model, status, started_at,
                    spend_cents, spend_micros, tokens_in, tokens_out
             FROM agents WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(agent_from_row)
            .ok_or_else(|| SessionError::AgentNotFound(id.to_string()))
    }

    pub async fn set_agent_status(&self, id: &str, status: &str) -> Result<(), SessionError> {
        if !matches!(
            status,
            "starting"
                | "idle"
                | "running"
                | "waiting_approval"
                | "interrupted"
                | "failed"
                | "exited"
        ) {
            return Err(SessionError::InvalidStatus(status.to_string()));
        }
        sqlx::query("UPDATE agents SET status = ?, ended_at = CASE WHEN ? IN ('exited','failed') THEN ? ELSE ended_at END WHERE id = ?")
            .bind(status)
            .bind(status)
            .bind(now_ms())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Open a turn for an agent. Sequence numbers are per agent, not global.
    pub async fn append_turn(
        &self,
        agent_id: &str,
        prompt: Option<&str>,
    ) -> Result<Turn, SessionError> {
        self.append_turn_with_parent(agent_id, prompt, None).await
    }

    /// Open a turn that belongs to a parent turn (a subagent's work).
    pub async fn append_turn_with_parent(
        &self,
        agent_id: &str,
        prompt: Option<&str>,
        parent_turn_id: Option<&str>,
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
            "INSERT INTO turns (id, agent_id, seq, status, prompt, started_at, parent_turn_id)
             VALUES (?, ?, ?, 'running', ?, ?, ?)",
        )
        .bind(&id)
        .bind(agent_id)
        .bind(seq)
        .bind(prompt)
        .bind(started_at)
        .bind(parent_turn_id)
        .execute(&self.pool)
        .await?;

        Ok(Turn {
            id,
            agent_id: agent_id.to_string(),
            parent_turn_id: parent_turn_id.map(str::to_string),
            seq,
            status: "running".to_string(),
            prompt: prompt.map(str::to_string),
            started_at,
            completed_at: None,
            usage: None,
            stop_reason: None,
        })
    }

    /// Close a turn. Exactly one of completed/failed/cancelled is allowed (I1).
    pub async fn finish_turn(&self, turn_id: &str, status: &str) -> Result<Turn, SessionError> {
        self.finish_turn_with_reason(turn_id, status, None).await
    }

    /// `stop_reason` is how the turn ended in the coordinator's words (`completed`,
    /// `budget_exceeded`, `cancelled`, `model_error`), kept for replay and for the QA script.
    pub async fn finish_turn_with_reason(
        &self,
        turn_id: &str,
        status: &str,
        stop_reason: Option<&str>,
    ) -> Result<Turn, SessionError> {
        if !matches!(status, "completed" | "failed" | "cancelled") {
            return Err(SessionError::InvalidTurnStatus(status.to_string()));
        }

        if status == "completed" {
            self.bump_tasks_done_for_turn(turn_id).await?;
        }

        let completed_at = now_ms();
        let result = sqlx::query(
            "UPDATE turns SET status = ?, completed_at = ?, stop_reason = COALESCE(?, stop_reason)
             WHERE id = ?",
        )
        .bind(status)
        .bind(completed_at)
        .bind(stop_reason)
        .bind(turn_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(SessionError::TurnNotFound(turn_id.to_string()));
        }

        self.turn(turn_id).await
    }

    pub async fn turn(&self, turn_id: &str) -> Result<Turn, SessionError> {
        let row: Option<TurnRow> = sqlx::query_as(
            "SELECT id, agent_id, seq, status, prompt, started_at, completed_at,
                    usage_json, stop_reason, parent_turn_id
             FROM turns WHERE id = ?",
        )
        .bind(turn_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(turn_from_row)
            .transpose()?
            .ok_or_else(|| SessionError::TurnNotFound(turn_id.to_string()))
    }

    /// A completed turn counts as a task only if it recorded no error item (§8.8).
    async fn bump_tasks_done_for_turn(&self, turn_id: &str) -> Result<(), SessionError> {
        let row: Option<(String, i64)> = sqlx::query_as(
            "SELECT a.session_id, COUNT(*) FILTER (WHERE i.kind = 'error')
             FROM turns t
             JOIN agents a ON a.id = t.agent_id
             LEFT JOIN items i ON i.turn_id = t.id
             WHERE t.id = ? AND t.parent_turn_id IS NULL
             GROUP BY a.session_id",
        )
        .bind(turn_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some((session_id, errors)) = row {
            if errors == 0 {
                sqlx::query("UPDATE sessions SET tasks_done = tasks_done + 1 WHERE id = ?")
                    .bind(session_id)
                    .execute(&self.pool)
                    .await?;
            }
        }
        Ok(())
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
            raw: None,
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
        self.items_with_raw(turn_id, false).await
    }

    /// `include_raw` is the replay pane's debug toggle (P3.9). Raw JSON is always stored; this
    /// only decides whether the query pays to deserialise it.
    pub async fn items_with_raw(
        &self,
        turn_id: &str,
        include_raw: bool,
    ) -> Result<Vec<Item>, SessionError> {
        let rows: Vec<ItemRow> = sqlx::query_as(
            "SELECT id, turn_id, seq, kind, status, body_json, created_at, raw_json
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
                    raw: if include_raw {
                        Some(serde_json::from_str(&row.7).unwrap_or(serde_json::Value::Null))
                    } else {
                        None
                    },
                })
            })
            .collect()
    }

    /// Everything the session surface renders, newest turn last, with raw included so the
    /// debug toggle costs no extra round trip.
    pub async fn transcript(&self, session_id: &str) -> Result<Vec<TurnTranscript>, SessionError> {
        let session = self.get(session_id).await?;
        let turns: Vec<TurnRow> = sqlx::query_as(
            "SELECT t.id, t.agent_id, t.seq, t.status, t.prompt, t.started_at, t.completed_at,
                    t.usage_json, t.stop_reason, t.parent_turn_id
             FROM turns t JOIN agents a ON a.id = t.agent_id
             WHERE a.session_id = ? ORDER BY t.started_at, t.seq",
        )
        .bind(&session.id)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(turns.len());
        for row in turns {
            let turn = turn_from_row(row)?;
            let items = self.items_with_raw(&turn.id, true).await?;
            out.push(TurnTranscript { turn, items });
        }
        Ok(out)
    }

    pub async fn latest_checkpoint(
        &self,
        session_id: &str,
    ) -> Result<Option<Checkpoint>, SessionError> {
        let row: Option<(String, String, Option<String>, String, i64)> = sqlx::query_as(
            "SELECT id, session_id, turn_id, tree_hash, created_at
             FROM checkpoints WHERE session_id = ? ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(Checkpoint {
                id: row.0,
                session_id: row.1,
                turn_id: row.2,
                tree_hash: row.3,
                created_at: row.4,
            })
        })
        .transpose()
    }

    pub async fn running_turns(&self, session_id: &str) -> Result<Vec<Turn>, SessionError> {
        let rows: Vec<TurnRow> = sqlx::query_as(
            "SELECT t.id, t.agent_id, t.seq, t.status, t.prompt, t.started_at, t.completed_at,
                    t.usage_json, t.stop_reason, t.parent_turn_id
             FROM turns t JOIN agents a ON a.id = t.agent_id
             WHERE a.session_id = ? AND t.status = 'running'
             ORDER BY t.started_at, t.seq",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(turn_from_row).collect()
    }

    pub async fn set_budget(
        &self,
        session_id: &str,
        cap_cents: Option<i64>,
    ) -> Result<(), SessionError> {
        let result = sqlx::query("UPDATE sessions SET budget_cents = ? WHERE id = ?")
            .bind(cap_cents)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(SessionError::SessionNotFound(session_id.to_string()));
        }
        Ok(())
    }

    /// The `interventions` counter is the product metric's denominator (D17, §8.8). One
    /// function writes it so the event row and the counter cannot drift.
    pub async fn record_intervention(
        &self,
        session_id: &str,
        kind: &str,
        detail: serde_json::Value,
    ) -> Result<(), SessionError> {
        sqlx::query(
            "INSERT INTO interventions (id, session_id, kind, detail_json, at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(session_id)
        .bind(kind)
        .bind(detail.to_string())
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        sqlx::query("UPDATE sessions SET interventions = interventions + 1 WHERE id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Pin a skill to the body hash the session first loaded (§8.7). Re-pinning requires
    /// `repin: true`, which only the UI's explicit action passes.
    pub async fn pin_skill(
        &self,
        session_id: &str,
        skill_id: &str,
        body_sha: &str,
        repin: bool,
    ) -> Result<String, SessionError> {
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT body_sha FROM session_skills WHERE session_id = ? AND skill_id = ?",
        )
        .bind(session_id)
        .bind(skill_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((sha,)) = existing {
            if repin && sha != body_sha {
                sqlx::query(
                    "UPDATE session_skills SET body_sha = ?, pinned_at = ?
                     WHERE session_id = ? AND skill_id = ?",
                )
                .bind(body_sha)
                .bind(now_ms())
                .bind(session_id)
                .bind(skill_id)
                .execute(&self.pool)
                .await?;
                return Ok(body_sha.to_string());
            }
            return Ok(sha);
        }

        sqlx::query(
            "INSERT INTO session_skills (session_id, skill_id, body_sha, pinned_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(skill_id)
        .bind(body_sha)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(body_sha.to_string())
    }
}

type AgentRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    i64,
    i64,
    i64,
    i64,
    i64,
);

fn agent_from_row(row: AgentRow) -> Agent {
    Agent {
        id: row.0,
        session_id: row.1,
        harness: row.2,
        role: row.3,
        model: row.4,
        status: row.5,
        started_at: row.6,
        spend_cents: row.7,
        spend_micros: row.8,
        tokens_in: row.9,
        tokens_out: row.10,
    }
}

type TurnRow = (
    String,
    String,
    i64,
    String,
    Option<String>,
    i64,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn turn_from_row(row: TurnRow) -> Result<Turn, SessionError> {
    Ok(Turn {
        id: row.0,
        agent_id: row.1,
        seq: row.2,
        status: row.3,
        prompt: row.4,
        started_at: row.5,
        completed_at: row.6,
        usage: row
            .7
            .as_deref()
            .and_then(|json| serde_json::from_str::<Usage>(json).ok()),
        stop_reason: row.8,
        parent_turn_id: row.9,
    })
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
        spend_micros: row.6,
        interventions: row.7,
        tasks_done: row.8,
        created_at: row.9,
        ended_at: row.10,
    })
}

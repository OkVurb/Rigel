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
    /// D148: pinned sessions sort first in the window's list.
    pub pinned_at: Option<i64>,
    /// D148: hidden from the default list, still searchable and still resumable.
    pub archived_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Agent {
    pub id: String,
    pub session_id: String,
    pub harness: String,
    pub role: Option<String>,
    pub model: Option<String>,
    /// The agent's private checkout, when it was given one (P5.5).
    pub worktree: Option<String>,
    pub status: String,
    pub started_at: i64,
    pub spend_cents: i64,
    pub spend_micros: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

/// D150: a kept line. `label` is the snapshot the inspector draws; `target_id` is what a jump
/// resolves against (`item-…` in the transcript, `hunk-…` in Diff).
#[derive(Debug, Clone, Serialize)]
pub struct Bookmark {
    pub id: String,
    pub session_id: String,
    pub kind: String,
    pub target_id: String,
    pub label: String,
    pub created_at: i64,
}

/// A writable subagent as the Diff view lists it (D80/D114/D115): who it is, what it was
/// approved to do, and where its pending work lives. The pending diff itself is read on
/// demand, not on the two-second coordination poll.
#[derive(Debug, Clone, Serialize)]
pub struct WritableAgent {
    pub agent: Agent,
    /// The plan the user approved; `None` means the gate was waived.
    pub plan: Option<String>,
    /// The waiver, when the plan gate was skipped.
    pub plan_note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamAgent {
    /// Native children use their agent id; chat-room agents use their room name.
    pub agent_key: String,
    pub role: Option<String>,
    /// `native` or `room`: where the assignment was delivered.
    pub kind: String,
    /// Project-relative paths this agent was told it may read.
    pub files: Vec<String>,
    pub task: String,
    pub status: String,
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

    #[error("unknown role preset `{0}`; use implementer, reviewer, tester or researcher")]
    InvalidRole(String),

    #[error("turn {0} does not exist")]
    TurnNotFound(String),

    #[error("invalid status: {0}")]
    InvalidStatus(String),

    #[error("invalid turn status: {0}")]
    InvalidTurnStatus(String),

    #[error("bookmark kind `{0}` is not one of item or hunk")]
    InvalidBookmarkKind(String),

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
    Option<i64>,
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
                    interventions, tasks_done, created_at, ended_at, pinned_at, archived_at
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

    /// Pinned first, then newest: the session list is a log with a favourites shelf.
    ///
    /// Archived sessions are still returned — the window hides them behind a toggle, and the
    /// palette searches them — because a flag must not make a transcript unreachable.
    pub async fn list(&self) -> Result<Vec<Session>, SessionError> {
        let rows: Vec<SessionRow> = sqlx::query_as(
            "SELECT id, project_root, title, status, budget_cents, spend_cents, spend_micros,
                    interventions, tasks_done, created_at, ended_at, pinned_at, archived_at
             FROM sessions
             ORDER BY (pinned_at IS NULL), pinned_at DESC, created_at DESC, id DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(session_from_row).collect()
    }

    /// D148: name a session, or clear the name so the automatic title takes over again.
    ///
    /// The list is one line per row, so the name is the first line and at most 120 characters;
    /// silently clipping is still better than a row that breaks the pane it is drawn in.
    pub async fn rename(&self, id: &str, title: Option<&str>) -> Result<Session, SessionError> {
        let title = title
            .map(|value| value.lines().next().unwrap_or("").trim())
            .filter(|value| !value.is_empty())
            .map(|value| value.chars().take(120).collect::<String>());

        let result = sqlx::query("UPDATE sessions SET title = ? WHERE id = ?")
            .bind(title)
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(SessionError::SessionNotFound(id.to_string()));
        }
        self.get(id).await
    }

    /// D148: pin or unpin. Pinning is a timestamp, not a boolean, so the pinned block keeps its
    /// own order when it is re-sorted by recency of the pin.
    pub async fn set_pinned(&self, id: &str, pinned: bool) -> Result<Session, SessionError> {
        let result = sqlx::query("UPDATE sessions SET pinned_at = ? WHERE id = ?")
            .bind(pinned.then(now_ms))
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(SessionError::SessionNotFound(id.to_string()));
        }
        self.get(id).await
    }

    /// D148: archive or restore. Nothing is deleted, so the transcript, patches and checkpoints
    /// stay resumable while the session is out of the default list.
    pub async fn set_archived(&self, id: &str, archived: bool) -> Result<Session, SessionError> {
        let result = sqlx::query("UPDATE sessions SET archived_at = ? WHERE id = ?")
            .bind(archived.then(now_ms))
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(SessionError::SessionNotFound(id.to_string()));
        }
        self.get(id).await
    }

    /// D150: newest first. A star is a note to the reader, so the list is read like one.
    pub async fn bookmarks(&self, session_id: &str) -> Result<Vec<Bookmark>, SessionError> {
        let rows: Vec<(String, String, String, String, String, i64)> = sqlx::query_as(
            "SELECT id, session_id, kind, target_id, label, created_at
             FROM bookmarks WHERE session_id = ? ORDER BY created_at DESC, id DESC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, session_id, kind, target_id, label, created_at)| Bookmark {
                    id,
                    session_id,
                    kind,
                    target_id,
                    label,
                    created_at,
                },
            )
            .collect())
    }

    /// D150: starring twice clears the star. `Some` means the star is now on, `None` means off.
    /// The label keeps one line and 160 characters; the target is the row the jump resolves.
    pub async fn toggle_bookmark(
        &self,
        session_id: &str,
        kind: &str,
        target_id: &str,
        label: &str,
    ) -> Result<Option<Bookmark>, SessionError> {
        if !matches!(kind, "item" | "hunk") {
            return Err(SessionError::InvalidBookmarkKind(kind.to_string()));
        }
        // A star on a missing session would fail the foreign key with a database error; name
        // the real problem instead.
        self.get(session_id).await?;

        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM bookmarks WHERE session_id = ? AND kind = ? AND target_id = ?",
        )
        .bind(session_id)
        .bind(kind)
        .bind(target_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(id) = existing {
            sqlx::query("DELETE FROM bookmarks WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
            return Ok(None);
        }

        let label = label
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .take(160)
            .collect::<String>();
        let bookmark = Bookmark {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            kind: kind.to_string(),
            target_id: target_id.to_string(),
            label,
            created_at: now_ms(),
        };
        sqlx::query(
            "INSERT INTO bookmarks (id, session_id, kind, target_id, label, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&bookmark.id)
        .bind(&bookmark.session_id)
        .bind(&bookmark.kind)
        .bind(&bookmark.target_id)
        .bind(&bookmark.label)
        .bind(bookmark.created_at)
        .execute(&self.pool)
        .await?;

        Ok(Some(bookmark))
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

    /// D94: apply a named role preset to an existing agent. The prompt, effort and tool
    /// allowance are looked up from the preset on every turn; the row remembers the role and
    /// model so the roster shows what was chosen.
    pub async fn apply_role_preset(
        &self,
        session_id: &str,
        agent_id: &str,
        preset: &str,
    ) -> Result<Agent, SessionError> {
        let Some(preset) = crate::agent::roles::preset(Some(preset)) else {
            return Err(SessionError::InvalidRole(preset.to_string()));
        };
        let result =
            sqlx::query("UPDATE agents SET role = ?, model = ? WHERE id = ? AND session_id = ?")
                .bind(preset.id)
                .bind(preset.model)
                .bind(agent_id)
                .bind(session_id)
                .execute(&self.pool)
                .await?;
        if result.rows_affected() == 0 {
            return Err(SessionError::AgentNotFound(agent_id.to_string()));
        }
        self.agent(agent_id).await
    }

    pub async fn agents(&self, session_id: &str) -> Result<Vec<Agent>, SessionError> {
        let rows: Vec<AgentRow> = sqlx::query_as(
            "SELECT id, session_id, harness, role, model, worktree, status, started_at,
                    spend_cents, spend_micros, tokens_in, tokens_out
             FROM agents WHERE session_id = ? ORDER BY started_at, rowid",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(agent_from_row).collect())
    }

    /// Record one file-scoped assignment on the session's work board. Re-assigning the same
    /// agent replaces its previous row, which is what a retry or a re-split should do.
    pub async fn assign_team(
        &self,
        session_id: &str,
        agent_key: &str,
        role: Option<&str>,
        kind: &str,
        files: &[String],
        task: &str,
    ) -> Result<(), SessionError> {
        if !matches!(kind, "native" | "room") {
            return Err(SessionError::InvalidStatus(kind.to_string()));
        }
        self.get(session_id).await?;
        let files_json = serde_json::to_string(files)?;
        sqlx::query(
            "INSERT INTO team_agents
                 (session_id, agent_key, role, kind, files_json, task, status, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, 'assigned', ?)
             ON CONFLICT(session_id, agent_key) DO UPDATE SET
                 role = excluded.role,
                 kind = excluded.kind,
                 files_json = excluded.files_json,
                 task = excluded.task,
                 status = 'assigned',
                 updated_at = excluded.updated_at",
        )
        .bind(session_id)
        .bind(agent_key)
        .bind(role)
        .bind(kind)
        .bind(files_json)
        .bind(task)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_team_status(
        &self,
        session_id: &str,
        agent_key: &str,
        status: &str,
    ) -> Result<(), SessionError> {
        if !matches!(status, "assigned" | "working" | "done" | "failed") {
            return Err(SessionError::InvalidStatus(status.to_string()));
        }
        sqlx::query(
            "UPDATE team_agents SET status = ?, updated_at = ? WHERE session_id = ? AND agent_key = ?",
        )
        .bind(status)
        .bind(now_ms())
        .bind(session_id)
        .bind(agent_key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The work board, in the order assignments were first made.
    pub async fn team(&self, session_id: &str) -> Result<Vec<TeamAgent>, SessionError> {
        let rows: Vec<(String, Option<String>, String, String, String, String)> = sqlx::query_as(
            "SELECT agent_key, role, kind, files_json, task, status
             FROM team_agents WHERE session_id = ? ORDER BY rowid",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(agent_key, role, kind, files_json, task, status)| TeamAgent {
                    files: serde_json::from_str(&files_json).unwrap_or_default(),
                    agent_key,
                    role,
                    kind,
                    task,
                    status,
                },
            )
            .collect())
    }

    pub async fn agent(&self, id: &str) -> Result<Agent, SessionError> {
        let row: Option<AgentRow> = sqlx::query_as(
            "SELECT id, session_id, harness, role, model, worktree, status, started_at,
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

    /// D80/D114: remember that this agent may write, and which plan the user approved (or
    /// why the gate was skipped). Written before the child starts, so a child that dies
    /// mid-turn still has a record of what it was allowed to do.
    pub async fn mark_writable(
        &self,
        agent_id: &str,
        plan: &str,
        note: Option<&str>,
    ) -> Result<(), SessionError> {
        let result = sqlx::query(
            "UPDATE agents SET writable = 1, plan_json = ?, plan_note = ? WHERE id = ?",
        )
        .bind(plan)
        .bind(note)
        .bind(agent_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(SessionError::AgentNotFound(agent_id.to_string()));
        }
        Ok(())
    }

    /// D113: writable children currently running in this session. The cap counts these, not
    /// the ones that already finished: a finished child no longer holds a worktree open.
    pub async fn writable_running(&self, session_id: &str) -> Result<i64, SessionError> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agents WHERE session_id = ? AND writable = 1
             AND status = 'running'",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    /// Every agent in the session that may write, in registration order. This is the list the
    /// Diff view shows with their plans and reviews; it is small by construction (D113 caps
    /// the running ones).
    pub async fn writable_agents(
        &self,
        session_id: &str,
    ) -> Result<Vec<WritableAgent>, SessionError> {
        let ids: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT id, plan_json, plan_note FROM agents
             WHERE session_id = ? AND writable = 1 ORDER BY started_at, rowid",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(ids.len());
        for (id, plan, plan_note) in ids {
            out.push(WritableAgent {
                agent: self.agent(&id).await?,
                plan,
                plan_note,
            });
        }
        Ok(out)
    }

    /// The plan an agent was approved to carry out, if any.
    pub async fn plan_of(&self, agent_id: &str) -> Result<Option<String>, SessionError> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT plan_json FROM agents WHERE id = ?")
                .bind(agent_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.and_then(|(plan,)| plan))
    }

    /// Whether this agent was registered as writable.
    pub async fn is_writable(&self, agent_id: &str) -> Result<bool, SessionError> {
        let row: Option<(i64,)> = sqlx::query_as("SELECT writable FROM agents WHERE id = ?")
            .bind(agent_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(writable,)| writable != 0).unwrap_or(false))
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

    /// Where a note about `agent_id`'s work belongs: the parent turn that spawned its latest
    /// run, so the note reads beside the child's own rows, or the agent's latest turn when it
    /// has no parent. The parent link is the turn's, not the agent's (`turns.parent_turn_id`),
    /// which is the one a spawned child actually records.
    pub async fn note_turn(&self, agent_id: &str) -> Result<Option<Turn>, SessionError> {
        let row: Option<TurnRow> = sqlx::query_as(
            "SELECT id, agent_id, seq, status, prompt, started_at, completed_at, usage_json,\n                    stop_reason, parent_turn_id\n             FROM turns WHERE agent_id = ? ORDER BY started_at DESC, seq DESC LIMIT 1",
        )
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(latest) = row.map(turn_from_row).transpose()? else {
            return Ok(None);
        };
        match latest.parent_turn_id.clone() {
            Some(parent) => self.turn(&parent).await.map(Some),
            None => Ok(Some(latest)),
        }
    }

    /// The notes left on this agent's previous turn, in the order they were posted. Every turn
    /// starts from a fresh prompt, so this is the one path by which something the user did in
    /// the window reaches the next model call.
    pub async fn previous_notes(
        &self,
        agent_id: &str,
        current_turn: &str,
    ) -> Result<Vec<serde_json::Value>, SessionError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT body_json FROM items\n             WHERE kind = 'note' AND turn_id = (\n               SELECT id FROM turns WHERE agent_id = ? AND id != ?\n               ORDER BY started_at DESC, seq DESC LIMIT 1\n             ) ORDER BY seq",
        )
        .bind(agent_id)
        .bind(current_turn)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(body,)| serde_json::from_str(&body).ok())
            .collect())
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

    /// Every checkpoint this session recorded, newest last. D89 lists one per turn.
    pub async fn checkpoints(&self, session_id: &str) -> Result<Vec<Checkpoint>, SessionError> {
        self.get(session_id).await?;
        let rows: Vec<(String, String, Option<String>, String, i64)> = sqlx::query_as(
            "SELECT id, session_id, turn_id, tree_hash, created_at
             FROM checkpoints WHERE session_id = ? ORDER BY created_at, id",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| Checkpoint {
                id: row.0,
                session_id: row.1,
                turn_id: row.2,
                tree_hash: row.3,
                created_at: row.4,
            })
            .collect())
    }

    /// The journal a checkpoint wrote, which names the snapshot to restore (D89).
    pub async fn checkpoint_journal(
        &self,
        id: &str,
    ) -> Result<Option<serde_json::Value>, SessionError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT journal_json FROM checkpoints WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.and_then(|row| serde_json::from_str(&row.0).ok()))
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

    /// The body hash this session pinned for a skill, without changing anything.
    pub async fn skill_pin(
        &self,
        session_id: &str,
        skill_id: &str,
    ) -> Result<Option<String>, SessionError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT body_sha FROM session_skills WHERE session_id = ? AND skill_id = ?",
        )
        .bind(session_id)
        .bind(skill_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| row.0))
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
        worktree: row.5,
        status: row.6,
        started_at: row.7,
        spend_cents: row.8,
        spend_micros: row.9,
        tokens_in: row.10,
        tokens_out: row.11,
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
        pinned_at: row.11,
        archived_at: row.12,
    })
}

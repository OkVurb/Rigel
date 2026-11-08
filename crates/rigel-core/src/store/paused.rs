//! Paused turns that survive the window closing (roadmap 40).
//!
//! The row owns the exact `ChatMessage` list the coordinator had built, the remaining step
//! bound, and the remaining session micro-dollars. `resume` is an atomic claim: the second
//! caller gets `None`, so a double resume cannot send the same messages twice.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::agent::ChatMessage;
use crate::migrate::now_ms;
use crate::session::SessionError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PausedTurn {
    pub turn_id: String,
    pub session_id: String,
    pub agent_id: String,
    pub messages: Vec<ChatMessage>,
    pub remaining_steps: usize,
    pub remaining_micros: Option<i64>,
    pub paused_at: i64,
    pub resumed_at: Option<i64>,
}

/// The eight columns `load` reads, named so the row stays readable.
type PausedRow = (
    String,
    String,
    String,
    String,
    i64,
    Option<i64>,
    i64,
    Option<i64>,
);

#[derive(Clone)]
pub struct PauseStore {
    pool: SqlitePool,
}

impl PauseStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn save(&self, paused: &PausedTurn) -> Result<(), SessionError> {
        let messages = serde_json::to_string(&paused.messages)?;
        sqlx::query(
            "INSERT INTO paused_turns
                (turn_id, session_id, agent_id, messages_json, remaining_steps,
                 remaining_micros, paused_at, resumed_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, NULL)
             ON CONFLICT(turn_id) DO UPDATE SET
                messages_json = excluded.messages_json,
                remaining_steps = excluded.remaining_steps,
                remaining_micros = excluded.remaining_micros,
                paused_at = excluded.paused_at,
                resumed_at = NULL",
        )
        .bind(&paused.turn_id)
        .bind(&paused.session_id)
        .bind(&paused.agent_id)
        .bind(messages)
        .bind(paused.remaining_steps as i64)
        .bind(paused.remaining_micros)
        .bind(paused.paused_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load(&self, turn_id: &str) -> Result<Option<PausedTurn>, SessionError> {
        let row: Option<PausedRow> = sqlx::query_as(
            "SELECT turn_id, session_id, agent_id, messages_json, remaining_steps,
                        remaining_micros, paused_at, resumed_at
                 FROM paused_turns WHERE turn_id = ?",
        )
        .bind(turn_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(PausedTurn {
                turn_id: row.0,
                session_id: row.1,
                agent_id: row.2,
                messages: serde_json::from_str(&row.3)?,
                remaining_steps: row.4 as usize,
                remaining_micros: row.5,
                paused_at: row.6,
                resumed_at: row.7,
            })
        })
        .transpose()
    }

    /// Claim a paused turn exactly once. The update and the read are separate statements so an
    /// already-resumed row is still readable for audit via [`load`], while this method refuses
    /// to hand it out a second time.
    pub async fn resume(&self, turn_id: &str) -> Result<Option<PausedTurn>, SessionError> {
        let changed = sqlx::query(
            "UPDATE paused_turns SET resumed_at = ? WHERE turn_id = ? AND resumed_at IS NULL",
        )
        .bind(now_ms())
        .bind(turn_id)
        .execute(&self.pool)
        .await?;
        if changed.rows_affected() != 1 {
            return Ok(None);
        }
        self.load(turn_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn service() -> (
        tempfile::TempDir,
        crate::SessionService,
        crate::Session,
        crate::Agent,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let service = crate::SessionService::open(tmp.path()).await.unwrap();
        let session = service.start(Some("paused"), Some(25)).await.unwrap();
        let agent = service
            .register_agent(&session.id, "native", Some("worker"))
            .await
            .unwrap();
        (tmp, service, session, agent)
    }

    #[tokio::test]
    async fn reopen_keeps_the_same_messages_and_budget_and_double_resume_is_refused() {
        let (_tmp, service, session, agent) = service().await;
        let turn = service
            .append_turn(&agent.id, Some("finish the report"))
            .await
            .unwrap();
        let store = PauseStore::new(service.pool().clone());
        let messages = vec![
            ChatMessage::system("system"),
            ChatMessage::user("finish the report"),
            ChatMessage::assistant(Some("working".to_string()), Vec::new()),
        ];
        store
            .save(&PausedTurn {
                turn_id: turn.id.clone(),
                session_id: session.id.clone(),
                agent_id: agent.id.clone(),
                messages: messages.clone(),
                remaining_steps: 3,
                remaining_micros: Some(42_000),
                paused_at: now_ms(),
                resumed_at: None,
            })
            .await
            .unwrap();

        // A fresh store over the same database is what reopening the window does.
        let reopened = PauseStore::new(service.pool().clone());
        let loaded = reopened.load(&turn.id).await.unwrap().unwrap();
        assert_eq!(loaded.messages.len(), 3);
        assert_eq!(
            loaded.messages[1].content.as_deref(),
            Some("finish the report")
        );
        assert_eq!(loaded.remaining_steps, 3);
        assert_eq!(loaded.remaining_micros, Some(42_000));

        assert!(reopened.resume(&turn.id).await.unwrap().is_some());
        assert!(
            reopened.resume(&turn.id).await.unwrap().is_none(),
            "a second resume must not send the same messages twice"
        );
    }
}

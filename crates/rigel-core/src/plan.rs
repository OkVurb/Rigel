//! D98: plan mode. A session can plan before it touches the tree.
//!
//! The flag lives in `session_plan`, not in the window, so a reload cannot silently turn the
//! guard off. While the mode is on, the coordinator hides the writing tools and refuses them
//! even if a model calls one anyway; approving the plan is the only way to clear it.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::migrate::now_ms;

#[derive(Debug, Clone, Serialize)]
pub struct PlanState {
    pub session_id: String,
    pub mode: bool,
    pub approved_at: Option<i64>,
    pub updated_at: i64,
}

pub async fn state(pool: &SqlitePool, session_id: &str) -> Result<PlanState, sqlx::Error> {
    let row: Option<(i64, Option<i64>, i64)> = sqlx::query_as(
        "SELECT mode, approved_at, updated_at FROM session_plan WHERE session_id = ?",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    let (mode, approved_at, updated_at) = row.unwrap_or((0, None, 0));
    Ok(PlanState {
        session_id: session_id.to_string(),
        mode: mode != 0,
        approved_at,
        updated_at,
    })
}

/// Turn plan mode on or off. Turning it on clears the previous approval: a new plan needs a
/// new hand-off. Turning it off is the approval itself.
pub async fn set(
    pool: &SqlitePool,
    session_id: &str,
    mode: bool,
) -> Result<PlanState, sqlx::Error> {
    let now = now_ms();
    sqlx::query(
        "INSERT INTO session_plan (session_id, mode, approved_at, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(session_id) DO UPDATE SET
             mode = excluded.mode,
             approved_at = excluded.approved_at,
             updated_at = excluded.updated_at",
    )
    .bind(session_id)
    .bind(i64::from(mode))
    .bind(if mode { None } else { Some(now) })
    .bind(now)
    .execute(pool)
    .await?;
    state(pool, session_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionService;

    #[tokio::test]
    async fn plan_mode_survives_a_reopen_and_an_approval_clears_it() {
        let tmp = tempfile::TempDir::new().unwrap();
        let service = SessionService::open(tmp.path()).await.unwrap();
        let session = service.start(Some("plan"), None).await.unwrap();
        assert!(!state(service.pool(), &session.id).await.unwrap().mode);

        let planned = set(service.pool(), &session.id, true).await.unwrap();
        assert!(planned.mode);
        assert!(planned.approved_at.is_none());
        assert!(state(service.pool(), &session.id).await.unwrap().mode);

        let approved = set(service.pool(), &session.id, false).await.unwrap();
        assert!(!approved.mode);
        assert!(approved.approved_at.is_some());
    }
}

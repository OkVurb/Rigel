//! D98/roadmap 41: plan mode with one approved tool plan.
//!
//! The phase lives in `session_plan`, not in the window, so a reload cannot silently turn the
//! guard off. While the mode is on, the coordinator hides the writing tools and refuses them
//! even if a model calls one anyway. The agent proposes the exact tools it needs through
//! `propose_plan`; approving once records those names, and execution inside that set does not
//! ask again. A tool outside the set re-arms the approval instead of running silently.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::migrate::now_ms;

#[derive(Debug, Clone, Serialize)]
pub struct PlanState {
    pub session_id: String,
    pub mode: bool,
    /// The numbered plan the agent proposed, kept after approval for the session view.
    pub plan: Option<String>,
    /// The tool names the user approved for this plan. Empty before a proposal.
    pub tools: Vec<String>,
    pub proposed_at: Option<i64>,
    pub approved_at: Option<i64>,
    pub updated_at: i64,
}

/// The six columns `state` reads, named so the row stays readable.
type PlanRow = (
    i64,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    i64,
);

pub async fn state(pool: &SqlitePool, session_id: &str) -> Result<PlanState, sqlx::Error> {
    let row: Option<PlanRow> = sqlx::query_as(
        "SELECT mode, plan_text, tools_json, proposed_at, approved_at, updated_at
             FROM session_plan WHERE session_id = ?",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    let (mode, plan, tools_json, proposed_at, approved_at, updated_at) =
        row.unwrap_or((0, None, None, None, None, 0));
    let tools = tools_json
        .as_deref()
        .and_then(|text| serde_json::from_str::<Vec<String>>(text).ok())
        .unwrap_or_default();
    Ok(PlanState {
        session_id: session_id.to_string(),
        mode: mode != 0,
        plan,
        tools,
        proposed_at,
        approved_at,
        updated_at,
    })
}

/// Turn plan mode on or off. Turning it on clears the previous approval and proposal: a new
/// plan needs a new hand-off. Turning it off is the approval itself and keeps the proposal.
pub async fn set(
    pool: &SqlitePool,
    session_id: &str,
    mode: bool,
) -> Result<PlanState, sqlx::Error> {
    let now = now_ms();
    if mode {
        sqlx::query(
            "INSERT INTO session_plan
                (session_id, mode, plan_text, tools_json, proposed_at, approved_at, updated_at)
             VALUES (?, 1, NULL, NULL, NULL, NULL, ?)
             ON CONFLICT(session_id) DO UPDATE SET
                 mode = 1,
                 plan_text = NULL,
                 tools_json = NULL,
                 proposed_at = NULL,
                 approved_at = NULL,
                 updated_at = excluded.updated_at",
        )
        .bind(session_id)
        .bind(now)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO session_plan
                (session_id, mode, plan_text, tools_json, proposed_at, approved_at, updated_at)
             VALUES (?, 0, NULL, NULL, NULL, ?, ?)
             ON CONFLICT(session_id) DO UPDATE SET
                 mode = 0,
                 approved_at = excluded.approved_at,
                 updated_at = excluded.updated_at",
        )
        .bind(session_id)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
    }
    state(pool, session_id).await
}

/// Record a tool plan proposed by the agent. Proposing does not approve anything: the mode
/// stays on and the previous approval is cleared.
pub async fn propose(
    pool: &SqlitePool,
    session_id: &str,
    plan: &str,
    tools: &[String],
) -> Result<PlanState, sqlx::Error> {
    let now = now_ms();
    let tools_json = serde_json::to_string(tools).unwrap_or_else(|_| "[]".to_string());
    sqlx::query(
        "INSERT INTO session_plan
            (session_id, mode, plan_text, tools_json, proposed_at, approved_at, updated_at)
         VALUES (?, 1, ?, ?, ?, NULL, ?)
         ON CONFLICT(session_id) DO UPDATE SET
             mode = 1,
             plan_text = excluded.plan_text,
             tools_json = excluded.tools_json,
             proposed_at = excluded.proposed_at,
             approved_at = NULL,
             updated_at = excluded.updated_at",
    )
    .bind(session_id)
    .bind(plan)
    .bind(tools_json)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    state(pool, session_id).await
}

/// Approve the latest proposal once. The plan and its tool names stay visible afterwards.
pub async fn approve(pool: &SqlitePool, session_id: &str) -> Result<PlanState, sqlx::Error> {
    set(pool, session_id, false).await
}

/// A tool outside the approved set requires a fresh approval: keep the proposal for the card,
/// clear the hand-off, and turn the guard back on.
pub async fn invalidate(pool: &SqlitePool, session_id: &str) -> Result<PlanState, sqlx::Error> {
    let now = now_ms();
    sqlx::query(
        "INSERT INTO session_plan
            (session_id, mode, plan_text, tools_json, proposed_at, approved_at, updated_at)
         VALUES (?, 1, NULL, NULL, NULL, NULL, ?)
         ON CONFLICT(session_id) DO UPDATE SET
             mode = 1,
             approved_at = NULL,
             updated_at = excluded.updated_at",
    )
    .bind(session_id)
    .bind(now)
    .execute(pool)
    .await?;
    state(pool, session_id).await
}

/// The tool names execution may use without asking again, if a plan was approved.
pub async fn approved_tools(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Option<Vec<String>>, sqlx::Error> {
    let state = state(pool, session_id).await?;
    Ok(
        (!state.mode && state.approved_at.is_some() && !state.tools.is_empty())
            .then_some(state.tools),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionService;

    #[tokio::test]
    async fn a_proposal_names_tools_and_approval_keeps_them() {
        let tmp = tempfile::TempDir::new().unwrap();
        let service = SessionService::open(tmp.path()).await.unwrap();
        let session = service.start(Some("plan"), None).await.unwrap();
        assert!(!state(service.pool(), &session.id).await.unwrap().mode);

        let proposed = propose(
            service.pool(),
            &session.id,
            "1. Run the tests.",
            &["run_command".to_string()],
        )
        .await
        .unwrap();
        assert!(proposed.mode);
        assert!(proposed.approved_at.is_none());
        assert_eq!(proposed.tools, vec!["run_command".to_string()]);
        assert_eq!(proposed.plan.as_deref(), Some("1. Run the tests."));

        let approved = approve(service.pool(), &session.id).await.unwrap();
        assert!(!approved.mode);
        assert!(approved.approved_at.is_some());
        assert_eq!(
            approved_tools(service.pool(), &session.id)
                .await
                .unwrap()
                .unwrap(),
            vec!["run_command".to_string()]
        );

        // A tool outside the set asks again: the hand-off is cleared but the proposal stays.
        let invalidated = invalidate(service.pool(), &session.id).await.unwrap();
        assert!(invalidated.mode);
        assert!(invalidated.approved_at.is_none());
        assert_eq!(invalidated.tools, vec!["run_command".to_string()]);
        assert!(
            approved_tools(service.pool(), &session.id)
                .await
                .unwrap()
                .is_none()
        );

        // Turning the mode on explicitly starts a new plan with nothing approved.
        let cleared = set(service.pool(), &session.id, true).await.unwrap();
        assert!(cleared.mode);
        assert!(cleared.tools.is_empty());
        assert!(cleared.plan.is_none());
    }
}

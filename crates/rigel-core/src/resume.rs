//! Resume after a kill (I8, §8.5).
//!
//! A session resumes cleanly only when the working tree still hashes to its latest
//! checkpoint. Otherwise it becomes `Interrupted`: the running turn is closed as
//! `cancelled` (a turn cannot stay `running` after its process is gone), patch journals are
//! reconciled without applying anything, and the user decides whether to continue on the
//! changed tree. `force` is that decision, and it is recorded as an intervention because it
//! is exactly the kind of action the metric counts.

use std::path::Path;

use serde::Serialize;

use crate::budget::{BudgetError, BudgetService, ReconcileReport};
use crate::patches::{PatchError, PatchPipeline, Recovery};
use crate::session::{Checkpoint, Session, SessionError, SessionService, SessionStatus, Turn};
use crate::tree::{TreeError, tree_hash};

#[derive(Debug, Serialize)]
pub struct ResumeOutcome {
    pub session: Session,
    /// The tree still matches the latest checkpoint.
    pub clean: bool,
    pub tree_hash: String,
    pub checkpoint: Option<Checkpoint>,
    /// Turns that were still `running` when the process died; now `cancelled`.
    pub closed_turns: Vec<Turn>,
    pub patches: Vec<Recovery>,
    pub reconcile: ReconcileReport,
}

#[derive(Debug, thiserror::Error)]
pub enum ResumeError {
    #[error("session {0} has ended")]
    Ended(String),

    #[error(transparent)]
    Session(#[from] SessionError),

    #[error(transparent)]
    Patch(#[from] PatchError),

    #[error(transparent)]
    Tree(#[from] TreeError),

    #[error(transparent)]
    Budget(#[from] BudgetError),
}

/// Reconcile a session with the tree and the patch journal.
pub async fn resume(
    service: &SessionService,
    root: &Path,
    session_id: &str,
    force: bool,
) -> Result<ResumeOutcome, ResumeError> {
    let session = service.get(session_id).await?;
    if session.status == SessionStatus::Ended {
        return Err(ResumeError::Ended(session_id.to_string()));
    }

    let current_hash = tree_hash(root)?;
    let checkpoint = service.latest_checkpoint(session_id).await?;
    let clean = checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.tree_hash == current_hash)
        .unwrap_or(false);

    let pipeline = PatchPipeline::new(root, service.pool().clone());
    let patches = pipeline.recover(session_id).await?;

    let mut closed_turns = Vec::new();
    for turn in service.running_turns(session_id).await? {
        let closed = service
            .finish_turn_with_reason(&turn.id, "cancelled", Some("interrupted"))
            .await?;
        closed_turns.push(closed);
    }
    for agent in service.agents(session_id).await? {
        if matches!(
            agent.status.as_str(),
            "running" | "waiting_approval" | "starting"
        ) {
            service.set_agent_status(&agent.id, "interrupted").await?;
        }
    }

    let budget = BudgetService::with_null_interrupt(service.pool().clone());
    let reconcile = budget.reconcile(session_id).await?;

    if clean {
        service
            .set_status(session_id, SessionStatus::Active)
            .await?;
    } else if force {
        service
            .set_status(session_id, SessionStatus::Active)
            .await?;
        service
            .record_intervention(
                session_id,
                "resume_confirmed",
                serde_json::json!({
                    "checkpoint_tree": checkpoint.as_ref().map(|c| c.tree_hash.clone()),
                    "current_tree": current_hash,
                }),
            )
            .await?;
        // The user accepted the new tree; make that the baseline so the next resume is clean.
        service
            .checkpoint(session_id, None, &current_hash, &serde_json::json!([]))
            .await?;
    } else {
        service
            .set_status(session_id, SessionStatus::Interrupted)
            .await?;
    }

    let session = service.get(session_id).await?;
    Ok(ResumeOutcome {
        session,
        clean,
        tree_hash: current_hash,
        checkpoint,
        closed_turns,
        patches,
        reconcile,
    })
}

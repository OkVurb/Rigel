//! What stands between a writable child and the main tree (D115).
//!
//! The reviewer is a model call with no context but the diff: it never sees the author's
//! reasoning, its prompt, or the transcript that led to the change. Its verdict is stored
//! before the merge is attempted, and a merge that a reviewer blocked is only possible with
//! an override the user typed, which is kept beside the verdict. The record is the audit
//! trail D64 asks for; nothing here can allow an edit on its own.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::migrate::now_ms;
use crate::session::{Agent, SessionService};
use crate::worktree::{MergeReport, WorktreeError, WorktreeService};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approved,
    Blocked,
    /// The reviewer could not answer. Treated as blocked: a call that failed is not consent.
    Unavailable,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Blocked => "blocked",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "approved" => Self::Approved,
            "blocked" => Self::Blocked,
            "unavailable" => Self::Unavailable,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MergeReview {
    pub id: String,
    pub session_id: String,
    pub agent_id: String,
    pub reviewer_model: String,
    pub verdict: Verdict,
    pub reason: Option<String>,
    pub diff_sha: String,
    pub files: Vec<String>,
    pub override_note: Option<String>,
    pub merged: bool,
    pub created_at: i64,
}

pub fn diff_sha(diff: &str) -> String {
    crate::patches::sha256_hex(diff.as_bytes())
}

/// Everything one review records. A bundle rather than eight positional arguments: the
/// verdict, the diff it applies to and the override all have to travel together.
pub struct ReviewInput<'a> {
    pub session_id: &'a str,
    pub agent_id: &'a str,
    pub reviewer_model: &'a str,
    pub verdict: Verdict,
    pub reason: Option<&'a str>,
    pub diff: &'a str,
    pub files: &'a [String],
    pub override_note: Option<&'a str>,
}

/// The stored columns, in the order [`MergeReviews::latest`] reads them.
type ReviewRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
    i64,
    i64,
);

pub struct MergeReviews {
    pool: SqlitePool,
}

impl MergeReviews {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Write the verdict before the merge is attempted. `merged` is updated afterwards, so a
    /// crash between the two leaves an approved review that did not land: honest, and easy
    /// to retry.
    pub async fn record(&self, input: ReviewInput<'_>) -> Result<MergeReview, sqlx::Error> {
        let id = uuid::Uuid::new_v4().to_string();
        let files_json = serde_json::to_string(input.files).unwrap_or_else(|_| "[]".to_string());
        let sha = diff_sha(input.diff);
        let created_at = now_ms();
        sqlx::query(
            "INSERT INTO merge_reviews
               (id, session_id, agent_id, reviewer_model, verdict, reason, diff_sha,
                files_json, override_note, merged, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?)",
        )
        .bind(&id)
        .bind(input.session_id)
        .bind(input.agent_id)
        .bind(input.reviewer_model)
        .bind(input.verdict.as_str())
        .bind(input.reason)
        .bind(&sha)
        .bind(&files_json)
        .bind(input.override_note)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(MergeReview {
            id,
            session_id: input.session_id.to_string(),
            agent_id: input.agent_id.to_string(),
            reviewer_model: input.reviewer_model.to_string(),
            verdict: input.verdict,
            reason: input.reason.map(str::to_string),
            diff_sha: sha,
            files: input.files.to_vec(),
            override_note: input.override_note.map(str::to_string),
            merged: false,
            created_at,
        })
    }

    /// Attach the user's reason to the verdict it answers. The earlier `None` case was a
    /// merge without an override, so this is only written when a block is being crossed.
    pub async fn set_override(&self, id: &str, note: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE merge_reviews SET override_note = ? WHERE id = ?")
            .bind(note)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_merged(&self, id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE merge_reviews SET merged = 1 WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn latest(&self, agent_id: &str) -> Result<Option<MergeReview>, sqlx::Error> {
        let row: Option<ReviewRow> = sqlx::query_as(
            "SELECT id, session_id, agent_id, reviewer_model, verdict, reason, diff_sha,
                    files_json, override_note, merged, created_at
             FROM merge_reviews WHERE agent_id = ? ORDER BY created_at DESC, rowid DESC LIMIT 1",
        )
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(
                id,
                session_id,
                agent_id,
                reviewer_model,
                verdict,
                reason,
                diff_sha,
                files_json,
                override_note,
                merged,
                created_at,
            )| MergeReview {
                id,
                session_id,
                agent_id,
                reviewer_model,
                verdict: Verdict::parse(&verdict).unwrap_or(Verdict::Unavailable),
                reason,
                diff_sha,
                files: serde_json::from_str(&files_json).unwrap_or_default(),
                override_note,
                merged: merged != 0,
                created_at,
            },
        ))
    }
}

/// A user decision that could not be carried out, or the review it was applied under.
#[derive(Debug, thiserror::Error)]
pub enum DecisionError {
    /// Nothing was written. The message is for the user, so it says what to do instead.
    #[error("{0}")]
    Refused(String),

    #[error(transparent)]
    Session(#[from] crate::session::SessionError),

    #[error(transparent)]
    Worktree(#[from] WorktreeError),

    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// What a user-driven merge produced: the review it happened under, and the file-by-file
/// report. No model is called here — the verdict is the one already stored, and the reason is
/// the user's, kept beside it rather than replacing it (D115).
#[derive(Debug, Clone, Serialize)]
pub struct AppliedReview {
    pub review: MergeReview,
    pub report: MergeReport,
    /// True when the merge was clean and the checkout was removed afterwards.
    pub worktree_removed: bool,
}

/// The label a note uses for a child: its role and the first eight characters of its id, the
/// shape the spend view uses, so two children of one role stay apart in the transcript.
fn child_label(child: &Agent) -> String {
    format!(
        "{} {}",
        child.role.as_deref().unwrap_or("subagent"),
        child.id.chars().take(8).collect::<String>()
    )
}

/// Put the user's merge into the transcript, on the turn that spawned the child (D115).
///
/// The merge happened in the window, not in a tool call, so no tool result would otherwise tell
/// the parent that the main tree moved. Best effort: `merge_reviews` is the audit record, and a
/// note that cannot be written must not turn a merge that already landed into an error.
async fn post_merge_note(
    session: &SessionService,
    child: &Agent,
    review: &MergeReview,
    report: &MergeReport,
) {
    let files = review.files.len();
    let landed = if report.clean() {
        format!(
            "{files} file{} landed in the main tree",
            if files == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{} applied, {} conflicted, {} skipped — the checkout is still there",
            report.applied, report.conflicted, report.skipped
        )
    };
    let crossed = match &review.override_note {
        Some(note) => format!(
            ", overriding a {} review (\"{note}\")",
            review.verdict.as_str()
        ),
        None => String::new(),
    };
    let text = format!(
        "the user merged {}'s work from the Diff view{crossed}: {landed}",
        child_label(child)
    );
    let body = serde_json::json!({
        "text": text,
        "kind": "user_merge",
        "agent_id": child.id,
        "review_id": review.id,
        "files": review.files,
        "applied": report.applied,
        "conflicted": report.conflicted,
        "skipped": report.skipped,
        "overridden": review.override_note.is_some(),
    });
    let Ok(Some(turn)) = session.note_turn(&child.id).await else {
        return;
    };
    let _ = session
        .append_item(&turn.id, "note", "ok", &body, &body)
        .await;
}

/// Apply the stored verdict for `agent_id` on the user's behalf.
///
/// This is the Diff view's button, not a second reviewer: the merge only goes ahead when the
/// worktree still holds exactly the diff the verdict was about (`diff_sha`), and a verdict
/// that is not `approved` needs a typed reason, which is stored on that same review. Merging
/// an approved review that a conflict interrupted needs no reason. Every file still goes
/// through the CAS journal, attributed to the child, so a stale main tree is refused by the
/// pipeline rather than overwritten.
pub async fn apply_review(
    session: &SessionService,
    worktrees: &WorktreeService,
    reviews: &MergeReviews,
    session_id: &str,
    agent_id: &str,
    override_note: Option<&str>,
) -> Result<AppliedReview, DecisionError> {
    let child = session.agent(agent_id).await?;
    if child.session_id != session_id {
        return Err(DecisionError::Refused(format!(
            "{agent_id} is not an agent in this session"
        )));
    }
    if child.status == "running" {
        return Err(DecisionError::Refused(format!(
            "{agent_id} is still running; wait for its turn to end"
        )));
    }
    if !session.is_writable(agent_id).await? {
        return Err(DecisionError::Refused(format!(
            "{agent_id} is not a writable subagent; there is nothing to merge"
        )));
    }
    let Some(mut review) = reviews.latest(agent_id).await? else {
        return Err(DecisionError::Refused(
            "no review has been recorded for this worktree; ask the agent to merge it first"
                .to_string(),
        ));
    };
    if review.merged {
        return Err(DecisionError::Refused(
            "this review was already merged".to_string(),
        ));
    }
    let (diff, files) = worktrees.diff(agent_id).await?;
    if files.is_empty() {
        return Err(DecisionError::Refused(
            "the worktree has no pending changes".to_string(),
        ));
    }
    if diff_sha(&diff) != review.diff_sha {
        return Err(DecisionError::Refused(
            "the worktree changed after this review; ask for a new review before merging"
                .to_string(),
        ));
    }
    if review.verdict != Verdict::Approved {
        let Some(note) = override_note.map(str::trim).filter(|note| !note.is_empty()) else {
            return Err(DecisionError::Refused(
                "this verdict is blocked; type why you are overriding it".to_string(),
            ));
        };
        let note: String = note.chars().take(400).collect();
        reviews.set_override(&review.id, &note).await?;
        review.override_note = Some(note);
    }
    let report = worktrees.merge_back(session_id, agent_id).await?;
    let worktree_removed = if report.clean() {
        reviews.mark_merged(&review.id).await?;
        review.merged = true;
        worktrees.remove(agent_id, true).await.is_ok()
    } else {
        false
    };
    post_merge_note(session, &child, &review, &report).await;
    Ok(AppliedReview {
        review,
        report,
        worktree_removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionService;

    #[tokio::test]
    async fn a_verdict_is_stored_before_a_merge_is_marked() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = SessionService::open(tmp.path()).await.unwrap();
        let session = sessions.start(Some("native"), Some(5)).await.unwrap();
        let agent = sessions
            .register_agent(&session.id, "native", Some("writer"))
            .await
            .unwrap();
        let reviews = MergeReviews::new(sessions.pool().clone());
        let review = reviews
            .record(ReviewInput {
                session_id: &session.id,
                agent_id: &agent.id,
                reviewer_model: "openai/gpt-5-nano",
                verdict: Verdict::Approved,
                reason: Some("one file, tests included"),
                diff: "--- a\n+++ b\n",
                files: &["src/lib.rs".to_string()],
                override_note: None,
            })
            .await
            .unwrap();
        assert!(!review.merged);
        reviews.mark_merged(&review.id).await.unwrap();
        let latest = reviews.latest(&agent.id).await.unwrap().unwrap();
        assert_eq!(latest.verdict, Verdict::Approved);
        assert!(latest.merged);
        assert_eq!(latest.files, vec!["src/lib.rs".to_string()]);
        assert_eq!(latest.diff_sha, diff_sha("--- a\n+++ b\n"));
    }
}

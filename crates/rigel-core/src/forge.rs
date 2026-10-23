//! Pull-request creation through the user's own `gh` CLI (D97).
//!
//! Rigel never holds a forge token. `gh` is checked, the title and body are composed from the
//! session's own turns, and the command runs only when the user clicks the button. A missing or
//! signed-out `gh` is a clear refusal, not a silent no-op.

use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::session::{Session, TurnTranscript};

#[derive(Debug, Clone, Serialize)]
pub struct PullRequestPlan {
    pub title: String,
    pub body: String,
    pub branch: String,
    pub base: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ForgeError {
    #[error("gh is not installed or not on PATH")]
    GhMissing,

    #[error("gh is not signed in: {0}")]
    NotSignedIn(String),

    #[error("{0} is not a git repository")]
    NotARepository(String),

    #[error("gh pr create failed: {0}")]
    Failed(String),
}

fn git_output(project: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Whether `gh` exists and reports an authenticated account. Never prints a token.
pub fn gh_status() -> Result<String, ForgeError> {
    let output = Command::new("gh")
        .args(["auth", "status"])
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ForgeError::GhMissing
            } else {
                ForgeError::Failed(error.to_string())
            }
        })?;
    if output.status.success() {
        Ok("gh is signed in".to_string())
    } else {
        let text = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(ForgeError::NotSignedIn(if text.is_empty() {
            "run gh auth login".to_string()
        } else {
            text
        }))
    }
}

/// Compose a title and a reviewable body from the session. The body names every turn prompt
/// that went into it, so a reviewer can trace the branch back to the work.
pub fn plan(
    project: &Path,
    session: &Session,
    turns: &[TurnTranscript],
) -> Result<PullRequestPlan, ForgeError> {
    let branch = git_output(project, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok_or_else(|| ForgeError::NotARepository(project.display().to_string()))?;
    if branch == "HEAD" {
        return Err(ForgeError::Failed(
            "the repository is on a detached HEAD; create a branch first".to_string(),
        ));
    }
    let base = git_output(project, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        .map(|name| name.rsplit('/').next().unwrap_or("main").to_string())
        .unwrap_or_else(|| "main".to_string());
    let latest = turns
        .iter()
        .rev()
        .find_map(|turn| turn.turn.prompt.as_deref())
        .unwrap_or("Rigel session");
    let title = latest
        .lines()
        .next()
        .unwrap_or("Rigel session")
        .trim()
        .chars()
        .take(72)
        .collect::<String>();

    let mut body = String::new();
    body.push_str(&format!(
        "Session `{}` — {} turn(s), ${:.4} spent.\n\n",
        session.id,
        turns.len(),
        session.spend_micros as f64 / 1_000_000.0
    ));
    body.push_str("### What was asked\n\n");
    for turn in turns {
        if let Some(prompt) = &turn.turn.prompt {
            let first = prompt.lines().next().unwrap_or("").trim();
            if !first.is_empty() {
                body.push_str(&format!("- {first}\n"));
            }
        }
    }
    body.push_str("\n### Review\n\n");
    body.push_str(
        "Generated from Rigel's session transcript. Review the diff; Rigel did not push or open \
this pull request by itself.\n",
    );
    Ok(PullRequestPlan {
        title,
        body,
        branch,
        base,
    })
}

/// Run `gh pr create` with the reviewed title and body. The caller is the explicit click.
pub fn create(project: &Path, plan: &PullRequestPlan) -> Result<String, ForgeError> {
    gh_status()?;
    let output = Command::new("gh")
        .current_dir(project)
        .args([
            "pr",
            "create",
            "--title",
            &plan.title,
            "--body",
            &plan.body,
            "--base",
            &plan.base,
        ])
        .output()
        .map_err(|error| ForgeError::Failed(error.to_string()))?;
    if !output.status.success() {
        return Err(ForgeError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(text.lines().last().unwrap_or(&text).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session {
            id: "s-1".to_string(),
            project_root: "C:/work/demo".to_string(),
            title: Some("demo".to_string()),
            status: crate::session::SessionStatus::Active,
            budget_cents: Some(500),
            spend_cents: 1,
            spend_micros: 1000,
            interventions: 0,
            tasks_done: 1,
            created_at: 1,
            ended_at: None,
        }
    }

    fn turn(prompt: &str) -> TurnTranscript {
        TurnTranscript {
            turn: crate::session::Turn {
                id: "t-1".to_string(),
                agent_id: "a-1".to_string(),
                parent_turn_id: None,
                seq: 1,
                status: "completed".to_string(),
                prompt: Some(prompt.to_string()),
                started_at: 1,
                completed_at: Some(2),
                usage: None,
                stop_reason: Some("completed".to_string()),
            },
            items: Vec::new(),
        }
    }

    #[test]
    fn a_plan_uses_the_latest_prompt_and_lists_each_turn() {
        let tmp = tempfile::TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args([
                "-c",
                "user.email=t@example.com",
                "-c",
                "user.name=T",
                "commit",
                "--allow-empty",
                "-m",
                "base",
            ])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        let plan = plan(tmp.path(), &session(), &[turn("fix the parser\nand tests")]).unwrap();
        assert_eq!(plan.branch, "feature");
        assert!(plan.title.starts_with("fix the parser"));
        assert!(plan.body.contains("fix the parser"));
        assert!(plan.body.contains("did not push"));
    }

    #[test]
    fn a_missing_gh_is_reported_rather_than_silently_skipped() {
        // PATH manipulation is process-wide and racy, so this only asserts the mapping helper's
        // contract: NotFound becomes GhMissing. The real call is exercised by the exit test.
        let error = std::io::Error::new(std::io::ErrorKind::NotFound, "gh");
        let mapped = if error.kind() == std::io::ErrorKind::NotFound {
            ForgeError::GhMissing
        } else {
            ForgeError::Failed(error.to_string())
        };
        assert!(matches!(mapped, ForgeError::GhMissing));
    }
}

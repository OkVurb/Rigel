//! The Swarm inbox: the durable hand-off from an ordinary room message to Muse.
//!
//! One job is one JSON file. The write is atomic (temp + rename), so a consumer polling the
//! directory never reads a half-written prompt. The file survives a crash and is only moved by
//! the claim, complete and release transitions in the control API.
//!
//! The window already writes this shape from its Swarm preflight. This module is the same queue
//! reached from an ordinary chat message, so Muse is woken by the conversation itself instead of
//! requiring a separate preflight step.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};

use crate::agent::agentchat;

/// The same cap the window applies before queueing a Swarm prompt.
pub const MAX_PROMPT_CHARS: usize = 20_000;

/// What one room message did about Muse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kickoff {
    /// The clean room names the job targets, sorted. Empty when no job was queued.
    pub targets: Vec<String>,
    /// Configured, ready harnesses with no roster row at all.
    pub missing: Vec<String>,
    /// Base names registered more than once (a `-2` slot): the room is not clean.
    pub duplicates: Vec<String>,
    /// The queued job, or `None` when the room was not ready for Muse.
    pub job_id: Option<String>,
}

/// The registration without its duplicate slot: `claude-2` belongs to `claude`.
/// Only a trailing `-<digits>` is removed, so a harness whose name really contains a dash
/// (`rigel-free`) keeps it.
pub fn base_name(name: &str) -> &str {
    let Some((head, tail)) = name.rsplit_once('-') else {
        return name;
    };
    if !tail.is_empty() && tail.bytes().all(|byte| byte.is_ascii_digit()) {
        head
    } else {
        name
    }
}

/// The names the room reports as online. `/api/status` is a flat map of agent name to presence,
/// plus a `paused` room flag; only a row that says `available` is really in the room.
pub fn roster_names(status: &Value) -> Vec<String> {
    status
        .as_object()
        .map(|rows| {
            rows.iter()
                .filter(|(name, row)| {
                    name.as_str() != "paused"
                        && row.get("available").and_then(Value::as_bool) == Some(true)
                })
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Decide whether a room message may wake Muse, from the roster and the configured harnesses.
///
/// Every configured harness that is `ready` must be present. A duplicate (`claude` and
/// `claude-2`) is not a staffed room: it means two processes claimed one harness, and assigning
/// to either would leave the other unaccounted for. The caller reports the three lists instead
/// of guessing.
pub fn plan(roster: &[String], ready: &[String]) -> Kickoff {
    let mut missing = Vec::new();
    let mut duplicates = Vec::new();
    let mut targets = BTreeSet::new();
    for harness in ready {
        let matches = roster
            .iter()
            .filter(|name| base_name(name) == harness.as_str())
            .count();
        match matches {
            0 => missing.push(harness.clone()),
            1 => {
                targets.insert(harness.clone());
            }
            _ => duplicates.push(harness.clone()),
        }
    }
    Kickoff {
        targets: targets.into_iter().collect(),
        missing,
        duplicates,
        job_id: None,
    }
}

/// Write one Swarm job for Muse into `<data_dir>/swarm/inbox` and return its id.
pub fn enqueue(
    data_dir: &Path,
    project: &Path,
    prompt: &str,
    targets: &[String],
) -> Result<String, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("the Swarm prompt is empty".to_string());
    }
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(format!(
            "the Swarm prompt is longer than {MAX_PROMPT_CHARS} characters"
        ));
    }
    if !project.is_dir() {
        return Err("open a project before starting a swarm".to_string());
    }
    let mut unique = Vec::new();
    for target in targets {
        let target = target.trim();
        if target.is_empty() || unique.iter().any(|known| known == target) {
            continue;
        }
        if target.chars().count() > 40 || target.chars().any(char::is_control) {
            return Err(format!("{target} is not a valid room agent name"));
        }
        unique.push(target.to_string());
    }
    if unique.is_empty() {
        return Err("no ready room agent is online to take the swarm".to_string());
    }

    let inbox = data_dir.join("swarm").join("inbox");
    std::fs::create_dir_all(&inbox).map_err(|error| error.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let id = format!("swarm-{stamp}-{}", std::process::id());
    let body = json!({
        "id": id,
        "project": job_project(project),
        "prompt": prompt,
        "targets": unique,
        "lead": { "kind": "rigel" },
        "created_at": crate::migrate::now_ms(),
    });
    let path = inbox.join(format!("{id}.json"));
    let temp = inbox.join(format!("{id}.json.tmp"));
    std::fs::write(&temp, body.to_string()).map_err(|error| error.to_string())?;
    std::fs::rename(&temp, &path).map_err(|error| error.to_string())?;
    Ok(id)
}

/// The canonical project path on this host, without the Windows extended-length prefix. The job
/// is read by Muse on another machine, where `\\?\C:\...` is not a path it can open.
fn job_project(project: &Path) -> String {
    let text = project.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

/// Post the message's work to Muse when the room is fully staffed.
///
/// The room is read, not inferred: a harness that cannot start is absent from `configured`, and
/// one that is configured but not online is reported as `missing`. A job is written only when
/// `plan` finds no gap and no duplicate, so Muse never receives an assignment list that does not
/// match the room it will talk to.
pub async fn kick_off_muse(
    data_dir: &Path,
    project: &Path,
    prompt: &str,
) -> Result<Kickoff, String> {
    let status = agentchat::agents().await?;
    let roster = roster_names(&status);
    let project_text = project.to_string_lossy();
    let ready: Vec<String> = agentchat::configured(Some(&project_text))
        .into_iter()
        .filter(|harness| harness.ready)
        .map(|harness| harness.name)
        .collect();
    let mut plan = plan(&roster, &ready);
    if !plan.missing.is_empty() || !plan.duplicates.is_empty() {
        return Ok(plan);
    }
    plan.job_id = Some(enqueue(data_dir, project, prompt, &plan.targets)?);
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rigel-swarm-{tag}-{stamp}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_duplicate_slot_belongs_to_its_base_name() {
        assert_eq!(base_name("claude"), "claude");
        assert_eq!(base_name("claude-2"), "claude");
        assert_eq!(base_name("rigel-free"), "rigel-free");
        assert_eq!(base_name("agent-12"), "agent");
    }

    #[test]
    fn paused_and_unavailable_rows_are_not_online_agents() {
        let status = json!({
            "claude": { "available": true },
            "codex": { "available": false },
            "paused": false
        });
        assert_eq!(roster_names(&status), vec!["claude"]);
    }

    #[test]
    fn a_full_clean_room_plans_one_target_per_harness() {
        let roster = vec![
            "claude".to_string(),
            "codex".to_string(),
            "antigravity".to_string(),
        ];
        let ready = vec![
            "claude".to_string(),
            "codex".to_string(),
            "antigravity".to_string(),
        ];
        let plan = plan(&roster, &ready);
        assert!(plan.missing.is_empty());
        assert!(plan.duplicates.is_empty());
        assert_eq!(plan.targets, vec!["antigravity", "claude", "codex"]);
    }

    #[test]
    fn an_absent_or_duplicated_harness_blocks_the_kickoff() {
        let roster = vec!["claude".to_string(), "claude-2".to_string()];
        let ready = vec!["claude".to_string(), "antigravity".to_string()];
        let plan = plan(&roster, &ready);
        assert_eq!(plan.missing, vec!["antigravity"]);
        assert_eq!(plan.duplicates, vec!["claude"]);
        assert!(plan.targets.is_empty());
    }

    #[test]
    fn a_job_is_one_atomic_file_with_the_muse_lead() {
        let data = temp_dir("write");
        let project = temp_dir("project");
        let id = enqueue(
            &data,
            &project,
            "  wire the retry  ",
            &[
                "claude".to_string(),
                "claude".to_string(),
                "codex".to_string(),
            ],
        )
        .unwrap();
        let text =
            std::fs::read_to_string(data.join("swarm").join("inbox").join(format!("{id}.json")))
                .unwrap();
        let job: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(job["id"], id);
        assert_eq!(job["prompt"], "wire the retry");
        assert_eq!(job["lead"], json!({ "kind": "rigel" }));
        assert_eq!(job["targets"], json!(["claude", "codex"]));
        assert!(job["created_at"].as_i64().unwrap() > 0);
        assert!(
            !data
                .join("swarm")
                .join("inbox")
                .join(format!("{id}.json.tmp"))
                .exists()
        );
        std::fs::remove_dir_all(&data).ok();
        std::fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn an_empty_prompt_or_no_targets_is_refused() {
        let data = temp_dir("refuse");
        let project = temp_dir("refuse-project");
        assert!(enqueue(&data, &project, "   ", &["claude".to_string()]).is_err());
        assert!(enqueue(&data, &project, "work", &[]).is_err());
        std::fs::remove_dir_all(&data).ok();
        std::fs::remove_dir_all(&project).ok();
    }
}

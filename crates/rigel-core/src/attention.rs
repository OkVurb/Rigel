//! D154: what is waiting for a person, grouped by session.
//!
//! Only unresolved rows are read. A failed turn or a budget stop is resolved by a later
//! completed turn on the same agent, a claim by being released (or by going stale, which is the
//! unresolved state), and a merge review by being merged. Archived sessions are included: they
//! are hidden from the list, not from the work that still needs an answer.
//!
//! Approvals are not here. `PendingApproval` records a command or a mail body, not the turn that
//! asked, so attributing one to a session would be a guess; the app adds the open approvals as
//! one ungrouped block in front of these groups.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use sqlx::{Row, SqlitePool};

use crate::agent::approval::PendingApproval;
use crate::claims::STALE_AFTER;

#[derive(Debug, Clone, Serialize)]
pub struct AttentionItem {
    /// `failed_turn`, `budget_stop`, `stale_claim`, `merge_review` or `blocked_merge`.
    pub kind: String,
    pub session_id: String,
    pub label: String,
    pub detail: String,
    /// Milliseconds since the epoch: when the row became unresolved.
    pub at: i64,
    pub turn_id: Option<String>,
    pub agent_id: Option<String>,
    pub patch_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttentionGroup {
    pub session_id: String,
    pub title: String,
    pub status: String,
    pub archived: bool,
    pub items: Vec<AttentionItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Attention {
    pub groups: Vec<AttentionGroup>,
    pub total: usize,
}

impl Attention {
    /// D154: put the open approval questions in front of the ledger groups as one block.
    ///
    /// The question is the same surface every time, so it is not a session group of its own;
    /// each row still carries the session, agent and turn the asking surface recorded, which
    /// is what lets the view open the transcript row instead of the card. The bus is app-wide
    /// while this read is per project, so `PendingApproval::belongs_to` leaves out a question
    /// that names another project. That same rule scopes the Session card's pending read.
    pub fn with_approvals(mut self, pending: &[PendingApproval], project_root: &Path) -> Self {
        let visible = pending
            .iter()
            .filter(|pending| pending.belongs_to(project_root))
            .collect::<Vec<_>>();
        if visible.is_empty() {
            return self;
        }
        let items = visible
            .into_iter()
            .map(|pending| AttentionItem {
                kind: "approval".to_string(),
                session_id: pending.session_id.clone().unwrap_or_default(),
                label: pending.title.clone(),
                detail: format!("{} · waiting for an answer", pending.kind),
                at: pending.requested_at,
                turn_id: pending.turn_id.clone(),
                agent_id: pending.agent_id.clone(),
                patch_id: None,
            })
            .collect::<Vec<_>>();
        self.total += items.len();
        self.groups.insert(
            0,
            AttentionGroup {
                session_id: String::new(),
                title: "Waiting on you".to_string(),
                status: "active".to_string(),
                archived: false,
                items,
            },
        );
        self
    }
}

/// A session's display fields, repeated per row so the queries stay one join each.
struct SessionRow {
    id: String,
    title: String,
    status: String,
    archived: bool,
}

fn session_of(row: &sqlx::sqlite::SqliteRow, offset: &str) -> SessionRow {
    SessionRow {
        id: row.get(format!("{offset}id").as_str()),
        title: row.get(format!("{offset}title").as_str()),
        status: row.get(format!("{offset}status").as_str()),
        archived: row
            .try_get::<Option<i64>, _>(format!("{offset}archived_at").as_str())
            .ok()
            .flatten()
            .is_some(),
    }
}

/// Read the project's unresolved work. `now` is passed so a test can make a claim stale without
/// sleeping; the app passes [`crate::migrate::now_ms`].
pub async fn attention(pool: &SqlitePool, now: i64) -> Result<Attention, sqlx::Error> {
    let mut groups: BTreeMap<String, (SessionRow, Vec<AttentionItem>)> = BTreeMap::new();

    fn push(
        groups: &mut BTreeMap<String, (SessionRow, Vec<AttentionItem>)>,
        session: SessionRow,
        item: AttentionItem,
    ) {
        groups
            .entry(session.id.clone())
            .or_insert_with(|| (session, Vec::new()))
            .1
            .push(item);
    }

    // A failed turn, or a turn the cap stopped, is unresolved until that agent completes a
    // later turn. Ending a session is not a retry, so it does not clear the row.
    let stalled = sqlx::query(
        "SELECT t.id AS turn_id, t.seq, t.status AS turn_status, t.stop_reason, t.completed_at,
                t.agent_id, a.role,
                s.id AS session_id, s.title AS session_title, s.status AS session_status,
                s.archived_at AS session_archived_at
         FROM turns t
         JOIN agents a ON a.id = t.agent_id
         JOIN sessions s ON s.id = a.session_id
         WHERE (t.status = 'failed' OR t.stop_reason = 'budget_exceeded')
           AND NOT EXISTS (
             SELECT 1 FROM turns later
              WHERE later.agent_id = t.agent_id
                AND later.seq > t.seq
                AND later.status = 'completed'
           )
         ORDER BY t.completed_at DESC, t.seq DESC",
    )
    .fetch_all(pool)
    .await?;
    for row in stalled {
        let session = session_of(&row, "session_");
        let turn_id: String = row.get("turn_id");
        let agent_id: String = row.get("agent_id");
        let seq: i64 = row.get("seq");
        let role: Option<String> = row.get("role");
        let stop: Option<String> = row.get("stop_reason");
        let status: String = row.get("turn_status");
        let at: i64 = row.get("completed_at");
        let budget = stop.as_deref() == Some("budget_exceeded");
        let who = role.unwrap_or_else(|| agent_id[..agent_id.len().min(8)].to_string());
        let (kind, label, detail) = if budget {
            (
                "budget_stop",
                format!("Budget stop on turn {seq}"),
                format!("{who} · the cap interrupted this turn; raise it or send a smaller task"),
            )
        } else {
            (
                "failed_turn",
                format!("Turn {seq} failed"),
                format!("{who} · no later turn on this agent completed"),
            )
        };
        push(
            &mut groups,
            session,
            AttentionItem {
                kind: kind.to_string(),
                session_id: row.get("session_id"),
                label,
                detail: if status == "failed" {
                    detail
                } else {
                    format!("{detail} ({status})")
                },
                at,
                turn_id: Some(turn_id),
                agent_id: Some(agent_id),
                patch_id: None,
            },
        );
    }

    // A claim nobody is heartbeating is the only stale signal the ledger has. Releasing it
    // clears the row; the Attention view links to Diff, where the release is one click.
    let stale_at = now - STALE_AFTER.as_millis() as i64;
    let stale = sqlx::query(
        "SELECT c.id AS claim_id, c.path, c.heartbeat_at, c.agent_id,
                a.role, s.id AS session_id, s.title AS session_title, s.status AS session_status,
                s.archived_at AS session_archived_at
         FROM claims c
         JOIN agents a ON a.id = c.agent_id
         JOIN sessions s ON s.id = a.session_id
         WHERE c.released_at IS NULL AND c.heartbeat_at < ?
         ORDER BY c.heartbeat_at",
    )
    .bind(stale_at)
    .fetch_all(pool)
    .await?;
    for row in stale {
        let session = session_of(&row, "session_");
        let agent_id: String = row.get("agent_id");
        let role: Option<String> = row.get("role");
        let path: String = row.get("path");
        let who = role.unwrap_or_else(|| agent_id[..agent_id.len().min(8)].to_string());
        push(
            &mut groups,
            session,
            AttentionItem {
                kind: "stale_claim".to_string(),
                session_id: row.get("session_id"),
                label: format!("Stale claim on {path}"),
                detail: format!("{who} · heartbeat stopped; release it in Diff"),
                at: row.get("heartbeat_at"),
                turn_id: None,
                agent_id: Some(agent_id),
                patch_id: None,
            },
        );
    }

    // An approved review that never landed can be retried; a blocked one needs a reason.
    // An override note means the human already decided, so the merge is the only thing left.
    let reviews = sqlx::query(
        "SELECT r.id AS review_id, r.agent_id, r.verdict, r.reason, r.files_json,
                r.created_at, a.role,
                s.id AS session_id, s.title AS session_title, s.status AS session_status,
                s.archived_at AS session_archived_at
         FROM merge_reviews r
         JOIN sessions s ON s.id = r.session_id
         LEFT JOIN agents a ON a.id = r.agent_id
         WHERE r.merged = 0 AND r.override_note IS NULL
         ORDER BY r.created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    for row in reviews {
        let session = session_of(&row, "session_");
        let agent_id: String = row.get("agent_id");
        let role: Option<String> = row.get("role");
        let verdict: String = row.get("verdict");
        let files: Vec<String> = row
            .try_get::<String, _>("files_json")
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let who = role.unwrap_or_else(|| agent_id[..agent_id.len().min(8)].to_string());
        let blocked = verdict == "blocked";
        push(
            &mut groups,
            session,
            AttentionItem {
                kind: if blocked {
                    "blocked_merge"
                } else {
                    "merge_review"
                }
                .to_string(),
                session_id: row.get("session_id"),
                label: if blocked {
                    format!("Blocked merge from {who}")
                } else {
                    format!("Unmerged review from {who}")
                },
                detail: format!(
                    "{} · {} file{}{}",
                    verdict,
                    files.len(),
                    if files.len() == 1 { "" } else { "s" },
                    row.try_get::<Option<String>, _>("reason")
                        .ok()
                        .flatten()
                        .map(|reason| format!(" · {reason}"))
                        .unwrap_or_default()
                ),
                at: row.get("created_at"),
                turn_id: None,
                agent_id: Some(agent_id),
                patch_id: None,
            },
        );
    }

    let mut groups: Vec<AttentionGroup> = groups
        .into_values()
        .map(|(session, mut items)| {
            items.sort_by_key(|a| std::cmp::Reverse(a.at));
            AttentionGroup {
                session_id: session.id,
                title: session.title,
                status: session.status,
                archived: session.archived,
                items,
            }
        })
        .filter(|group| !group.items.is_empty())
        .collect();
    groups.sort_by(|a, b| {
        let newest = |group: &AttentionGroup| group.items.first().map(|item| item.at).unwrap_or(0);
        newest(b)
            .cmp(&newest(a))
            .then_with(|| a.title.cmp(&b.title))
    });
    let total = groups.iter().map(|group| group.items.len()).sum();
    Ok(Attention { groups, total })
}

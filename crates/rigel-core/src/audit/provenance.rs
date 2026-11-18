//! Read one command item's recorded link chain (roadmap 84).
//!
//! This module never infers a missing link from timestamps. Each link is either `known` with
//! its value, or `unknown` with the reason the journal cannot supply it. A command that was not
//! destructive has no snapshot by design, and says so rather than showing an empty cell.

use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;

#[derive(Debug, thiserror::Error)]
pub enum ProvenanceError {
    #[error("item {0} is not in the journal")]
    UnknownItem(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

/// A value that is present, or explicitly absent with a reason. The shape is deliberately the
/// same for every link so the window and the CLI cannot disagree about what \"unknown\" means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link<T> {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl<T> Link<T> {
    pub fn known(value: T) -> Self {
        Self {
            status: "known",
            value: Some(value),
            reason: None,
        }
    }

    pub fn unknown(reason: impl Into<String>) -> Self {
        Self {
            status: "unknown",
            value: None,
            reason: Some(reason.into()),
        }
    }

    pub fn is_known(&self) -> bool {
        self.value.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolCallLink {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApprovalLink {
    pub id: String,
    pub decision: Option<String>,
    pub scope: Option<String>,
    pub requested_at: i64,
    pub decided_at: Option<i64>,
}

/// The chain for one command item. `turn`, `agent`, `model`, `step`, `tool_call`, `approval`
/// and `snapshot` are all separate links; a missing one is never collapsed into an empty string.
#[derive(Debug, Clone, Serialize)]
pub struct CommandProvenance {
    pub item_id: String,
    pub command: Link<String>,
    pub turn: Link<String>,
    pub agent: Link<String>,
    pub model: Link<String>,
    pub step: Link<u64>,
    pub tool_call: Link<ToolCallLink>,
    pub approval: Link<ApprovalLink>,
    pub snapshot: Link<String>,
}

fn text(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

type ApprovalRow = (Option<String>, Option<String>, i64, Option<i64>);

/// Read the chain for one command item. The item itself is the starting point; the turn and
/// agent come from the journal, the approval from the `approvals` row named in the tool result,
/// and the snapshot from the command result the tool wrote.
pub async fn command(
    pool: &SqlitePool,
    item_id: &str,
) -> Result<CommandProvenance, ProvenanceError> {
    let row: Option<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT i.turn_id, i.body_json, t.agent_id, a.model
         FROM items i
         JOIN turns t ON t.id = i.turn_id
         LEFT JOIN agents a ON a.id = t.agent_id
         WHERE i.id = ?",
    )
    .bind(item_id)
    .fetch_optional(pool)
    .await?;
    let (turn_id, body_json, agent_id, model) =
        row.ok_or_else(|| ProvenanceError::UnknownItem(item_id.to_string()))?;
    let body = serde_json::from_str::<Value>(&body_json).unwrap_or(Value::Null);
    let result = body.get("result").unwrap_or(&body);

    let command = match text(result.get("command")).or_else(|| text(body.get("command"))) {
        Some(command) => Link::known(command.to_string()),
        None => Link::unknown("the item does not contain a command"),
    };
    let step = match body.get("step").and_then(Value::as_u64) {
        Some(step) => Link::known(step),
        None => Link::unknown("the item predates model-step recording"),
    };
    let tool_call = match text(body.get("tool_call_id")) {
        Some(id) => Link::known(ToolCallLink {
            id: id.to_string(),
            name: text(body.get("tool")).map(str::to_string),
        }),
        None => Link::unknown("the item did not record its provider tool-call id"),
    };

    let approval_id = text(result.get("approval_id")).or_else(|| text(body.get("approval_id")));
    let approval = match approval_id {
        Some(id) => {
            let approval: Option<ApprovalRow> = sqlx::query_as(
                "SELECT decision, scope, requested_at, decided_at
                     FROM approvals WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?;
            match approval {
                Some((decision, scope, requested_at, decided_at)) => Link::known(ApprovalLink {
                    id: id.to_string(),
                    decision,
                    scope,
                    requested_at,
                    decided_at,
                }),
                None => Link::unknown(format!(
                    "approval {id} is named by the item but has no approvals row"
                )),
            }
        }
        None => Link::unknown("the command item did not record an approval id"),
    };

    let snapshot_id = result
        .get("snapshot")
        .and_then(|snapshot| snapshot.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());
    let destructive = result
        .get("destructive")
        .and_then(Value::as_array)
        .map(|rows| !rows.is_empty())
        .unwrap_or(false);
    let snapshot = match snapshot_id {
        Some(id) => Link::known(id.to_string()),
        None if !destructive => {
            Link::unknown("no snapshot was taken: the command was not destructive")
        }
        None => Link::unknown("the destructive command did not record a snapshot"),
    };

    Ok(CommandProvenance {
        item_id: item_id.to_string(),
        command,
        turn: Link::known(turn_id),
        agent: Link::known(agent_id),
        model: match model {
            Some(model) => Link::known(model),
            None => Link::unknown("the agent row has no model"),
        },
        step,
        tool_call,
        approval,
        snapshot,
    })
}

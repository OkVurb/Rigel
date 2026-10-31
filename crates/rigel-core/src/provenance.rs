//! D156: read one patch's recorded chain back — patch → turn → item → tool → agent → model.
//!
//! The chain is exactly what the database recorded. Where a link is missing the answer is
//! `None` and the window prints "unknown"; this module never infers a cause from timestamps,
//! and it never writes. A hunk is as far down as the journal goes: the `patch_hunks` row names
//! the lines, and the item that carried the write names the tool that made it.

use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, thiserror::Error)]
pub enum ProvenanceError {
    #[error("patch {0} is not in the journal")]
    UnknownPatch(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

/// One patch, with every recorded link back to the tool call that wrote it.
#[derive(Debug, Clone, Serialize)]
pub struct Provenance {
    pub patch_id: String,
    pub path: String,
    pub status: String,
    pub created_at: i64,
    pub applied_at: Option<i64>,
    pub agent_id: String,
    pub agent_role: Option<String>,
    pub harness: Option<String>,
    pub model: Option<String>,
    /// The turn that wrote the patch. `None` means the journal has no item for it.
    pub turn_id: Option<String>,
    pub turn_seq: Option<i64>,
    pub turn_status: Option<String>,
    pub item_id: Option<String>,
    pub item_kind: Option<String>,
    /// The tool the model called, as recorded in the item (`edit_file`, `write_file`).
    pub tool: Option<String>,
    /// True only when patch, item, turn and agent were all found; otherwise the window says
    /// "unknown" for the links that are missing rather than guessing.
    pub complete: bool,
}

/// Read the chain for one patch. A patch that is not in the journal is an error, not a guess.
pub async fn patch(pool: &SqlitePool, patch_id: &str) -> Result<Provenance, ProvenanceError> {
    let row = sqlx::query(
        "SELECT p.path, p.status, p.created_at, p.applied_at, p.agent_id, a.role, a.harness, \
                a.model \
         FROM patches p LEFT JOIN agents a ON a.id = p.agent_id WHERE p.id = ?",
    )
    .bind(patch_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ProvenanceError::UnknownPatch(patch_id.to_string()))?;

    use sqlx::Row;
    let agent_id: String = row.get("agent_id");
    let agent_known: Option<String> = row.try_get("harness").ok();

    // The item that carried the write. Matched on the recorded result, which is the only link
    // the transcript actually stores; the newest wins when a patch was somehow recorded twice.
    let item: Option<(String, String, String, String)> = sqlx::query_as(
        "SELECT i.id, i.turn_id, i.kind, i.body_json FROM items i \
         WHERE json_extract(i.body_json, '$.result.patch_id') = ? \
         ORDER BY i.created_at DESC, i.seq DESC LIMIT 1",
    )
    .bind(patch_id)
    .fetch_optional(pool)
    .await?;

    let tool = item.as_ref().and_then(|(_, _, _, body)| {
        serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|value| {
                value
                    .get("tool")
                    .and_then(|tool| tool.as_str())
                    .map(str::to_string)
            })
    });

    let turn = match item.as_ref() {
        Some((_, turn_id, _, _)) => {
            sqlx::query_as::<_, (i64, String)>("SELECT seq, status FROM turns WHERE id = ?")
                .bind(turn_id)
                .fetch_optional(pool)
                .await?
        }
        None => None,
    };

    let complete = item.is_some() && turn.is_some() && agent_known.is_some();
    let (turn_id, item_id, item_kind) = match item {
        Some((id, turn_id, kind, _)) => (Some(turn_id), Some(id), Some(kind)),
        None => (None, None, None),
    };

    Ok(Provenance {
        patch_id: patch_id.to_string(),
        path: row.get("path"),
        status: row.get("status"),
        created_at: row.get("created_at"),
        applied_at: row.get("applied_at"),
        agent_id,
        agent_role: row.get("role"),
        harness: agent_known,
        model: row.get("model"),
        turn_id,
        turn_seq: turn.as_ref().map(|(seq, _)| *seq),
        turn_status: turn.map(|(_, status)| status),
        item_id,
        item_kind,
        tool,
        complete,
    })
}

//! Hunk comments: a sentence attached to one hunk, sent back to the agent that wrote it (D88).
//!
//! The diff is read-only. A comment never edits the patch, never changes its status and never
//! applies anything; it becomes a turn for the hunk's author, and `delivered_turn_id` is the
//! record that it did. A comment with no delivery is pending, which is what the roster counts.
//!
//! Two names are kept apart on purpose: `agent_id` is the hunk's author (who is being asked),
//! `author` is who wrote the comment (`user`, or another agent reviewing the merge).

use serde::Serialize;
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use uuid::Uuid;

use crate::migrate::now_ms;

/// A comment longer than this is a task, not a note. The cap is characters, not bytes, so a
/// multi-byte sentence is not cut mid-character.
pub const MAX_BODY_CHARS: usize = 4000;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HunkComment {
    pub id: String,
    pub hunk_id: String,
    pub patch_id: String,
    pub path: String,
    pub header: String,
    /// The hunk's author: the agent this comment is for.
    pub agent_id: String,
    /// `user`, or the id of the agent that wrote the comment.
    pub author: String,
    pub body: String,
    pub created_at: i64,
    pub delivered_turn_id: Option<String>,
    pub delivered_at: Option<i64>,
}

impl HunkComment {
    pub fn pending(&self) -> bool {
        self.delivered_at.is_none()
    }

    /// The prompt a delivery sends. The header is quoted so the agent knows which lines are
    /// being discussed; the body is the user's words, unedited.
    pub fn prompt(&self) -> String {
        format!(
            "Comment on {} {}\n\n{}",
            self.path,
            self.header.trim(),
            self.body
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommentError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("no hunk {0} in this project")]
    UnknownHunk(String),

    #[error("no comment {0}")]
    Unknown(String),

    #[error("a comment needs a sentence")]
    Empty,
}

type Result<T> = std::result::Result<T, CommentError>;

const COLUMNS: &str = "id, hunk_id, patch_id, path, header, agent_id, author, body, created_at, \
                       delivered_turn_id, delivered_at";

fn from_row(row: &SqliteRow) -> HunkComment {
    HunkComment {
        id: row.get("id"),
        hunk_id: row.get("hunk_id"),
        patch_id: row.get("patch_id"),
        path: row.get("path"),
        header: row.get("header"),
        agent_id: row.get("agent_id"),
        author: row.get("author"),
        body: row.get("body"),
        created_at: row.get("created_at"),
        delivered_turn_id: row.get("delivered_turn_id"),
        delivered_at: row.get("delivered_at"),
    }
}

pub struct HunkComments {
    pool: SqlitePool,
}

impl HunkComments {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Attach a comment to a hunk. The author is copied from the hunk row, so a comment can
    /// never be addressed to an agent that did not write those lines.
    pub async fn add(&self, hunk_id: &str, body: &str, author: &str) -> Result<HunkComment> {
        let body = body.trim();
        if body.is_empty() {
            return Err(CommentError::Empty);
        }
        let body: String = body.chars().take(MAX_BODY_CHARS).collect();

        let hunk: Option<(String, String, String, String)> =
            sqlx::query_as("SELECT patch_id, path, header, agent_id FROM patch_hunks WHERE id = ?")
                .bind(hunk_id)
                .fetch_optional(&self.pool)
                .await?;
        let Some((patch_id, path, header, agent_id)) = hunk else {
            return Err(CommentError::UnknownHunk(hunk_id.to_string()));
        };

        let comment = HunkComment {
            id: Uuid::new_v4().to_string(),
            hunk_id: hunk_id.to_string(),
            patch_id,
            path,
            header,
            agent_id,
            author: if author.trim().is_empty() {
                "user".to_string()
            } else {
                author.trim().to_string()
            },
            body,
            created_at: now_ms(),
            delivered_turn_id: None,
            delivered_at: None,
        };

        sqlx::query(
            "INSERT INTO hunk_comments
               (id, hunk_id, patch_id, path, header, agent_id, author, body, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&comment.id)
        .bind(&comment.hunk_id)
        .bind(&comment.patch_id)
        .bind(&comment.path)
        .bind(&comment.header)
        .bind(&comment.agent_id)
        .bind(&comment.author)
        .bind(&comment.body)
        .bind(comment.created_at)
        .execute(&self.pool)
        .await?;

        Ok(comment)
    }

    pub async fn get(&self, id: &str) -> Result<Option<HunkComment>> {
        let sql = format!("SELECT {COLUMNS} FROM hunk_comments WHERE id = ?");
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.as_ref().map(from_row))
    }

    pub async fn for_patch(&self, patch_id: &str) -> Result<Vec<HunkComment>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM hunk_comments WHERE patch_id = ? ORDER BY created_at, id"
        );
        let rows = sqlx::query(&sql)
            .bind(patch_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(from_row).collect())
    }

    /// Every comment in a session, for the Diff tab. Newest last, so it reads like a thread.
    pub async fn for_session(&self, session_id: &str) -> Result<Vec<HunkComment>> {
        let sql = format!(
            "SELECT {} FROM hunk_comments c JOIN patches p ON p.id = c.patch_id
             WHERE p.session_id = ?
             ORDER BY c.created_at, c.id",
            COLUMNS
                .split(", ")
                .map(|column| format!("c.{column}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let rows = sqlx::query(&sql)
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(from_row).collect())
    }

    /// What an agent still owes an answer to, oldest first.
    pub async fn pending_for(&self, agent_id: &str) -> Result<Vec<HunkComment>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM hunk_comments
              WHERE agent_id = ? AND delivered_at IS NULL
              ORDER BY created_at, id"
        );
        let rows = sqlx::query(&sql)
            .bind(agent_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(from_row).collect())
    }

    /// Record that a comment went out. Delivering twice is a no-op that keeps the first turn:
    /// the id is the evidence, and rewriting it would orphan the turn that carried the words.
    pub async fn deliver(&self, id: &str, turn_id: &str) -> Result<HunkComment> {
        sqlx::query(
            "UPDATE hunk_comments
                SET delivered_turn_id = COALESCE(delivered_turn_id, ?),
                    delivered_at = COALESCE(delivered_at, ?)
              WHERE id = ?",
        )
        .bind(turn_id)
        .bind(now_ms())
        .bind(id)
        .execute(&self.pool)
        .await?;

        self.get(id)
            .await?
            .ok_or_else(|| CommentError::Unknown(id.to_string()))
    }
}

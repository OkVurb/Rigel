//! One read over the workspace's own history: sessions, turns and patches.
//!
//! D147's palette is a renderer over this module; the window owns no search state. Queries are
//! bound, never built into SQL, and `%` / `_` / `\` are escaped so a query is text rather than a
//! pattern. FTS would be faster, but the project database is small enough that `LIKE` over the
//! indexed columns is honest and needs no second index to keep in step.
//!
//! Results are capped per section by the caller. The current session sorts first and recency
//! breaks every tie; the palette is for returning to what you were doing, not for a global rank.

use std::collections::HashSet;

use serde::Serialize;
use sqlx::SqlitePool;

/// One section's cap is chosen by the caller; these bounds keep a bad bind from asking for the
/// whole history.
pub const MAX_HITS: usize = 25;

/// A matching item with the columns the palette needs from its turn and agent: turn id, seq,
/// prompt, session id, role, harness, started_at, body.
type ItemHitRow = (String, i64, String, String, String, String, i64, String);

#[derive(Debug, Clone, Serialize)]
pub struct SessionHit {
    pub id: String,
    pub title: String,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnHit {
    pub session_id: String,
    pub turn_id: String,
    pub seq: i64,
    pub role: String,
    pub harness: String,
    pub prompt: String,
    /// The prompt, or the first item body that matched when the prompt did not.
    pub snippet: String,
    /// True when the match came from an item rather than the prompt.
    pub from_item: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatchHit {
    pub id: String,
    pub session_id: String,
    pub path: String,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WorkspaceHits {
    pub sessions: Vec<SessionHit>,
    pub turns: Vec<TurnHit>,
    pub patches: Vec<PatchHit>,
}

/// Search the project's own rows. An empty query is the palette's default state: recent
/// sessions and patches, no turns (a turn list with no query is just the newest transcript).
pub async fn workspace(
    pool: &SqlitePool,
    query: &str,
    current_session: Option<&str>,
    limit: usize,
) -> Result<WorkspaceHits, sqlx::Error> {
    let limit = limit.clamp(1, MAX_HITS) as i64;
    let query = query.trim();
    let pattern = like_pattern(query);
    let current = current_session.unwrap_or("");

    let mut hits = WorkspaceHits::default();

    if query.is_empty() {
        let sessions: Vec<(String, String, String, i64)> = sqlx::query_as(
            "SELECT id, COALESCE(title, ''), status, created_at
             FROM sessions ORDER BY created_at DESC, id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        hits.sessions = sessions
            .into_iter()
            .map(|(id, title, status, created_at)| SessionHit {
                id,
                title,
                status,
                created_at,
            })
            .collect();

        let patches: Vec<(String, String, String, String, i64)> = sqlx::query_as(
            "SELECT id, session_id, path, status, created_at
             FROM patches ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        hits.patches = patches
            .into_iter()
            .map(|(id, session_id, path, status, created_at)| PatchHit {
                id,
                session_id,
                path,
                status,
                created_at,
            })
            .collect();
    } else {
        let sessions: Vec<(String, String, String, i64)> = sqlx::query_as(
            "SELECT id, COALESCE(title, ''), status, created_at
             FROM sessions
             WHERE COALESCE(title, '') LIKE ? ESCAPE '\\' OR id = ?
             ORDER BY created_at DESC, id DESC LIMIT ?",
        )
        .bind(&pattern)
        .bind(query)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        hits.sessions = sessions
            .into_iter()
            .map(|(id, title, status, created_at)| SessionHit {
                id,
                title,
                status,
                created_at,
            })
            .collect();

        // Prompt matches first: they need no body scan and are what a palette is usually for.
        let prompt_rows: Vec<(String, i64, String, String, String, String, i64)> = sqlx::query_as(
            "SELECT t.id, t.seq, COALESCE(t.prompt, ''), a.session_id,
                        COALESCE(a.role, ''), a.harness, t.started_at
                 FROM turns t JOIN agents a ON a.id = t.agent_id
                 WHERE COALESCE(t.parent_turn_id, '') = ''
                   AND COALESCE(t.prompt, '') LIKE ? ESCAPE '\\'
                 ORDER BY t.started_at DESC LIMIT ?",
        )
        .bind(&pattern)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let mut seen: HashSet<String> = HashSet::new();
        let mut turns: Vec<TurnHit> = prompt_rows
            .into_iter()
            .map(
                |(turn_id, seq, prompt, session_id, role, harness, created_at)| {
                    seen.insert(turn_id.clone());
                    TurnHit {
                        session_id,
                        turn_id,
                        seq,
                        role,
                        harness,
                        snippet: snippet(&prompt, query),
                        prompt,
                        from_item: false,
                        created_at,
                    }
                },
            )
            .collect();

        // Item bodies are searched only in the 500 most recent top-level turns. `LIKE '%x%'`
        // cannot use an index, and a project with thousands of tool results must not turn every
        // keystroke into a full scan. Older turns stay findable by their prompt.
        let remaining = limit.saturating_sub(turns.len() as i64);
        if remaining > 0 {
            let item_rows: Vec<ItemHitRow> = sqlx::query_as(
                "SELECT t.id, t.seq, COALESCE(t.prompt, ''), a.session_id,
                        COALESCE(a.role, ''), a.harness, t.started_at, i.body_json
                 FROM items i
                 JOIN turns t ON t.id = i.turn_id
                 JOIN agents a ON a.id = t.agent_id
                 WHERE COALESCE(t.parent_turn_id, '') = ''
                   AND t.id IN (SELECT id FROM turns
                                 WHERE COALESCE(parent_turn_id, '') = ''
                                 ORDER BY started_at DESC LIMIT 500)
                   AND i.body_json LIKE ? ESCAPE '\\'
                 ORDER BY t.started_at DESC, i.seq ASC LIMIT 200",
            )
            .bind(&pattern)
            .fetch_all(pool)
            .await?;

            for (turn_id, seq, prompt, session_id, role, harness, created_at, body) in item_rows {
                if !seen.insert(turn_id.clone()) {
                    continue;
                }
                turns.push(TurnHit {
                    session_id,
                    turn_id,
                    seq,
                    role,
                    harness,
                    snippet: snippet(&body, query),
                    prompt,
                    from_item: true,
                    created_at,
                });
                if turns.len() as i64 >= limit {
                    break;
                }
            }
        }
        hits.turns = turns;

        let patches: Vec<(String, String, String, String, i64)> = sqlx::query_as(
            "SELECT id, session_id, path, status, created_at
             FROM patches
             WHERE path LIKE ? ESCAPE '\\' OR diff LIKE ? ESCAPE '\\'
             ORDER BY created_at DESC LIMIT ?",
        )
        .bind(&pattern)
        .bind(&pattern)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        hits.patches = patches
            .into_iter()
            .map(|(id, session_id, path, status, created_at)| PatchHit {
                id,
                session_id,
                path,
                status,
                created_at,
            })
            .collect();
    }

    // The session you are in first, then recency. This is a stable two-key sort, not a score.
    hits.sessions.sort_by(|a, b| {
        (a.id != current)
            .cmp(&(b.id != current))
            .then(b.created_at.cmp(&a.created_at))
    });
    hits.turns.sort_by(|a, b| {
        (a.session_id != current)
            .cmp(&(b.session_id != current))
            .then(b.created_at.cmp(&a.created_at))
    });
    hits.patches.sort_by(|a, b| {
        (a.session_id != current)
            .cmp(&(b.session_id != current))
            .then(b.created_at.cmp(&a.created_at))
    });

    Ok(hits)
}

/// Escape LIKE's metacharacters so a query is text. The caller always supplies `ESCAPE '\'`.
fn like_pattern(query: &str) -> String {
    let mut escaped = String::with_capacity(query.len() + 2);
    escaped.push('%');
    for character in query.chars() {
        match character {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(character);
            }
            other => escaped.push(other),
        }
    }
    escaped.push('%');
    escaped
}

/// A short, one-line window around the first match. JSON bodies are not pretty, but a snippet
/// that names the file or the sentence is enough to recognise the turn.
fn snippet(text: &str, needle: &str) -> String {
    let flattened: String = text
        .chars()
        .map(|character| {
            if character.is_whitespace() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let trimmed = flattened.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Lowercasing can change a byte length; only use the folded string to find a starting point,
    // then walk the original to a character boundary before slicing.
    let folded = trimmed.to_lowercase();
    let position = folded
        .find(&needle.to_lowercase())
        .unwrap_or(0)
        .min(trimmed.len());
    let mut start = position.saturating_sub(48);
    while start > 0 && !trimmed.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (position + needle.len().max(72)).min(trimmed.len());
    while end < trimmed.len() && !trimmed.is_char_boundary(end) {
        end += 1;
    }
    let body = trimmed[start..end].trim();
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(body);
    if end < trimmed.len() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_metacharacters_are_escaped() {
        assert_eq!(like_pattern("100%"), "%100\\%%");
        assert_eq!(like_pattern("a_b"), "%a\\_b%");
        assert_eq!(like_pattern("c:\\tmp"), "%c:\\\\tmp%");
    }

    #[test]
    fn a_snippet_is_one_line_and_bounded() {
        let text = "line one\nline two with the needle in the middle\nline three";
        let out = snippet(text, "needle");
        assert!(out.contains("needle"));
        assert!(!out.contains('\n'));
        assert!(out.chars().count() < 140);
    }
}

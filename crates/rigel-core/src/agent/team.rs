//! The session work board and the file-scoped assignments on it.
//!
//! The coordinator is the "big boss": it splits a job and gives each helper a role and the
//! exact project files that helper may read. The board is persisted with the session and
//! rendered into every agent prompt, so a child knows what its siblings hold and states its
//! own files in its answer. A native child is additionally enforced by the tools' scope.

use std::collections::HashSet;
use std::path::{Component, Path};

use crate::session::TeamAgent;

/// More than a handful of files is not a focused assignment.
pub const MAX_FILES: usize = 8;
pub const MAX_TASK_CHARS: usize = 400;
pub const MAX_ROLE_CHARS: usize = 40;

/// Resolve the files a coordinator named into canonical project-relative paths.
///
/// Every path must already exist, stay inside the project, and name a file rather than a
/// directory. Validation happens before any child is registered or any chat message is sent,
/// so a bad split costs nothing.
pub fn scope(root: &Path, files: &[String]) -> Result<Vec<String>, String> {
    if files.is_empty() {
        return Err("files must name at least one project file".to_string());
    }
    if files.len() > MAX_FILES {
        return Err(format!("at most {MAX_FILES} files per assignment"));
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("project root is not readable: {error}"))?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in files {
        let rel = raw.trim().replace('\\', "/");
        if rel.is_empty() {
            return Err("a file path is empty".to_string());
        }
        let path = Path::new(&rel);
        if path.is_absolute()
            || rel.contains(':')
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
        {
            return Err(format!("{raw} is not a project-relative file"));
        }
        let canonical = root
            .join(path)
            .canonicalize()
            .map_err(|_| format!("{raw} does not exist in the project"))?;
        if !canonical.starts_with(&root) {
            return Err(format!("{raw} is outside the project"));
        }
        if !canonical.is_file() {
            return Err(format!("{raw} is not a file"));
        }
        let relative = canonical
            .strip_prefix(&root)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or(rel);
        if seen.insert(relative.clone()) {
            out.push(relative);
        }
    }
    Ok(out)
}

/// The board as it appears in a prompt. `me` marks the reader's own row.
pub fn board(rows: &[TeamAgent], me: Option<&str>) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut text = String::from(
        "Work board for this session (the coordinator split it; each helper may read only the \
         files assigned to it):\n",
    );
    for row in rows {
        let role = row.role.as_deref().unwrap_or("agent");
        let marker = if Some(row.agent_key.as_str()) == me {
            " (you)"
        } else {
            ""
        };
        text.push_str(&format!(
            "- {}{} [{}] files: {} — status: {}\n",
            row.agent_key,
            marker,
            role,
            if row.files.is_empty() {
                "none".to_string()
            } else {
                row.files.join(", ")
            },
            row.status
        ));
        if !row.task.is_empty() {
            text.push_str(&format!("  task: {}\n", row.task));
        }
    }
    text
}

/// The child's own assignment, stated before the board so it cannot be missed.
pub fn assignment(row: &TeamAgent) -> String {
    format!(
        "Your assignment from the coordinator:\n\
         Role: {}\n\
         Files you may read (and only these): {}\n\
         Task: {}\n\
         Start your final answer with \"Working on: {}\". If the task needs a file that is not \
         on this list, name it instead of reading it; the coordinator can reassign it.",
        row.role.as_deref().unwrap_or("agent"),
        if row.files.is_empty() {
            "none".to_string()
        } else {
            row.files.join(", ")
        },
        row.task,
        if row.files.is_empty() {
            "none".to_string()
        } else {
            row.files.join(", ")
        }
    )
}

/// The message the coordinator posts to the chat room for a room agent.
pub fn room_message(row: &TeamAgent) -> String {
    format!(
        "@{} — role: {}\nFiles you may read (and only these): {}\nTask: {}\nStart your reply \
         with \"Working on: {}\". If you need another file, name it instead of reading it.",
        row.agent_key,
        row.role.as_deref().unwrap_or("agent"),
        if row.files.is_empty() {
            "none".to_string()
        } else {
            row.files.join(", ")
        },
        row.task,
        if row.files.is_empty() {
            "none".to_string()
        } else {
            row.files.join(", ")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_names_files_and_marks_its_reader() {
        let rows = vec![
            TeamAgent {
                agent_key: "looker".to_string(),
                role: Some("reader".to_string()),
                kind: "native".to_string(),
                files: vec!["README.md".to_string()],
                task: "summarise".to_string(),
                status: "assigned".to_string(),
            },
            TeamAgent {
                agent_key: "claude".to_string(),
                role: Some("reviewer".to_string()),
                kind: "room".to_string(),
                files: vec!["docs/handoff.md".to_string()],
                task: "check".to_string(),
                status: "working".to_string(),
            },
        ];
        let text = board(&rows, Some("looker"));
        assert!(text.contains("looker (you) [reader] files: README.md"));
        assert!(text.contains("claude [reviewer] files: docs/handoff.md"));
        assert!(text.contains("status: working"));
    }

    #[test]
    fn scope_rejects_missing_and_parent_paths() {
        let root = std::env::temp_dir().join(format!("rigel-team-{}", std::process::id()));
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/one.md"), "one").unwrap();
        assert!(scope(&root, &["docs/one.md".to_string()]).is_ok());
        assert!(scope(&root, &["docs/missing.md".to_string()]).is_err());
        assert!(scope(&root, &["../escape.md".to_string()]).is_err());
        assert!(scope(&root, &["docs".to_string()]).is_err());
        std::fs::remove_dir_all(&root).ok();
    }
}

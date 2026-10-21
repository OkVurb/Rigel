//! Committed memory, and the validator that keeps it safe to commit (P3.6, I7).
//!
//! Sessions are local because they contain prompts and machine paths; memory is committed
//! because it is the distilled thing that should travel. That split only holds if memory
//! cannot write a machine path or a secret: a committed file is forever, and a secret in git
//! is a secret leaked. So every write passes [`validate`], and a rejection names the pattern
//! and the line — never the matched content, which would put the secret in the error path.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::migrate::now_ms;

/// §7.1's per-write cap.
pub const MAX_CONTENT: usize = 8_000;

pub const MEMORY_FILE: &str = "MEMORY.md";

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory would leak a machine path ({pattern}) on line {line}")]
    PathLeak { pattern: &'static str, line: usize },

    #[error("memory would leak a secret ({pattern}) on line {line}")]
    Secret { pattern: &'static str, line: usize },

    #[error("topic {0:?} must match ^[a-z0-9-]{{1,48}}$")]
    InvalidTopic(String),

    #[error("agent id {0:?} is not a safe directory name")]
    InvalidAgent(String),

    #[error("memory content is {size} bytes; the limit is {MAX_CONTENT}")]
    TooLarge { size: usize },

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl MemoryError {
    /// A leak is a policy refusal, not a bug in the caller's arguments.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::PathLeak { .. } | Self::Secret { .. } => 3,
            Self::InvalidTopic(_) | Self::InvalidAgent(_) | Self::TooLarge { .. } => 1,
            Self::Io(_) | Self::Database(_) | Self::Json(_) => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryFile {
    pub agent_id: String,
    pub topic: String,
    /// Project-relative, so the record itself carries no machine path.
    pub path: String,
    pub sha256: String,
    pub words: usize,
    pub bytes: usize,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    Append,
    Replace,
}

impl WriteMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "append" => Some(Self::Append),
            "replace" => Some(Self::Replace),
            _ => None,
        }
    }
}

/// Patterns from I7 and §7.1. The list is conservative: it is better to reject a sentence
/// that mentions `/home/` than to commit one that contains a real home directory.
const PATH_PATTERNS: &[(&str, &str)] = &[
    ("drive-letter", "C:\\"),
    ("unc", "\\\\"),
    ("users", "/Users/"),
    ("home", "/home/"),
    ("root-home", "/root/"),
    ("tilde-home", "~/"),
];

const SECRET_PATTERNS: &[(&str, &str)] = &[
    ("openai-key", "sk-"),
    ("stripe-key", "sk_live_"),
    ("github-token", "ghp_"),
    ("github-pat", "github_pat_"),
    ("gitlab-token", "glpat-"),
    ("slack-token", "xoxb-"),
    ("slack-token", "xoxp-"),
    ("aws-key", "AKIA"),
    ("ssh-public-key", "ssh-rsa"),
    ("ssh-public-key", "ssh-ed25519"),
    ("private-key", "BEGIN RSA PRIVATE KEY"),
    ("private-key", "BEGIN OPENSSH PRIVATE KEY"),
    ("private-key", "BEGIN EC PRIVATE KEY"),
];

/// Assignments catch the general case the prefixes miss: `OPENROUTER_API_KEY=…`.
const ASSIGNMENT_HINTS: &[&str] = &["api_key=", "apikey=", "token=", "secret=", "password="];

/// Reject anything that would put a machine path or a credential into a committed file.
pub fn validate(content: &str) -> Result<(), MemoryError> {
    for (index, line) in content.lines().enumerate() {
        let line_no = index + 1;
        let lower = line.to_ascii_lowercase();

        for (name, needle) in PATH_PATTERNS {
            // `C:\` only means a drive when a letter precedes it.
            let hit = if *name == "drive-letter" {
                line.as_bytes().windows(3).any(|w| {
                    w[0].is_ascii_alphabetic() && w[1] == b':' && (w[2] == b'\\' || w[2] == b'/')
                })
            } else {
                line.contains(needle)
            };
            if hit {
                return Err(MemoryError::PathLeak {
                    pattern: name,
                    line: line_no,
                });
            }
        }

        for (name, needle) in SECRET_PATTERNS {
            if line.contains(needle) {
                return Err(MemoryError::Secret {
                    pattern: name,
                    line: line_no,
                });
            }
        }
        if ASSIGNMENT_HINTS.iter().any(|hint| lower.contains(hint))
            && lower
                .split_once('=')
                .map(|(_, value)| value.trim().len() >= 8)
                .unwrap_or(false)
        {
            return Err(MemoryError::Secret {
                pattern: "credential-assignment",
                line: line_no,
            });
        }
    }
    Ok(())
}

pub struct MemoryService {
    root: PathBuf,
    pool: SqlitePool,
}

impl MemoryService {
    pub fn new(root: &Path, pool: SqlitePool) -> Self {
        Self {
            root: root.to_path_buf(),
            pool,
        }
    }

    fn agent_dir(&self, agent_id: &str) -> Result<PathBuf, MemoryError> {
        if agent_id.is_empty()
            || agent_id.len() > 64
            || !agent_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(MemoryError::InvalidAgent(agent_id.to_string()));
        }
        Ok(self.root.join(".rigel").join("memory").join(agent_id))
    }

    fn path_for(&self, agent_id: &str, topic: &str) -> Result<(PathBuf, String), MemoryError> {
        if topic.is_empty()
            || topic.len() > 48
            || !topic
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(MemoryError::InvalidTopic(topic.to_string()));
        }
        let dir = self.agent_dir(agent_id)?;
        let (file, relative) = if topic == "memory" {
            (dir.join(MEMORY_FILE), "MEMORY.md".to_string())
        } else {
            (
                dir.join("topics").join(format!("{topic}.md")),
                format!("topics/{topic}.md"),
            )
        };
        let relative = format!(".rigel/memory/{agent_id}/{}", relative.replace('\\', "/"));
        Ok((file, relative))
    }

    pub async fn write(
        &self,
        agent_id: &str,
        topic: &str,
        content: &str,
        mode: WriteMode,
    ) -> Result<MemoryFile, MemoryError> {
        if content.len() > MAX_CONTENT {
            return Err(MemoryError::TooLarge {
                size: content.len(),
            });
        }
        validate(content)?;

        let (path, relative) = self.path_for(agent_id, topic)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let existing = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(MemoryError::Io(error)),
        };
        let body = match mode {
            WriteMode::Replace => content.to_string(),
            WriteMode::Append if existing.is_empty() => content.to_string(),
            WriteMode::Append if existing.ends_with('\n') => format!("{existing}\n{content}"),
            WriteMode::Append => format!("{existing}\n\n{content}"),
        };

        {
            let mut file = fs::File::create(&path)?;
            file.write_all(body.as_bytes())?;
            file.sync_all()?;
        }

        let updated_at = now_ms();
        let sha256 = sha256_hex(body.as_bytes());
        let words = body.split_whitespace().count();

        sqlx::query(
            "INSERT INTO memory_index (id, path, sha256, updated_at, words)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               path = excluded.path, sha256 = excluded.sha256,
               updated_at = excluded.updated_at, words = excluded.words",
        )
        .bind(format!("{agent_id}:{topic}"))
        .bind(&relative)
        .bind(&sha256)
        .bind(updated_at)
        .bind(words as i64)
        .execute(&self.pool)
        .await?;

        Ok(MemoryFile {
            agent_id: agent_id.to_string(),
            topic: topic.to_string(),
            path: relative,
            sha256,
            words,
            bytes: body.len(),
            updated_at,
        })
    }

    pub fn read(&self, agent_id: &str, topic: &str) -> Result<Option<String>, MemoryError> {
        let (path, _) = self.path_for(agent_id, topic)?;
        match fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(MemoryError::Io(error)),
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_prose_passes() {
        validate("The parser lives in crates/rigel-core and uses sqlx.\n").unwrap();
        validate("Use `rigel skills search pdf` before adding a dependency.\n").unwrap();
    }

    #[test]
    fn a_windows_path_is_named_but_not_echoed() {
        let error = validate("root: C:\\Users\\someone\\project").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("drive-letter"));
        assert!(
            !message.contains("someone"),
            "the error must not repeat the path"
        );
    }

    #[test]
    fn home_directories_are_rejected() {
        assert!(validate("see /Users/someone/x").is_err());
        assert!(validate("see /home/someone/x").is_err());
        assert!(validate("see ~/x").is_err());
    }

    #[test]
    fn secrets_and_assignments_are_rejected() {
        assert!(validate("key sk-live-abcdef").is_err());
        assert!(validate("aws AKIAIOSFODNN7EXAMPLE").is_err());
        assert!(validate("OPENROUTER_API_KEY=abcdefgh12345678").is_err());
        assert!(validate("ssh-rsa AAAAB3NzaC1yc2E").is_err());
    }
}

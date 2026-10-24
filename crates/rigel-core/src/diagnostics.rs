//! Opt-in diagnostics and the local crash log (D82).
//!
//! Nothing here sends anything anywhere. A bundle is assembled only when the user asks for it,
//! every string is redacted with the provider keys Rigel can resolve, and the result is written
//! beside the app data so it can be copied into an issue by hand. The crash log follows the same
//! rules: local, and redacted before it is written.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::credentials::{self, PROVIDERS, Secret};
use crate::session::TurnTranscript;

const CRASH_LOG: &str = "crash.log";
const MAX_CRASH_LOG: u64 = 2 * 1024 * 1024;
const MAX_LOG_TAIL: usize = 64 * 1024;

/// Provider keys currently resolvable, for redaction. Environment first, like every other key
/// path; a keychain that refuses is simply not part of the set.
pub fn secrets() -> Vec<Secret> {
    PROVIDERS
        .iter()
        .filter_map(|provider| credentials::get(provider).ok())
        .map(Secret::new)
        .collect()
}

/// Replace every known secret, longest first so one secret cannot hide inside another.
pub fn redact_text(text: &str, secrets: &[Secret]) -> String {
    let mut ordered: Vec<&str> = secrets.iter().map(Secret::expose).collect();
    ordered.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    ordered
        .into_iter()
        .filter(|secret| !secret.is_empty())
        .fold(text.to_string(), |out, secret| {
            out.replace(secret, "<redacted>")
        })
}

fn redact_value(value: &Value, secrets: &[Secret]) -> Value {
    match value {
        Value::String(text) => Value::String(redact_text(text, secrets)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_value(item, secrets))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), redact_value(value, secrets)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Database facts for the doctor section, kept here so the window does not need sqlx.
pub async fn database_health(pool: &sqlx::SqlitePool) -> (String, Vec<(i64, String)>) {
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| format!("unavailable: {error}"));
    let schema: Vec<(i64, String)> =
        sqlx::query_as("SELECT version, name FROM schema_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .unwrap_or_default();
    (integrity, schema)
}

/// The last `limit` bytes of a log file, cut on a character boundary. `None` when there is no
/// log yet, which is common: the app writes one only after something goes wrong.
pub fn log_tail(path: &Path, limit: usize) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let start = bytes.len().saturating_sub(limit);
    let start = (start..bytes.len())
        .find(|index| bytes[*index].is_ascii() || std::str::from_utf8(&bytes[*index..]).is_ok())
        .unwrap_or(bytes.len());
    Some(String::from_utf8_lossy(&bytes[start..]).into_owned())
}

/// One Markdown bundle: the doctor JSON, the last turn, and a redacted log tail. This is the
/// intake format D119 promises, and D95 reuses the same assembly for a session export.
pub fn assemble(
    doctor: &Value,
    last_turn: Option<&TurnTranscript>,
    log: &str,
    secrets: &[Secret],
) -> String {
    let mut out = String::new();
    out.push_str("# Rigel diagnostics\n\n");
    out.push_str(&format!(
        "- Rigel {}\n- {}\n- generated {}\n\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        stamp(),
    ));
    out.push_str("Everything below is local. No request was made to assemble it.\n\n");

    out.push_str("## doctor\n\n```json\n");
    out.push_str(&serde_json::to_string_pretty(&redact_value(doctor, secrets)).unwrap_or_default());
    out.push_str("\n```\n\n");

    out.push_str("## last turn\n\n");
    match last_turn {
        Some(transcript) => {
            let turn = &transcript.turn;
            out.push_str(&format!(
                "- turn `{}` on agent `{}` ({})\n- prompt: {}\n\n",
                turn.id,
                turn.agent_id,
                turn.status,
                redact_text(turn.prompt.as_deref().unwrap_or("(none)"), secrets),
            ));
            for item in &transcript.items {
                out.push_str(&format!(
                    "### {} · {}\n\n```json\n{}\n```\n\n",
                    item.seq,
                    item.kind,
                    serde_json::to_string_pretty(&redact_value(&item.body, secrets))
                        .unwrap_or_default(),
                ));
            }
        }
        None => out.push_str("No turn has been recorded in this project yet.\n\n"),
    }

    out.push_str("## log\n\n```\n");
    out.push_str(&redact_text(log, secrets));
    if !log.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```\n");
    out
}

fn stamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| format!("{} ms since epoch", elapsed.as_millis()))
        .unwrap_or_else(|_| "an unknown time".to_string())
}

/// Write a bundle under `<data>/diagnostics/` and return its path. The directory is created on
/// demand; the file name carries the timestamp so the newest one sorts last.
pub fn write_bundle(data_dir: &Path, body: &str) -> io::Result<PathBuf> {
    let dir = data_dir.join("diagnostics");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("diagnostics-{}.md", now_ms()));
    fs::write(&path, body)?;
    Ok(path)
}

pub fn crash_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CRASH_LOG)
}

/// Append one crash record, rotating at 2 MiB. Redaction happens before the write.
pub fn append_crash(data_dir: &Path, message: &str) {
    let path = crash_log_path(data_dir);
    if fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) > MAX_CRASH_LOG {
        let _ = fs::rename(&path, data_dir.join("crash.old.log"));
    }
    let secrets = secrets();
    let body = format!(
        "--- {} ---\n{}\n\n",
        stamp(),
        redact_text(message, &secrets)
    );
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| io::Write::write_all(&mut file, body.as_bytes()));
}

/// Install a panic hook that writes the last panic to `<data>/crash.log` and still runs the
/// previous hook, so a terminal run keeps its backtrace.
pub fn install_crash_hook(data_dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "an unknown location".to_string());
        let message = format!(
            "panic at {location}\n{}",
            info.payload()
                .downcast_ref::<&str>()
                .map(|text| (*text).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "(non-string payload)".to_string())
        );
        append_crash(&data_dir, &message);
        previous(info);
    }));
}

/// Read the newest crash records for the diagnostics bundle. Empty when nothing has crashed.
pub fn recent_crash(data_dir: &Path) -> String {
    log_tail(&crash_log_path(data_dir), MAX_LOG_TAIL).unwrap_or_default()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0)
}

/// D95: one Markdown bundle and the JSON beside it for a whole session. Shares the redaction
/// and write path with the diagnostics bundle; the caller supplies the rows it already loaded.
pub fn session_bundle(
    session: &crate::session::Session,
    agents: &[crate::session::Agent],
    turns: &[TurnTranscript],
    patches: &[crate::patches::Patch],
    secrets: &[Secret],
) -> (String, String) {
    let value = serde_json::json!({
        "session": session,
        "agents": agents,
        "turns": turns,
        "patches": patches,
        "exported_at": now_ms(),
    });
    let json = serde_json::to_string_pretty(&redact_value(&value, secrets)).unwrap_or_default();

    let mut out = String::new();
    out.push_str(&format!(
        "# Session · {}\n\n",
        redact_text(session.title.as_deref().unwrap_or("untitled"), secrets)
    ));
    out.push_str(&format!(
        "- id: `{}`\n- status: {:?}\n- started: {}\n- spend: {} micros\n- budget: {}\n\n",
        session.id,
        session.status,
        session.created_at,
        session.spend_micros,
        session
            .budget_cents
            .map(|cents| format!("{cents} cents"))
            .unwrap_or_else(|| "no cap".to_string()),
    ));

    out.push_str("## Agents\n\n");
    out.push_str("| agent | harness | role | model | spend (micros) |\n|---|---|---|---|---|\n");
    for agent in agents {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            agent.id,
            agent.harness,
            redact_text(agent.role.as_deref().unwrap_or("—"), secrets),
            redact_text(agent.model.as_deref().unwrap_or("—"), secrets),
            agent.spend_micros,
        ));
    }

    out.push_str("\n## Turns\n\n");
    for transcript in turns {
        let turn = &transcript.turn;
        out.push_str(&format!(
            "### {} · `{}` · {}\n\nPrompt: {}\n\n",
            turn.seq,
            turn.agent_id,
            turn.status,
            redact_text(turn.prompt.as_deref().unwrap_or("(none)"), secrets),
        ));
        for item in &transcript.items {
            out.push_str(&format!(
                "- `{}` {} ({})\n  ```json\n  {}\n  ```\n",
                item.kind,
                item.status,
                item.seq,
                serde_json::to_string(&redact_value(&item.body, secrets)).unwrap_or_default(),
            ));
        }
        out.push('\n');
    }

    out.push_str("## Patches\n\n");
    for patch in patches {
        out.push_str(&format!(
            "### {} · {} · {}\n\n```diff\n{}\n```\n\n",
            patch.path,
            patch.status,
            patch.agent_id,
            redact_text(&patch.diff, secrets),
        ));
    }

    (out, json)
}

/// Write both halves of a session export under `<data>/exports/`, returning their paths.
pub fn write_session_bundle(
    data_dir: &Path,
    session_id: &str,
    markdown: &str,
    json: &str,
) -> io::Result<(PathBuf, PathBuf)> {
    let dir = data_dir.join("exports");
    fs::create_dir_all(&dir)?;
    let stamp = now_ms();
    let md_path = dir.join(format!("session-{session_id}-{stamp}.md"));
    let json_path = dir.join(format!("session-{session_id}-{stamp}.json"));
    fs::write(&md_path, markdown)?;
    fs::write(&json_path, json)?;
    Ok((md_path, json_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_carries_all_three_sections_and_redacts_the_key() {
        let secret = Secret::new("sk-diagnostics-secret");
        let doctor = serde_json::json!({ "check": "ok", "key": "sk-diagnostics-secret" });
        let body = assemble(&doctor, None, "start sk-diagnostics-secret end", &[secret]);
        assert!(body.contains("## doctor"));
        assert!(body.contains("## last turn"));
        assert!(body.contains("## log"));
        assert!(!body.contains("sk-diagnostics-secret"));
        assert!(body.contains("<redacted>"));
        assert!(body.contains("No request was made"));
    }

    #[test]
    fn the_longest_secret_wins_when_one_contains_another() {
        let short = Secret::new("sk-abc");
        let long = Secret::new("sk-abcdef");
        assert_eq!(
            redact_text("x sk-abcdef y", &[short, long]),
            "x <redacted> y"
        );
    }

    #[test]
    fn a_session_export_has_markdown_and_json_with_the_secret_removed() {
        let session = crate::session::Session {
            id: "s-1".to_string(),
            project_root: "C:/work/demo".to_string(),
            title: Some("fix sk-session-key".to_string()),
            status: crate::session::SessionStatus::Active,
            budget_cents: Some(500),
            spend_cents: 1,
            spend_micros: 1000,
            interventions: 0,
            tasks_done: 1,
            created_at: 1,
            ended_at: None,
        };
        let (markdown, json) =
            session_bundle(&session, &[], &[], &[], &[Secret::new("sk-session-key")]);
        assert!(markdown.contains("# Session"));
        assert!(markdown.contains("## Agents"));
        assert!(markdown.contains("## Turns"));
        assert!(markdown.contains("## Patches"));
        assert!(!markdown.contains("sk-session-key"));
        assert!(!json.contains("sk-session-key"));
        assert!(json.contains("\"session\""));
    }

    #[test]
    fn a_crash_record_is_appended_locally() {
        let dir = std::env::temp_dir().join(format!(
            "rigel-diagnostics-{}-{:?}",
            now_ms(),
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        append_crash(&dir, "boom: the window closed unexpectedly");
        let text = recent_crash(&dir);
        assert!(text.contains("boom: the window closed unexpectedly"));
        assert!(text.contains("---"));
        fs::remove_dir_all(&dir).ok();
    }
}

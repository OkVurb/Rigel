use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use rigel_core::agent::agentchat;
use rigel_core::credentials;

const DEFAULT_BASE_URL: &str = "https://api.askrigel.com";
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const ERROR_BACKOFF: Duration = Duration::from_secs(10);
const MESSAGE_LIMIT: u64 = 100;
const INBOX_LIMIT: u64 = 50;
const MAX_TEXT_CHARS: usize = 32_000;
/// Muse writes reports, not transcripts. A room line longer than this is summarised away; the
/// whole envelope stays in the local archive either way.
const MAX_REPLY_CHARS: usize = 4_000;
/// The sender the room shows for Muse's replies. The outbound bridge skips this name, so a reply
/// posted here is never echoed back to Muse.
const MUSE_SENDER: &str = "muse";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RelayCursor {
    #[serde(default)]
    room_message_id: u64,
    /// The highest relay seq whose envelope has been archived and surfaced locally.
    #[serde(default)]
    inbox_seq: u64,
    /// Envelopes archived but not yet acknowledged. Persisted, so a crash between the two steps
    /// re-acks instead of losing or re-posting the message.
    #[serde(default)]
    unacked: BTreeSet<String>,
    #[serde(default)]
    announced_swarm_jobs: BTreeSet<String>,
}

/// One durable envelope as the relay returns it.
#[derive(Debug, Clone, Deserialize)]
struct Envelope {
    seq: u64,
    id: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    payload: Value,
}

#[derive(Debug, Default, Deserialize)]
struct InboxPage {
    #[serde(default)]
    messages: Vec<Envelope>,
}

fn base_url() -> String {
    std::env::var("RIGEL_RELAY_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

fn cursor_path(data_dir: &Path) -> PathBuf {
    data_dir.join("relay-room-cursor.json")
}

fn read_cursor(data_dir: &Path) -> RelayCursor {
    std::fs::read_to_string(cursor_path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_cursor(data_dir: &Path, cursor: &RelayCursor) -> Result<(), String> {
    std::fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    let path = cursor_path(data_dir);
    let temp = path.with_extension("json.tmp");
    let body = serde_json::to_vec(cursor).map_err(|error| error.to_string())?;
    std::fs::write(&temp, body).map_err(|error| error.to_string())?;
    std::fs::rename(&temp, &path).map_err(|error| error.to_string())
}

fn bounded_to(text: &str, max: usize) -> (String, bool) {
    if text.chars().count() <= max {
        return (text.to_string(), false);
    }
    (text.chars().take(max).collect(), true)
}

fn bounded(text: &str) -> (String, bool) {
    bounded_to(text, MAX_TEXT_CHARS)
}

async fn send_envelope(
    client: &reqwest::Client,
    token: &str,
    body: &serde_json::Value,
) -> Result<(), String> {
    let response = client
        .post(format!("{}/v1/relay/messages", base_url()))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .map_err(|error| format!("relay request failed: {error}"))?;
    if response.status() == reqwest::StatusCode::ACCEPTED {
        return Ok(());
    }
    Err(format!(
        "relay refused an event with HTTP {}",
        response.status()
    ))
}

async fn send_message(
    client: &reqwest::Client,
    token: &str,
    project: Option<&Path>,
    message: &agentchat::ChatMessage,
) -> Result<(), String> {
    let (text, truncated) = bounded(&message.text);
    let payload = json!({
        "kind": "event",
        "idempotency_key": format!("room-message-{}", message.id),
        "payload": {
            "type": "swarm.agent_message",
            "source": "rigel-desktop",
            "room_message_id": message.id,
            "sender": message.sender,
            "channel": message.channel,
            "message_kind": message.kind,
            "time": message.time,
            "text": text,
            "truncated": truncated,
            "project_name": project.and_then(|root| root.file_name()).map(|name| name.to_string_lossy().into_owned()),
            "attachments": message.attachments.iter().map(|attachment| json!({
                "name": attachment.name,
                "available_through_relay": false
            })).collect::<Vec<_>>(),
        }
    });
    send_envelope(client, token, &payload).await
}

async fn announce_swarm_jobs(
    client: &reqwest::Client,
    token: &str,
    data_dir: &Path,
    cursor: &mut RelayCursor,
) -> Result<bool, String> {
    let inbox = data_dir.join("swarm").join("inbox");
    let Ok(entries) = std::fs::read_dir(inbox) else {
        return Ok(false);
    };
    let mut jobs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(job) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(id) = job.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !cursor.announced_swarm_jobs.contains(id) {
            jobs.push((id.to_string(), job));
        }
    }
    jobs.sort_by_key(|(_, job)| {
        job.get("created_at")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
    });
    let mut changed = false;
    for (id, job) in jobs {
        let payload = json!({
            "kind": "event",
            "idempotency_key": format!("swarm-ready-{id}"),
            "payload": {
                "type": "swarm.ready",
                "source": "rigel-desktop",
                "job_id": id,
                "created_at": job.get("created_at"),
                "targets": job.get("targets"),
                "lead": job.get("lead"),
                "project_name": job.get("project").and_then(serde_json::Value::as_str).and_then(|path| Path::new(path).file_name()).map(|name| name.to_string_lossy().into_owned()),
                "prompt_preview": job.get("prompt").and_then(serde_json::Value::as_str).map(|prompt| bounded(prompt).0),
                "control_api_action": "Fetch the authoritative job from the tailnet-only /v1/swarm/jobs endpoint before claiming."
            }
        });
        send_envelope(client, token, &payload).await?;
        cursor.announced_swarm_jobs.insert(id);
        write_cursor(data_dir, cursor)?;
        changed = true;
    }
    Ok(changed)
}

/* ------------------------------- inbox side ------------------------------- */

/// The one-line summary the room shows. Unknown payloads still produce a line: an envelope the
/// desktop cannot interpret must not pin the queue behind it.
fn summarize(payload: &Value) -> String {
    let kind = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("relay.message");
    let mut head = format!("◆ {MUSE_SENDER} · {kind}");
    if let Some(title) = payload.get("title").and_then(Value::as_str) {
        let (title, _) = bounded_to(title.trim(), 160);
        if !title.is_empty() {
            head.push_str(&format!(" — {title}"));
        }
    }
    if let Some(status) = payload.get("status").and_then(Value::as_str) {
        head.push_str(&format!(" [{status}]"));
    }
    let body = ["text", "message", "body", "summary", "detail"]
        .iter()
        .find_map(|key| payload.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty());
    match body {
        Some(text) => {
            let (text, cut) = bounded_to(text, MAX_REPLY_CHARS);
            let suffix = if cut { "\n…" } else { "" };
            format!("{head}\n{text}{suffix}")
        }
        None => {
            let (json, cut) = bounded_to(&payload.to_string(), MAX_REPLY_CHARS);
            format!("{head}\n{json}{}", if cut { "\n…" } else { "" })
        }
    }
}

/// Append the whole envelope to `relay/inbox.jsonl` and fsync it. This is what makes a later ack
/// safe: the message is on local disk before the relay is told to forget it.
fn archive(data_dir: &Path, envelope: &Envelope) -> Result<(), String> {
    let dir = data_dir.join("relay");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let line = json!({
        "seq": envelope.seq,
        "id": envelope.id,
        "kind": envelope.kind,
        "received_at": rigel_core::migrate::now_ms(),
        "payload": envelope.payload,
    });
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("inbox.jsonl"))
        .map_err(|error| error.to_string())?;
    writeln!(file, "{line}").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

async fn fetch_inbox(
    client: &reqwest::Client,
    token: &str,
    after: u64,
) -> Result<Vec<Envelope>, String> {
    let response = client
        .get(format!(
            "{}/v1/relay/messages?after={after}&limit={INBOX_LIMIT}",
            base_url()
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("relay inbox request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "relay inbox refused a read with HTTP {}",
            response.status()
        ));
    }
    let page: InboxPage = response
        .json()
        .await
        .map_err(|error| format!("relay inbox returned an unreadable body: {error}"))?;
    Ok(page.messages)
}

async fn acknowledge(client: &reqwest::Client, token: &str, id: &str) -> Result<(), String> {
    let response = client
        .post(format!("{}/v1/relay/messages/{id}/ack", base_url()))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("relay ack failed: {error}"))?;
    if response.status().is_success() {
        return Ok(());
    }
    Err(format!(
        "relay refused an ack with HTTP {}",
        response.status()
    ))
}

/// Post a reply into the room so it is visible where the work happens. Best effort: the archive is
/// the durable copy, and a room that is down must not block the ack.
async fn surface(project: Option<&Path>, text: &str) {
    let status = agentchat::status(project.and_then(Path::to_str)).await;
    if !status.running || !status.authed {
        return;
    }
    let _ = agentchat::send_as(text, "general", MUSE_SENDER).await;
}

/// A turn that selects the `rigel` model reads its own reply from the relay and acknowledges it.
/// Archiving or acknowledging that envelope here would consume the answer before the turn sees
/// it, so the bridge only advances its scan cursor past it.
fn is_model_reply(payload: &Value) -> bool {
    payload
        .get("reply_to")
        .and_then(Value::as_str)
        .is_some_and(|reply| reply.starts_with(rigel_core::agent::rigel::REQUEST_PREFIX))
}

async fn drain_inbox(
    client: &reqwest::Client,
    token: &str,
    data_dir: &Path,
    project: Option<&Path>,
    cursor: &mut RelayCursor,
) -> Result<bool, String> {
    let mut touched = false;
    loop {
        let page = fetch_inbox(client, token, cursor.inbox_seq).await?;
        if page.is_empty() {
            break;
        }
        let count = page.len();
        for envelope in page {
            if is_model_reply(&envelope.payload) {
                cursor.inbox_seq = cursor.inbox_seq.max(envelope.seq);
                write_cursor(data_dir, cursor)?;
                continue;
            }
            archive(data_dir, &envelope)?;
            surface(project, &summarize(&envelope.payload)).await;
            cursor.unacked.insert(envelope.id.clone());
            cursor.inbox_seq = cursor.inbox_seq.max(envelope.seq);
            write_cursor(data_dir, cursor)?;
            touched = true;
        }
        if count < INBOX_LIMIT as usize {
            break;
        }
    }
    // Only now is the relay told to forget them. An ack failure leaves the id in the cursor, so
    // the next pass retries it without re-archiving or re-posting anything.
    for id in cursor.unacked.clone() {
        match acknowledge(client, token, &id).await {
            Ok(()) => {
                cursor.unacked.remove(&id);
                write_cursor(data_dir, cursor)?;
                touched = true;
            }
            Err(_) => break,
        }
    }
    Ok(touched)
}

async fn bridge_once(
    client: &reqwest::Client,
    token: &str,
    data_dir: &Path,
    project: Option<&Path>,
) -> Result<bool, String> {
    let mut cursor = read_cursor(data_dir);
    let mut changed = drain_inbox(client, token, data_dir, project, &mut cursor).await?;
    changed |= announce_swarm_jobs(client, token, data_dir, &mut cursor).await?;
    // A room may start after the desktop. Refreshing status adopts that room's newly minted
    // session token before the authenticated history read; otherwise the bridge would retry a
    // token-less request forever until somebody happened to open the Chat tab.
    let status = agentchat::status(project.and_then(Path::to_str)).await;
    if !status.running || !status.authed {
        return Ok(changed);
    }
    let messages = agentchat::messages(None, cursor.room_message_id, MESSAGE_LIMIT).await?;
    for message in messages {
        // Muse receives its own sends through the relay already. Mirroring them back would create
        // an echo and reveal no additional model output.
        if message.sender.eq_ignore_ascii_case(MUSE_SENDER) {
            cursor.room_message_id = message.id;
            write_cursor(data_dir, &cursor)?;
            continue;
        }
        send_message(client, token, project, &message).await?;
        cursor.room_message_id = message.id;
        write_cursor(data_dir, &cursor)?;
        changed = true;
    }
    Ok(changed)
}

pub fn start(data_dir: PathBuf, project: fn() -> Option<PathBuf>) {
    std::thread::Builder::new()
        .name("rigel-domain-relay".to_string())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                let client = reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(5))
                    .timeout(Duration::from_secs(15))
                    .build()
                    .unwrap_or_default();
                loop {
                    let token = match credentials::get(credentials::RELAY) {
                        Ok(token) => token,
                        Err(_) => {
                            tokio::time::sleep(ERROR_BACKOFF).await;
                            continue;
                        }
                    };
                    match bridge_once(&client, &token, &data_dir, project().as_deref()).await {
                        Ok(true) => continue,
                        Ok(false) => tokio::time::sleep(POLL_INTERVAL).await,
                        Err(_) => tokio::time::sleep(ERROR_BACKOFF).await,
                    }
                }
            });
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_survives_a_restart() {
        let root = tempfile::tempdir().unwrap();
        let cursor = RelayCursor {
            room_message_id: 42,
            ..RelayCursor::default()
        };
        write_cursor(root.path(), &cursor).unwrap();
        assert_eq!(read_cursor(root.path()).room_message_id, 42);
    }

    #[test]
    fn an_unacked_envelope_survives_a_restart() {
        let root = tempfile::tempdir().unwrap();
        let cursor = RelayCursor {
            inbox_seq: 9,
            unacked: BTreeSet::from(["abc".to_string()]),
            ..RelayCursor::default()
        };
        write_cursor(root.path(), &cursor).unwrap();
        let read = read_cursor(root.path());
        assert_eq!(read.inbox_seq, 9);
        assert!(read.unacked.contains("abc"));
    }

    #[test]
    fn long_room_output_is_bounded() {
        let (text, truncated) = bounded(&"x".repeat(MAX_TEXT_CHARS + 1));
        assert!(truncated);
        assert_eq!(text.chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn a_status_report_summarises_to_title_and_body() {
        let line = summarize(&json!({
            "type": "swarm.status",
            "status": "blocked",
            "title": "Retry the deploy poller",
            "text": "Control API unreachable; nothing was claimed."
        }));
        assert!(line.starts_with("◆ muse · swarm.status — Retry the deploy poller [blocked]"));
        assert!(line.ends_with("Control API unreachable; nothing was claimed."));
    }

    #[test]
    fn an_unknown_payload_still_produces_a_line() {
        let line = summarize(&json!({ "type": "muse.handoff_response", "accepted": true }));
        assert!(line.starts_with("◆ muse · muse.handoff_response"));
        assert!(line.contains("\"accepted\":true"));
    }

    #[test]
    fn a_long_reply_body_is_cut_but_the_head_survives() {
        let line = summarize(&json!({
            "type": "muse.response",
            "text": "y".repeat(MAX_REPLY_CHARS + 50)
        }));
        assert!(line.starts_with("◆ muse · muse.response"));
        assert!(line.ends_with('…'));
        assert!(line.chars().count() < MAX_REPLY_CHARS + 200);
    }
}

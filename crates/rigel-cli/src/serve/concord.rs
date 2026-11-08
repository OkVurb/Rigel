//! A small stdio client for Concord (`@concord-ai/concord-mcp`).
//!
//! Concord is a Node MCP server, not a Rigel crate, so the API speaks JSON-RPC to it rather
//! than importing it. One process per request keeps the bridge stateless: a crashed or
//! upgraded Concord cannot leave a half-initialized child behind, and the workspace root is
//! whatever the API is serving at that moment. The handshake is the same one
//! `rigel-mcp/src/project_mcp.rs` uses for a project server; the only addition is `tools/call`.
//!
//! The inbox poller is the one exception to one-process-per-request. It holds a session open
//! and calls `inspect_work` every few seconds, because paying an `npx` cold start for each poll
//! would make a background service expensive. New messages are cached with a monotonic `seq`
//! so Muse can read them from `/v1/concord/inbox` instead of running the tool itself.

use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child as Process, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// The identity Muse speaks as. It is stable across requests, so a reply addressed to `muse`
/// reaches the next call. Other Concord clients see one API participant, not one per HTTP
/// connection.
pub const AGENT_ID: &str = "muse";
pub const PACKAGE: &str = "@concord-ai/concord-mcp";
const PROTOCOL: &str = "2025-06-18";

/// How often the background poller asks Concord for the inbox.
const POLL_INTERVAL: Duration = Duration::from_secs(4);
/// A failed session is retried on this cadence rather than hammering a broken `npx`.
const RETRY_INTERVAL: Duration = Duration::from_secs(10);
/// The cache is a wake buffer, not an archive. Older messages stay on disk in `.concord`.
const INBOX_KEEP: usize = 200;
/// Bounded de-duplication memory, so a long-lived API does not grow without limit.
const SEEN_KEEP: usize = 5_000;
const INIT_WAIT_SECS: u64 = 90;
const INSPECT_WAIT_SECS: u64 = 30;

/// Windows `canonicalize` prefixes a path with `\\?\`. Node's path helpers treat that as a
/// literal name and Concord fails with `EISDIR: lstat 'C:'`; it wants the ordinary spelling.
pub fn plain_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        text.into_owned()
    }
}

pub async fn tools(project: &Path) -> Result<Value, String> {
    let project = project.to_path_buf();
    tokio::task::spawn_blocking(move || exchange(&project, "tools/list", json!({}), 90))
        .await
        .map_err(|error| format!("concord task: {error}"))?
}

pub async fn call(project: &Path, tool: &str, arguments: Value) -> Result<Value, String> {
    let project = project.to_path_buf();
    let tool = tool.to_string();
    tokio::task::spawn_blocking(move || {
        exchange(
            &project,
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
            120,
        )
    })
    .await
    .map_err(|error| format!("concord task: {error}"))?
}

/// The concatenated text blocks of a Concord tool result. Concord puts its answer in
/// `content[].text`; the JSON itself is useful to a caller that wants the raw shape.
pub fn text(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn exchange(project: &Path, method: &str, params: Value, wait_secs: u64) -> Result<Value, String> {
    let mut client = Client::spawn(project)?;
    client.initialize()?;
    client.request(method, params, wait_secs)
}

/// One Concord child and its JSON-RPC stream. `Drop` ends the whole process tree: on Windows the
/// `cmd /c npx` wrapper owns the actual Node process, and killing only the wrapper would leave
/// an orphan holding the workspace.
struct Client {
    process: Process,
    stdin: ChildStdin,
    rx: mpsc::Receiver<Value>,
    stderr: Arc<Mutex<String>>,
    next_id: u64,
    project: PathBuf,
}

impl Client {
    fn spawn(project: &Path) -> Result<Self, String> {
        let root = plain_path(project);
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/c", "npx", "-y", PACKAGE]);
            command
        } else {
            let mut command = Command::new("npx");
            command.args(["-y", PACKAGE]);
            command
        };
        command
            .env("CONCORD_REPO_ROOT", root)
            .env("CONCORD_AGENT_ID", AGENT_ID)
            .env("CONCORD_AGENT_KIND", "api")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut process = command
            .spawn()
            .map_err(|error| format!("could not start Concord (is Node installed?): {error}"))?;
        let stdin = process.stdin.take().ok_or("Concord has no stdin")?;
        let stdout = process.stdout.take().ok_or("Concord has no stdout")?;
        let stderr = process.stderr.take();

        let (tx, rx) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(message) = serde_json::from_str::<Value>(&line) {
                    let _ = tx.send(message);
                }
            }
        });
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        if let Some(stderr) = stderr {
            let sink = stderr_tail.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    let Ok(line) = line else { break };
                    if let Ok(mut text) = sink.lock() {
                        text.push_str(&line);
                        text.push('\n');
                        if text.len() > 4_000 {
                            let excess = text.len() - 4_000;
                            text.drain(..excess);
                        }
                    }
                }
            });
        }

        Ok(Self {
            process,
            stdin,
            rx,
            stderr: stderr_tail,
            next_id: 1,
            project: project.to_path_buf(),
        })
    }

    fn initialize(&mut self) -> Result<(), String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": { "name": "rigel", "version": env!("CARGO_PKG_VERSION") }
            }
        }))?;
        let initialized = self.wait(id, Duration::from_secs(INIT_WAIT_SECS))?;
        if let Some(error) = initialized.get("error") {
            return Err(format!("Concord refused initialize: {error}"));
        }
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))
    }

    fn request(&mut self, method: &str, params: Value, wait_secs: u64) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;
        let reply = self.wait(id, Duration::from_secs(wait_secs))?;
        if let Some(error) = reply.get("error") {
            return Err(format!("Concord refused {method}: {error}"));
        }
        reply
            .get("result")
            .cloned()
            .ok_or_else(|| format!("Concord returned no result for {method}"))
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.flush())
            .map_err(|error| format!("could not write to Concord: {error}"))
    }

    fn wait(&self, id: u64, wait: Duration) -> Result<Value, String> {
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(
                    self.with_stderr(format!("Concord did not answer request {id} in time"))
                );
            }
            match self.rx.recv_timeout(left) {
                Ok(message) if message.get("id") == Some(&json!(id)) => return Ok(message),
                Ok(_) => continue,
                Err(_) => {
                    return Err(self.with_stderr("Concord closed before answering".to_string()));
                }
            }
        }
    }

    fn with_stderr(&self, error: String) -> String {
        let detail = self
            .stderr
            .lock()
            .map(|text| text.trim().to_string())
            .unwrap_or_default();
        if detail.is_empty() {
            error
        } else {
            format!("{error}: {detail}")
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            // `/T` reaches the Node process under `cmd /c npx`; killing only the wrapper leaves
            // the real Concord child running.
            let _ = rigel_core::agent::agentchat::kill_tree(self.process.id());
        }
        // Always guarantee the direct child is gone before `wait`, so a failed tree kill cannot
        // turn Drop into a hang.
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// A Concord session kept open for repeated calls. The poller owns one at a time; a workspace
/// switch or a failed call drops it, and the next poll opens a fresh session.
pub struct Session {
    client: Client,
}

impl Session {
    pub fn open(project: &Path) -> Result<Self, String> {
        let mut client = Client::spawn(project)?;
        client.initialize()?;
        Ok(Self { client })
    }

    fn project(&self) -> &Path {
        &self.client.project
    }

    fn inspect(&mut self) -> Result<Value, String> {
        let result = self.client.request(
            "tools/call",
            json!({ "name": "inspect_work", "arguments": { "agent_id": AGENT_ID } }),
            INSPECT_WAIT_SECS,
        )?;
        if result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let detail = text(&result);
            return Err(if detail.is_empty() {
                "Concord refused inspect_work".to_string()
            } else {
                detail
            });
        }
        Ok(result)
    }
}

/// The cached inbox. `seq` is assigned by this process, not Concord, so a client can resume
/// with `?since=<last seq>` without depending on Concord's ids or ordering.
pub struct InboxState {
    entries: VecDeque<Entry>,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
    next_seq: u64,
    /// Moves for a new message and for a changed receipt on an existing one, so `/v1/events`
    /// wakes a client when mail is answered as well as when it first arrives.
    revision: u64,
    checked_at: i64,
    updated_at: i64,
    error: Option<String>,
}

struct Entry {
    seq: u64,
    message: Value,
}

impl Default for InboxState {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            seen: HashSet::new(),
            seen_order: VecDeque::new(),
            next_seq: 1,
            revision: 0,
            checked_at: 0,
            updated_at: 0,
            error: None,
        }
    }
}

impl InboxState {
    /// Merge one `inspect_work` inbox into the cache. The same message returning on the next
    /// poll is normal (Concord may keep delivered mail in the inbox), so identity is the
    /// message's own id, or its whole body when it has none.
    pub fn apply(&mut self, inbox: Vec<Value>) -> bool {
        self.checked_at = rigel_core::now_ms();
        self.error = None;
        let mut changed = false;
        for message in inbox {
            if !message.is_object() {
                continue;
            }
            let key = message_key(&message);
            // Concord keeps a delivered message in the inbox and adds `readAt`, `repliedAt` and
            // a richer `receipt` on later polls. Replace the cached copy instead of treating the
            // id as "already seen", or the window would show a receipt that never advances.
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| message_key(&entry.message) == key)
            {
                if entry.message != message {
                    entry.message = message;
                    self.revision = self.revision.saturating_add(1);
                    changed = true;
                }
                continue;
            }
            if !self.seen.insert(key.clone()) {
                continue;
            }
            self.seen_order.push_back(key);
            while self.seen_order.len() > SEEN_KEEP {
                if let Some(oldest) = self.seen_order.pop_front() {
                    self.seen.remove(&oldest);
                }
            }
            let seq = self.next_seq;
            self.next_seq += 1;
            self.revision = self.revision.saturating_add(1);
            self.entries.push_back(Entry { seq, message });
            changed = true;
        }
        while self.entries.len() > INBOX_KEEP {
            self.entries.pop_front();
        }
        if changed {
            self.updated_at = self.checked_at;
        }
        changed
    }

    /// `inspect_work` marks a message read when the poller collects it, so the cache is the only
    /// copy left for a Muse that has not read it yet. Persisting it means an API restart (or the
    /// login start) does not silently drop mail.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let body = json!({
            "next_seq": self.next_seq,
            "revision": self.revision,
            "checked_at": self.checked_at,
            "updated_at": self.updated_at,
            "messages": self
                .entries
                .iter()
                .map(|entry| json!({ "seq": entry.seq, "message": entry.message }))
                .collect::<Vec<_>>(),
        });
        let parent = path
            .parent()
            .ok_or_else(|| "the inbox cache has no parent directory".to_string())?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, body.to_string()).map_err(|error| error.to_string())?;
        std::fs::rename(&temp, path).map_err(|error| error.to_string())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(format!("could not read {}: {error}", path.display()));
            }
        };
        let value = serde_json::from_str::<Value>(&text)
            .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))?;
        let mut state = Self {
            next_seq: value
                .get("next_seq")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .max(1),
            revision: value.get("revision").and_then(Value::as_u64).unwrap_or(0),
            checked_at: value.get("checked_at").and_then(Value::as_i64).unwrap_or(0),
            updated_at: value.get("updated_at").and_then(Value::as_i64).unwrap_or(0),
            ..Self::default()
        };
        if let Some(messages) = value.get("messages").and_then(Value::as_array) {
            for entry in messages {
                let Some(seq) = entry.get("seq").and_then(Value::as_u64) else {
                    continue;
                };
                let Some(message) = entry.get("message").filter(|message| message.is_object())
                else {
                    continue;
                };
                let key = message_key(message);
                state.seen.insert(key.clone());
                state.seen_order.push_back(key);
                state.entries.push_back(Entry {
                    seq: seq.max(1),
                    message: message.clone(),
                });
            }
        }
        let newest = state
            .entries
            .iter()
            .map(|entry| entry.seq)
            .max()
            .unwrap_or(0);
        state.next_seq = state.next_seq.max(newest + 1);
        // A cache written before `revision` existed still has one revision per retained row.
        state.revision = state.revision.max(newest);
        while state.entries.len() > INBOX_KEEP {
            state.entries.pop_front();
        }
        while state.seen_order.len() > SEEN_KEEP {
            if let Some(oldest) = state.seen_order.pop_front() {
                state.seen.remove(&oldest);
            }
        }
        Ok(state)
    }

    pub fn record_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[cfg(test)]
    fn retained(&self) -> usize {
        self.entries.len()
    }

    /// `since` is the last `seq` the caller handled. The limit applies after filtering, so a
    /// client replaying from zero still gets the newest rows rather than the oldest 200.
    pub fn snapshot(&self, since: u64, limit: usize) -> Value {
        let messages: Vec<Value> = self
            .entries
            .iter()
            .filter(|entry| entry.seq > since)
            .take(limit)
            .map(|entry| json!({ "seq": entry.seq, "message": entry.message }))
            .collect();
        json!({
            "ok": true,
            "agent": AGENT_ID,
            "messages": messages,
            "retained": self.entries.len(),
            "oldest": self.entries.front().map(|entry| entry.seq).unwrap_or(0),
            "revision": self.revision(),
            "checked_at": self.checked_at,
            "updated_at": self.updated_at,
            "error": self.error,
        })
    }

    /// One cached message by its Concord id. The reply route uses this to address the sender
    /// without asking the caller to copy `senderAgentId` out of the payload by hand.
    pub fn message(&self, message_id: &str) -> Option<Value> {
        self.entries
            .iter()
            .find(|entry| {
                ["messageId", "message_id", "id"]
                    .iter()
                    .any(|key| entry.message.get(*key).and_then(Value::as_str) == Some(message_id))
            })
            .map(|entry| entry.message.clone())
    }

    /// The compact form in `/v1/app` and `/v1/events`; the messages themselves stay on the
    /// inbox route.
    pub fn brief(&self) -> Value {
        json!({
            "agent": AGENT_ID,
            "pending": self.entries.len(),
            "revision": self.revision(),
            "checked_at": self.checked_at,
            "updated_at": self.updated_at,
            "error": self.error,
        })
    }
}

fn message_key(message: &Value) -> String {
    for key in ["id", "message_id", "messageId"] {
        if let Some(value) = message.get(key)
            && !value.is_null()
        {
            return match value {
                Value::String(text) => format!("{key}:{text}"),
                other => format!("{key}:{other}"),
            };
        }
    }
    if let Some(id) = message
        .get("message")
        .and_then(|inner| inner.get("id"))
        .filter(|value| !value.is_null())
    {
        return format!("message.id:{id}");
    }
    format!("body:{message}")
}

/// Poll Muse's Concord inbox until the process exits. The thread owns one session, reopening it
/// when the active workspace changes or a call fails. Errors are reported through the cache
/// rather than taking the API down; a machine without Node still serves every other route.
pub fn spawn_poller<P>(state: Arc<Mutex<InboxState>>, project: P, cache: PathBuf)
where
    P: Fn() -> PathBuf + Send + 'static,
{
    std::thread::spawn(move || {
        let mut session: Option<Session> = None;
        loop {
            let root = project();
            if session
                .as_ref()
                .is_some_and(|current| current.project() != root.as_path())
            {
                session = None;
            }
            if session.is_none() {
                match Session::open(&root) {
                    Ok(open) => session = Some(open),
                    Err(error) => {
                        if let Ok(mut inbox) = state.lock() {
                            inbox.record_error(error);
                        }
                        std::thread::sleep(RETRY_INTERVAL);
                        continue;
                    }
                }
            }
            let result = match session.as_mut() {
                Some(open) => open.inspect(),
                None => continue,
            };
            match result {
                Ok(result) => {
                    let inbox = result
                        .get("structuredContent")
                        .and_then(|value| value.get("inbox"))
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let mut save_error = None;
                    if let Ok(mut state) = state.lock()
                        && state.apply(inbox)
                    {
                        save_error = state.save(&cache).err();
                    }
                    if let Some(error) = save_error {
                        eprintln!("rigel: could not persist the Concord inbox: {error}");
                    }
                }
                Err(error) => {
                    if let Ok(mut inbox) = state.lock() {
                        inbox.record_error(error);
                    }
                    session = None;
                    std::thread::sleep(RETRY_INTERVAL);
                    continue;
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_messages_gain_one_sequence_each() {
        let mut state = InboxState::default();
        state.apply(vec![json!({ "id": "a", "text": "hello" })]);
        state.apply(vec![json!({ "id": "a", "text": "hello" })]);
        assert_eq!(state.retained(), 1);
        assert_eq!(state.revision(), 1);
        assert!(state.brief()["error"].is_null());
    }

    #[test]
    fn since_reads_only_what_the_client_has_not_seen() {
        let mut state = InboxState::default();
        state.apply(vec![json!({ "id": "a" }), json!({ "id": "b" })]);
        let snapshot = state.snapshot(1, 50);
        let messages = snapshot["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["seq"], 2);
        assert_eq!(messages[0]["message"]["id"], "b");
        assert_eq!(snapshot["oldest"], 1);
    }

    #[test]
    fn a_receipt_update_replaces_the_cached_row_and_moves_revision() {
        let mut state = InboxState::default();
        state.apply(vec![json!({
            "messageId": "m1",
            "status": "read",
            "receipt": { "status": "read" }
        })]);
        let revision = state.revision();
        state.apply(vec![json!({
            "messageId": "m1",
            "status": "replied",
            "receipt": { "status": "replied", "replied_at": "2026-11-06T22:00:00.000Z" }
        })]);
        assert_eq!(state.retained(), 1);
        assert_eq!(state.revision(), revision + 1);
        let snapshot = state.snapshot(0, 10);
        assert_eq!(snapshot["messages"][0]["message"]["status"], "replied");
        assert_eq!(
            snapshot["messages"][0]["message"]["receipt"]["replied_at"],
            "2026-11-06T22:00:00.000Z"
        );
    }

    #[test]
    fn an_unnamed_message_is_deduplicated_by_its_body() {
        let mut state = InboxState::default();
        state.apply(vec![json!({ "from": "forge", "text": "one" })]);
        state.apply(vec![json!({ "from": "forge", "text": "one" })]);
        state.apply(vec![json!({ "from": "forge", "text": "two" })]);
        assert_eq!(state.retained(), 2);
    }

    #[test]
    fn the_cache_keeps_the_newest_entries() {
        let mut state = InboxState::default();
        for index in 0..(INBOX_KEEP + 5) {
            state.apply(vec![json!({ "id": format!("m{index}") })]);
        }
        assert_eq!(state.retained(), INBOX_KEEP);
        let snapshot = state.snapshot(0, INBOX_KEEP + 50);
        assert_eq!(snapshot["messages"].as_array().unwrap().len(), INBOX_KEEP);
        assert_eq!(snapshot["oldest"], 6);
        assert_eq!(snapshot["revision"], (INBOX_KEEP + 5) as u64);
    }

    #[test]
    fn the_cache_survives_a_restart() {
        let path = std::env::temp_dir().join(format!(
            "rigel-concord-inbox-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        let mut state = InboxState::default();
        state.apply(vec![json!({ "messageId": "m1", "content": "hello" })]);
        state.save(&path).expect("save the cache");
        let mut loaded = InboxState::load(&path).expect("load the cache");
        assert_eq!(loaded.retained(), 1);
        assert_eq!(loaded.revision(), 1);
        assert_eq!(
            loaded.snapshot(0, 10)["messages"][0]["message"]["messageId"],
            "m1"
        );
        loaded.apply(vec![json!({ "messageId": "m2" })]);
        assert_eq!(loaded.revision(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn message_finds_a_cached_row_by_its_concord_id() {
        let mut state = InboxState::default();
        state.apply(vec![json!({ "messageId": "m1", "senderAgentId": "forge" })]);
        assert_eq!(state.message("m1").unwrap()["senderAgentId"], "forge");
        assert!(state.message("missing").is_none());
    }

    #[test]
    fn an_error_does_not_discard_cached_messages() {
        let mut state = InboxState::default();
        state.apply(vec![json!({ "id": "a" })]);
        state.record_error("node is missing".to_string());
        assert_eq!(state.retained(), 1);
        assert_eq!(state.brief()["error"], "node is missing");
        state.apply(vec![json!({ "id": "b" })]);
        assert!(state.brief()["error"].is_null());
        assert_eq!(state.retained(), 2);
    }
}

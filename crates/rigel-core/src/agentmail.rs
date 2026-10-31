//! AgentMail: one inbox the agent in this project can read and send from.
//!
//! The app is offline by default. This module is only reachable when the user saves an inbox
//! and an API key, and it makes exactly three kinds of request:
//!
//! * `GET  {base}/inboxes/{inbox}/messages` — the newest messages, summaries only;
//! * `GET  {base}/inboxes/{inbox}/messages/{id}` — one message, with its body;
//! * `POST {base}/inboxes/{inbox}/messages/send` — a new message;
//! * `POST {base}/inboxes/{inbox}/messages/{id}/reply` — a reply on the same thread.
//!
//! The key lives in the OS keychain under `rigel/agentmail` and is never written to
//! [`CONFIG_FILE`], never logged, and never returned to the renderer. `AGENTMAIL_API_KEY` is
//! honoured as a fallback for headless use, like every other credential in this crate.
//!
//! Sending is the one irreversible thing here, so the agent tools do not call [`Mail::send`]
//! without an approval from the user first (`agent::MailApprover`). This module has no opinion
//! about that; it just refuses to guess an inbox or a key.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::credentials;

/// Where the inbox address lives when it is not in the environment.
pub const CONFIG_FILE: &str = "agentmail.json";
/// The API root. Everything below it is the documented AgentMail v0 surface.
pub const DEFAULT_BASE: &str = "https://api.agentmail.to/v0";
/// Set to point the client at a local stand-in, the same way `RIGEL_*_BASE_URL` works elsewhere.
pub const BASE_ENV: &str = "RIGEL_AGENTMAIL_BASE";
/// The inbox address, for a shell or CI run that has no config file.
pub const INBOX_ENV: &str = "RIGEL_AGENTMAIL_INBOX";
/// How many summaries a list returns when the caller does not say.
pub const DEFAULT_LIMIT: usize = 10;
/// AgentMail's own ceiling for one list page.
pub const MAX_LIMIT: usize = 50;
/// One conversation is shown in full; past this, the page is a mailbox, not a thread.
pub const MAX_THREAD_MESSAGES: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error(
        "no AgentMail inbox configured: run `rigel mail configure --inbox <address>` or set {INBOX_ENV}"
    )]
    NotConfigured,

    #[error("`{0}` is not an email address")]
    InvalidInbox(String),

    #[error("no AgentMail API key: run `rigel keys set agentmail` or set AGENTMAIL_API_KEY")]
    MissingKey,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("the AgentMail config is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("AgentMail rejected the request ({status}): {message}")]
    Rejected { status: u16, message: String },

    #[error("AgentMail answered with something Rigel does not understand: {0}")]
    Malformed(String),
}

impl MailError {
    /// 1 is something the user can fix (a missing setting, a bad address); 2 is a failed
    /// precondition or a network fault. The CLI maps these straight through.
    pub fn exit_code(&self) -> u8 {
        match self {
            MailError::NotConfigured | MailError::InvalidInbox(_) | MailError::MissingKey => 1,
            MailError::Rejected { status, .. } if (400..500).contains(status) => 1,
            _ => 2,
        }
    }
}

/* -------------------------------- configuration -------------------------------- */

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailConfig {
    /// The inbox every request names, e.g. `rigelagent@agentmail.to`.
    pub inbox: String,
}

impl MailConfig {
    pub fn new(inbox: impl Into<String>) -> Result<Self, MailError> {
        let inbox = inbox.into();
        let inbox = inbox.trim().to_string();
        if !looks_like_address(&inbox) {
            return Err(MailError::InvalidInbox(inbox));
        }
        Ok(Self { inbox })
    }

    pub fn path(dir: &Path) -> PathBuf {
        dir.join(CONFIG_FILE)
    }

    /// The environment wins, so a test or a CI run needs no file on disk.
    pub fn load(dir: &Path) -> Result<Option<Self>, MailError> {
        if let Some(value) = std::env::var(INBOX_ENV)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            return Self::new(value).map(Some);
        }
        let path = Self::path(dir);
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        let stored: Self = serde_json::from_str(&text)?;
        Ok(Some(Self::new(stored.inbox)?))
    }

    pub fn save(dir: &Path, config: &Self) -> Result<PathBuf, MailError> {
        std::fs::create_dir_all(dir)?;
        let path = Self::path(dir);
        let text = serde_json::to_string_pretty(config)?;
        std::fs::write(&path, format!("{text}\n"))?;
        Ok(path)
    }
}

/// One `@`, something before it, and a dotted domain after it. Deliberately loose: AgentMail
/// decides what it will accept, and this only stops a typo becoming a request.
fn looks_like_address(value: &str) -> bool {
    if value.chars().any(char::is_whitespace) || value.len() > 254 {
        return false;
    }
    let mut parts = value.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

/* ---------------------------------- the status ---------------------------------- */

/// What Settings and `rigel mail status` show. No token, and no request: an unconfigured app
/// answers instead of failing, because the page has to render that state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MailStatus {
    pub configured: bool,
    pub inbox: Option<String>,
    pub key_present: bool,
}

/* ---------------------------------- messages ---------------------------------- */

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MessageSummary {
    pub message_id: String,
    pub from: String,
    pub subject: String,
    pub timestamp: Option<String>,
    pub preview: String,
    pub unread: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MessageBody {
    pub message_id: String,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub timestamp: Option<String>,
    /// The whole body, quoted history included.
    pub text: String,
    /// AgentMail's reply extraction: the new content without the quoted part.
    pub extracted_text: Option<String>,
}

/// One conversation in the inbox list. The messages themselves are only fetched when it is
/// opened, so listing a mailbox costs one request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThreadSummary {
    pub thread_id: String,
    pub subject: String,
    pub timestamp: Option<String>,
    pub message_count: usize,
    pub unread: bool,
}

/// One conversation, in the order the provider returns it, which is chronological.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThreadView {
    pub thread_id: String,
    pub subject: String,
    pub messages: Vec<MessageBody>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SentMail {
    pub message_id: String,
    pub thread_id: Option<String>,
}

/* ---------------------------------- the client ---------------------------------- */

pub struct Mail {
    config: MailConfig,
    api_key: String,
    base: String,
    http: reqwest::Client,
}

impl Mail {
    /// The API root, overridable so a test (or a proxy) needs no code change.
    pub fn base_url() -> String {
        std::env::var(BASE_ENV)
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE.to_string())
    }

    pub fn new(config: MailConfig, api_key: impl Into<String>, base: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self {
            config,
            api_key: api_key.into(),
            base: base.into().trim_end_matches('/').to_string(),
            http,
        }
    }

    /// The configured inbox and the stored key, or the error that says which one is missing.
    pub fn open(dir: &Path) -> Result<Self, MailError> {
        let config = MailConfig::load(dir)?.ok_or(MailError::NotConfigured)?;
        let api_key =
            credentials::get(credentials::AGENTMAIL).map_err(|_| MailError::MissingKey)?;
        if api_key.trim().is_empty() {
            return Err(MailError::MissingKey);
        }
        Ok(Self::new(config, api_key, Self::base_url()))
    }

    /// The state of the setup, without a network call.
    pub fn status_at(dir: &Path) -> MailStatus {
        let config = MailConfig::load(dir).ok().flatten();
        MailStatus {
            configured: config.is_some(),
            inbox: config.map(|config| config.inbox),
            key_present: credentials::present(credentials::AGENTMAIL),
        }
    }

    pub fn inbox(&self) -> &str {
        &self.config.inbox
    }

    /// Newest first. `limit` is clamped to what AgentMail will serve in one page.
    pub async fn list(&self, limit: usize) -> Result<Vec<MessageSummary>, MailError> {
        let limit = limit.clamp(1, MAX_LIMIT);
        let url = format!("{}?limit={limit}", self.messages_url());
        let answer = self.get_json(&url).await?;
        Ok(decode_summaries(&answer))
    }

    /// The conversations in the inbox, newest first. Subjects and counts only.
    pub async fn threads(&self, limit: usize) -> Result<Vec<ThreadSummary>, MailError> {
        let limit = limit.clamp(1, MAX_LIMIT);
        let url = format!("{}?limit={limit}", self.threads_url());
        let answer = self.get_json(&url).await?;
        Ok(decode_threads(&answer))
    }

    /// One conversation, oldest message first.
    ///
    /// A thread payload may list its messages by id without their bodies, so any message that
    /// arrives empty is fetched on its own. That is bounded by [`MAX_THREAD_MESSAGES`]: a
    /// conversation longer than that shows its first page rather than becoming a crawl.
    pub async fn thread(&self, thread_id: &str) -> Result<ThreadView, MailError> {
        let url = format!("{}/{}", self.threads_url(), encode_segment(thread_id));
        let answer = self.get_json(&url).await?;
        let mut view = decode_thread(&answer)?;
        view.messages.truncate(MAX_THREAD_MESSAGES);
        for message in view.messages.iter_mut() {
            if message.text.trim().is_empty() {
                let message_id = message.message_id.clone();
                if let Ok(full) = self.read(&message_id).await {
                    *message = full;
                }
            }
        }
        Ok(view)
    }

    pub async fn read(&self, message_id: &str) -> Result<MessageBody, MailError> {
        let url = format!("{}/{}", self.messages_url(), encode_segment(message_id));
        let answer = self.get_json(&url).await?;
        decode_body(&answer)
            .ok_or_else(|| MailError::Malformed("a message with no subject or body".to_string()))
    }

    pub async fn send(&self, to: &str, subject: &str, text: &str) -> Result<SentMail, MailError> {
        let url = format!("{}/send", self.messages_url());
        let answer = self.post_json(&url, &send_body(to, subject, text)).await?;
        decode_sent(&answer)
    }

    pub async fn reply(&self, message_id: &str, text: &str) -> Result<SentMail, MailError> {
        let url = format!(
            "{}/{}/reply",
            self.messages_url(),
            encode_segment(message_id)
        );
        let answer = self.post_json(&url, &json!({ "text": text })).await?;
        decode_sent(&answer)
    }

    fn messages_url(&self) -> String {
        format!(
            "{}/inboxes/{}/messages",
            self.base,
            encode_segment(&self.config.inbox)
        )
    }

    fn threads_url(&self) -> String {
        format!(
            "{}/inboxes/{}/threads",
            self.base,
            encode_segment(&self.config.inbox)
        )
    }

    async fn get_json(&self, url: &str) -> Result<Value, MailError> {
        let response = self.http.get(url).bearer_auth(&self.api_key).send().await?;
        self.answer(response).await
    }

    async fn post_json(&self, url: &str, body: &Value) -> Result<Value, MailError> {
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
            .await?;
        self.answer(response).await
    }

    async fn answer(&self, response: reqwest::Response) -> Result<Value, MailError> {
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(MailError::Rejected {
                status: status.as_u16(),
                message: friendly(&body, status.as_u16()),
            });
        }
        if body.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&body).map_err(|error| {
            MailError::Malformed(format!("{error}: {}", truncate(body.trim(), 200)))
        })
    }
}

/* -------------------------------- the wire shapes -------------------------------- */

/// The documented send payload. `to` is one address here: the tools take one recipient, and a
/// fifty-recipient fan-out is not something a model should do by accident.
fn send_body(to: &str, subject: &str, text: &str) -> Value {
    json!({
        "to": [to.trim()],
        "subject": subject,
        "text": text,
    })
}

fn decode_summaries(answer: &Value) -> Vec<MessageSummary> {
    let Some(messages) = answer.get("messages").and_then(Value::as_array) else {
        return Vec::new();
    };
    messages
        .iter()
        .filter_map(|message| {
            Some(MessageSummary {
                message_id: text_of(message, "message_id")?,
                from: party(message.get("from")),
                subject: text_of(message, "subject").unwrap_or_default(),
                timestamp: text_of(message, "timestamp"),
                preview: text_of(message, "preview").unwrap_or_default(),
                unread: message
                    .get("labels")
                    .and_then(Value::as_array)
                    .map(|labels| labels.iter().any(|label| label == "unread"))
                    .unwrap_or(false),
            })
        })
        .collect()
}

fn decode_threads(answer: &Value) -> Vec<ThreadSummary> {
    let Some(rows) = answer.get("threads").and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            Some(ThreadSummary {
                thread_id: text_of(row, "thread_id")?,
                subject: text_of(row, "subject").unwrap_or_default(),
                timestamp: text_of(row, "timestamp").or_else(|| text_of(row, "updated_at")),
                message_count: row
                    .get("message_ids")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .or_else(|| row.get("messages").and_then(Value::as_array).map(Vec::len))
                    .or_else(|| {
                        row.get("message_count")
                            .and_then(Value::as_u64)
                            .map(|count| count as usize)
                    })
                    .unwrap_or(0),
                unread: row
                    .get("labels")
                    .and_then(Value::as_array)
                    .map(|labels| labels.iter().any(|label| label == "unread"))
                    .unwrap_or(false),
            })
        })
        .collect()
}

fn decode_thread(answer: &Value) -> Result<ThreadView, MailError> {
    let thread_id = text_of(answer, "thread_id")
        .ok_or_else(|| MailError::Malformed("a thread with no id".to_string()))?;
    let messages = answer
        .get("messages")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(decode_body).collect())
        .unwrap_or_default();
    Ok(ThreadView {
        thread_id,
        subject: text_of(answer, "subject").unwrap_or_default(),
        messages,
    })
}

fn decode_body(answer: &Value) -> Option<MessageBody> {
    let extracted_text = text_of(answer, "extracted_text");
    let text = text_of(answer, "text")
        .or_else(|| extracted_text.clone())
        .unwrap_or_default();
    Some(MessageBody {
        message_id: text_of(answer, "message_id")?,
        from: party(answer.get("from")),
        to: answer
            .get("to")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().map(party_value).collect())
            .unwrap_or_default(),
        subject: text_of(answer, "subject").unwrap_or_default(),
        timestamp: text_of(answer, "timestamp"),
        text,
        extracted_text,
    })
}

fn decode_sent(answer: &Value) -> Result<SentMail, MailError> {
    Ok(SentMail {
        message_id: text_of(answer, "message_id")
            .or_else(|| text_of(answer, "id"))
            .ok_or_else(|| MailError::Malformed("a send with no message id".to_string()))?,
        thread_id: text_of(answer, "thread_id"),
    })
}

/// A sender is either `"Name <a@b>"` or `{ "email": …, "name": … }`, depending on the endpoint.
fn party(value: Option<&Value>) -> String {
    value.map(party_value).unwrap_or_default()
}

fn party_value(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    let email = value.get("email").and_then(Value::as_str).unwrap_or("");
    match value.get("name").and_then(Value::as_str) {
        Some(name) if !name.is_empty() && !email.is_empty() => format!("{name} <{email}>"),
        Some(name) if !name.is_empty() => name.to_string(),
        _ => email.to_string(),
    }
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

/// Percent-encode one path segment. Inbox addresses carry `@` and message ids carry `<`, `>`
/// and `@`, and every one of those has to be sent as an escape.
fn encode_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// AgentMail's error bodies are `{name, code, message, fix}`. Translate the ones a user can act
/// on and keep the rest, so a bad key does not read like a network failure.
fn friendly(body: &str, status: u16) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| truncate(body.trim(), 200));
    let fix = parsed
        .as_ref()
        .and_then(|value| value.get("fix"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());

    if status == 401 || status == 403 {
        return "the AgentMail API key was rejected: save a current one with `rigel keys set agentmail`"
            .to_string();
    }
    if status == 404 {
        return format!("{message} (that message or inbox is not on this account)");
    }
    if status == 429 {
        return "AgentMail rate limit: wait a moment and try again".to_string();
    }
    match fix {
        Some(fix) => format!("{message} — {fix}"),
        None => message,
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_needs_one_at_and_a_dotted_domain() {
        assert!(MailConfig::new("rigelagent@agentmail.to").is_ok());
        assert!(MailConfig::new("  rigelagent@agentmail.to  ").is_ok());
        assert!(MailConfig::new("rigelagent").is_err());
        assert!(MailConfig::new("rigel@agent").is_err());
        assert!(MailConfig::new("rigel@@agentmail.to").is_err());
        assert!(MailConfig::new("rigel agent@agentmail.to").is_err());
        assert!(MailConfig::new("").is_err());
    }

    #[test]
    fn path_segments_escape_the_address_characters() {
        assert_eq!(
            encode_segment("rigelagent@agentmail.to"),
            "rigelagent%40agentmail.to"
        );
        assert_eq!(
            encode_segment("<abc@agentmail.to>"),
            "%3Cabc%40agentmail.to%3E"
        );
        assert_eq!(encode_segment("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn the_send_body_holds_one_recipient() {
        let body = send_body("  someone@example.com ", "Hi", "Body");
        assert_eq!(body["to"], json!(["someone@example.com"]));
        assert_eq!(body["subject"], json!("Hi"));
        assert_eq!(body["text"], json!("Body"));
    }

    #[test]
    fn a_list_answer_becomes_summaries_and_reads_unread() {
        let answer = json!({
            "count": 1,
            "messages": [{
                "message_id": "<abc@google.com>",
                "from": "Firebase <firebase-noreply@google.com>",
                "subject": "You were invited",
                "preview": "Accept the invitation",
                "timestamp": "2026-10-31T06:23:43.000Z",
                "labels": ["received", "unread"]
            }]
        });
        let rows = decode_summaries(&answer);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].message_id, "<abc@google.com>");
        assert_eq!(rows[0].from, "Firebase <firebase-noreply@google.com>");
        assert!(rows[0].unread);
        assert_eq!(rows[0].subject, "You were invited");
    }

    #[test]
    fn a_body_prefers_text_and_keeps_the_extraction() {
        let answer = json!({
            "message_id": "<abc@google.com>",
            "from": {"email": "someone@example.com", "name": "Someone"},
            "to": [{"email": "rigelagent@agentmail.to", "name": "Rigel"}],
            "subject": "Re: hello",
            "text": "Thanks\n\nOn ... wrote:\n> hello",
            "extracted_text": "Thanks"
        });
        let body = decode_body(&answer).expect("a body");
        assert_eq!(body.from, "Someone <someone@example.com>");
        assert_eq!(body.to, vec!["Rigel <rigelagent@agentmail.to>".to_string()]);
        assert_eq!(body.extracted_text.as_deref(), Some("Thanks"));
        assert!(body.text.starts_with("Thanks"));
    }

    #[test]
    fn a_thread_answer_becomes_an_ordered_conversation() {
        let answer = json!({
            "thread_id": "t-1",
            "subject": "Hello",
            "messages": [
                { "message_id": "<a@x>", "from": "A <a@x>", "subject": "Hello", "text": "first" },
                { "message_id": "<b@y>", "from": "B <b@y>", "subject": "Re: Hello", "text": "second" }
            ]
        });
        let view = decode_thread(&answer).unwrap();
        assert_eq!(view.thread_id, "t-1");
        assert_eq!(view.messages.len(), 2);
        assert_eq!(view.messages[0].text, "first");
        assert_eq!(view.messages[1].text, "second");
        assert!(decode_thread(&json!({ "subject": "no id" })).is_err());
    }

    #[test]
    fn a_thread_list_counts_what_it_was_given() {
        let answer = json!({
            "count": 2,
            "threads": [
                { "thread_id": "t-1", "subject": "Hello", "timestamp": "2026-10-31T06:23:43.000Z", "message_ids": ["a", "b"], "labels": ["unread"] },
                { "thread_id": "t-2", "subject": "Quiet", "message_count": 3 }
            ]
        });
        let rows = decode_threads(&answer);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].message_count, 2);
        assert!(rows[0].unread);
        assert_eq!(rows[1].message_count, 3);
        assert!(!rows[1].unread);
    }

    #[test]
    fn a_send_answer_needs_an_id() {
        let sent = decode_sent(&json!({ "message_id": "<x@y>", "thread_id": "t1" })).unwrap();
        assert_eq!(sent.message_id, "<x@y>");
        assert_eq!(sent.thread_id.as_deref(), Some("t1"));
        assert!(decode_sent(&json!({ "ok": true })).is_err());
    }

    #[test]
    fn a_rejected_key_says_what_to_do() {
        let text = friendly(r#"{"name":"Unauthorized","message":"bad key"}"#, 401);
        assert!(text.contains("rigel keys set agentmail"), "{text}");
        assert!(friendly("", 429).contains("rate limit"));
        let other = friendly(
            r#"{"message":"Inbox not found","fix":"check the address"}"#,
            400,
        );
        assert!(other.contains("Inbox not found"));
        assert!(other.contains("check the address"));
    }
}

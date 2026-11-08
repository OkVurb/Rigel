//! The remote agent as a model door.
//!
//! Selecting `rigel` in the picker does not call an inference provider. It posts one bounded
//! `operator.request` through the askrigel relay and waits for the reply whose `reply_to` matches
//! that turn, so the answer comes from the agent on the other end of the relay with its own
//! context and its own tools.
//!
//! The wait is bounded on purpose. A relay that accepts a request while the remote poller is not
//! running would otherwise keep the composer open forever; after [`DEFAULT_TIMEOUT`] the turn
//! fails with the one sentence that names the cause.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::model::{ChatRequest, Delta, DeltaSink, Model, ModelError};

/// The only id this door serves, and the one the picker shows.
pub const DEFAULT_MODEL: &str = "rigel";
pub const DEFAULT_BASE_URL: &str = "https://api.askrigel.com";
/// Every turn's `reply_to` starts with this. The desktop relay loop leaves those envelopes for the
/// turn that asked, so the room bridge cannot consume the answer first.
pub const REQUEST_PREFIX: &str = "rigel-model-";
const POLL_LIMIT: u64 = 50;
const MAX_WAIT_SECONDS: u64 = 20;
const MAX_PROMPT_CHARS: usize = 20_000;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

pub struct RigelModel {
    client: reqwest::Client,
    token: String,
    base_url: String,
    timeout: Duration,
}

impl RigelModel {
    pub fn new(token: impl Into<String>, base_url: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            // Each read waits at most `MAX_WAIT_SECONDS`; the client bound is above that.
            .timeout(Duration::from_secs(40))
            .build()
            .unwrap_or_default();
        Self {
            client,
            token: token.into(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Tests and a headless run can shorten the wait; a turn never hangs past this.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.max(Duration::from_secs(1));
        self
    }
}

#[async_trait]
impl Model for RigelModel {
    fn id(&self) -> String {
        DEFAULT_MODEL.to_string()
    }

    async fn chat(
        &self,
        request: ChatRequest,
        cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError> {
        let prompt = prompt_of(&request);
        if prompt.is_empty() {
            return Err(ModelError::Other(
                "the rigel model needs a user prompt".to_string(),
            ));
        }

        let request_id = format!("{REQUEST_PREFIX}{}", uuid::Uuid::new_v4());
        let project = std::env::current_dir()
            .ok()
            .map(|path| path.to_string_lossy().into_owned());
        let body = json!({
            "kind": "request",
            "idempotency_key": request_id,
            "payload": {
                "type": "operator.request",
                "source": "rigel-model",
                "request_id": request_id,
                "title": "Rigel chat",
                "prompt": prompt,
                "project": project,
                "constraints": "Answer in one message, as the remote Rigel agent. This is a chat \
                                turn: do not start a swarm job for it.",
                "files": [],
                "max_steps": 1,
                "approve": false,
                "reply_to": request_id,
            }
        });

        let sent = tokio::select! {
            _ = cancel.cancelled() => return Err(ModelError::Cancelled),
            sent = self
                .client
                .post(format!("{}/v1/relay/messages", self.base_url))
                .bearer_auth(&self.token)
                .json(&body)
                .send() => sent,
        }
        .map_err(|error| {
            ModelError::Http(format!("the relay did not accept the request: {error}"))
        })?;
        if sent.status().as_u16() != 202 {
            let status = sent.status().as_u16();
            let body = sent.text().await.unwrap_or_default();
            return Err(ModelError::Status {
                status,
                body,
                retry_after: None,
            });
        }

        let deadline = Instant::now() + self.timeout;
        let mut after = 0u64;
        loop {
            if cancel.is_cancelled() {
                return Err(ModelError::Cancelled);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(ModelError::Other(format!(
                    "rigel did not answer within {}s — the remote agent is not polling. Start its \
                     poller, then try again.",
                    self.timeout.as_secs()
                )));
            }
            let wait = left.as_secs().clamp(1, MAX_WAIT_SECONDS);
            let url = format!(
                "{}/v1/relay/messages?after={after}&limit={POLL_LIMIT}&wait={wait}",
                self.base_url
            );
            let response = tokio::select! {
                _ = cancel.cancelled() => return Err(ModelError::Cancelled),
                response = self.client.get(&url).bearer_auth(&self.token).send() => response,
            }
            .map_err(|error| {
                ModelError::Http(format!("the relay did not answer the read: {error}"))
            })?;
            if !response.status().is_success() {
                let status = response.status().as_u16();
                let body = response.text().await.unwrap_or_default();
                return Err(ModelError::Status {
                    status,
                    body,
                    retry_after: None,
                });
            }
            let page: Value = response.json().await.map_err(|error| {
                ModelError::Parse(format!("the relay returned unreadable data: {error}"))
            })?;
            let messages = page
                .get("messages")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for message in &messages {
                let payload = message.get("payload").unwrap_or(&Value::Null);
                if !answers(payload, &request_id) {
                    continue;
                }
                let Some(id) = message.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let text = reply_text(payload).unwrap_or_else(|| payload.to_string());
                acknowledge(&self.client, &self.base_url, &self.token, id).await;
                for piece in chunks(&text, 400) {
                    on_delta(Delta::Text(piece));
                }
                on_delta(Delta::Finish("stop".to_string()));
                return Ok(());
            }
            after = page
                .get("next_after")
                .and_then(Value::as_u64)
                .unwrap_or(after);
        }
    }
}

/// The last user line is the prompt; the coordinator has already folded the system brief into the
/// history, and the remote agent does not need a second one.
fn prompt_of(request: &ChatRequest) -> String {
    let text = request
        .messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .and_then(|message| message.content.as_deref())
        .unwrap_or_default()
        .trim();
    text.chars().take(MAX_PROMPT_CHARS).collect()
}

/// A reply belongs to this turn only when it names it. The relay inbox is shared with the room
/// bridge, so matching on the id is what keeps one answer from being read as another.
fn answers(payload: &Value, request_id: &str) -> bool {
    ["reply_to", "request_id"]
        .iter()
        .any(|key| payload.get(*key).and_then(Value::as_str) == Some(request_id))
}

fn reply_text(payload: &Value) -> Option<String> {
    ["text", "message", "body", "summary", "answer", "content"]
        .iter()
        .find_map(|key| payload.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// Acknowledge is best effort: the answer is already in hand, and a failed ack only means the
/// relay will offer it again, which the id match ignores.
async fn acknowledge(client: &reqwest::Client, base: &str, token: &str, id: &str) {
    let _ = client
        .post(format!("{base}/v1/relay/messages/{id}/ack"))
        .bearer_auth(token)
        .send()
        .await;
}

fn chunks(text: &str, size: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut count = 0usize;
    for ch in text.chars() {
        current.push(ch);
        count += 1;
        if count >= size {
            out.push(std::mem::take(&mut current));
            count = 0;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::model::ChatMessage;
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let read = socket.read(&mut chunk).await.unwrap_or(0);
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            let text = String::from_utf8_lossy(&buffer).to_string();
            if let Some(head_end) = text.find("\r\n\r\n") {
                let length = text[..head_end]
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if buffer.len() >= head_end + 4 + length {
                    return text;
                }
            }
        }
        String::from_utf8_lossy(&buffer).to_string()
    }

    #[tokio::test]
    async fn a_turn_waits_for_the_reply_addressed_to_it() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(String::new()));
        let server_seen = seen.clone();
        tokio::spawn(async move {
            let mut request_id = String::new();
            for _ in 0..3 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let request = read_request(&mut socket).await;
                let first = request.lines().next().unwrap_or_default().to_string();
                let (status, body) = if first.starts_with("POST /v1/relay/messages ") {
                    let body = request.split("\r\n\r\n").nth(1).unwrap_or("{}");
                    let value: Value = serde_json::from_str(body).unwrap_or(Value::Null);
                    request_id = value
                        .get("idempotency_key")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    server_seen
                        .lock()
                        .unwrap()
                        .push_str(&request.to_ascii_lowercase());
                    (202u16, json!({"message": {"seq": 1}}).to_string())
                } else if first.starts_with("GET /v1/relay/messages") {
                    (
                        200,
                        json!({
                            "messages": [{
                                "seq": 2,
                                "id": "11111111-1111-1111-1111-111111111111",
                                "kind": "response",
                                "payload": {
                                    "type": "muse.response",
                                    "reply_to": request_id,
                                    "text": "answer from rigel"
                                }
                            }],
                            "next_after": 2
                        })
                        .to_string(),
                    )
                } else {
                    (200, "{}".to_string())
                };
                let response = format!(
                    "HTTP/1.1 {status} Answer\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });

        let model = RigelModel::new("test-token", format!("http://{address}"))
            .with_timeout(Duration::from_secs(5));
        let request = ChatRequest {
            model: DEFAULT_MODEL.to_string(),
            messages: vec![ChatMessage::user("say hello")],
            tools: Vec::new(),
            max_tokens: None,
            temperature: 0.0,
            effort: None,
        };
        let cancel = CancellationToken::new();
        let mut text = String::new();
        let outcome = model
            .chat(request, &cancel, &mut |delta| {
                if let Delta::Text(piece) = delta {
                    text.push_str(&piece);
                }
            })
            .await;
        assert!(outcome.is_ok(), "{:?}", outcome.err());
        assert_eq!(text, "answer from rigel");
        let seen = seen.lock().unwrap();
        assert!(seen.contains("say hello"), "prompt reached the relay");
        assert!(seen.contains("operator.request"), "request is typed");
    }

    #[test]
    fn only_the_addressed_reply_matches() {
        let payload = json!({"reply_to": "rigel-model-a", "text": "hi"});
        assert!(answers(&payload, "rigel-model-a"));
        assert!(!answers(&payload, "rigel-model-b"));
        assert_eq!(reply_text(&payload).as_deref(), Some("hi"));
        assert_eq!(chunks("abcdef", 2), vec!["ab", "cd", "ef"]);
    }
}

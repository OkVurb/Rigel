//! Anthropic's Messages API, streamed.
//!
//! Claude is not an OpenAI-compatible door with a different URL, which is why this is a module
//! and not a row in [`super::openrouter`]: the system prompt is a **top-level field**, `max_tokens`
//! is **required**, tool results travel as `tool_result` blocks inside a `user` message, and the
//! stream is a sequence of named events whose usage is split across `message_start` and
//! `message_delta`.
//!
//! The credential is sent in **both** forms the platform documents — `Authorization: Bearer` (its
//! own current examples) and `x-api-key` (what every client SDK sends). A live request on this
//! machine reached the API and was answered `401 invalid x-api-key`, which says the stored key is
//! not one this account can use, not which header carried it; sending the pair removes the
//! question rather than leaving it to whoever holds a working key.
//!
//! Two request fields are deliberately not sent, both because the current API version says so
//! (checked against the platform docs, 27 Oct 2026):
//!
//! * `temperature` — removed in API version [`API_VERSION`]; a request that sends it is a 400 on
//!   every model. [`ChatRequest::temperature`] is simply not part of this wire contract.
//! * a `thinking` object — manual `budget_tokens` thinking is deprecated on the 4.6 generation and
//!   rejected by 4.7 and later, and adaptive thinking needs no switch. The request's `effort`
//!   becomes `output_config.effort` (`low`/`medium`/`high` are the platform's own levels), which is
//!   the documented steering lever and is valid with no `thinking` field at all.
//!
//! Everything else is shared with the other adapters: the SSE decoder, the tool-call assembly,
//! the retry policy and the `Delta`s are the same types every consumer already handles.
//!
//! The roster is Anthropic's own — `GET {base}/models` answers with `data[].id` — so the picker
//! reads live ids rather than a list written down here.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::model::{
    ChatMessage, ChatRequest, Delta, DeltaSink, Model, ModelError, RawUsage, SseDecoder,
    jitter_delay,
};

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com/v1";

/// The dated API version every request carries, the model list included.
pub const API_VERSION: &str = "2026-10-21";

/// What the docs recommend when nothing has been chosen ("start with Claude Opus 5.5 for most
/// workloads"), in the id form the API uses.
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

/// Fixed headers this API needs beyond the credential. The adapter table hands them to the
/// catalogue reader, so the picker sends them too.
pub const HEADERS: &[(&str, &str)] = &[("anthropic-version", API_VERSION)];

/// `max_tokens` is required here — the one request field with no default of its own. A caller
/// that does not name one gets this.
const DEFAULT_MAX_TOKENS: u32 = 8_192;

pub struct AnthropicModel {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    max_attempts: u32,
}

impl AnthropicModel {
    pub fn new(api_key: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .connect_timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default();
        Self {
            client,
            api_key: api_key.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
            max_attempts: 3,
        }
    }

    /// For tests and a gateway that fronts the same API.
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        let mut model = Self::new(api_key);
        model.base_url = base_url.into().trim_end_matches('/').to_string();
        model
    }

    pub fn with_attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = attempts.max(1);
        self
    }

    /// The Messages body. `temperature` is absent on purpose: API version [`API_VERSION`] removed
    /// the sampling parameters and answers a request that carries one with a 400.
    fn body(&self, request: &ChatRequest) -> Value {
        let (system, messages) = encode_messages(&request.messages);
        let mut body = json!({
            // Never a batch-only id: the provider would route it to its batch endpoint and answer
            // 404 instead of streaming a turn.
            "model": super::model::chat_model_id(&request.model),
            "max_tokens": request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            "messages": messages,
            "stream": true,
        });
        if let Some(system) = system {
            body["system"] = json!(system);
        }
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(
                request
                    .tools
                    .iter()
                    .map(|spec| {
                        json!({
                            "name": spec.name,
                            "description": spec.description,
                            "input_schema": spec.parameters,
                        })
                    })
                    .collect(),
            );
            body["tool_choice"] = json!({ "type": "auto" });
        }
        if let Some(effort) = &request.effort {
            body["output_config"] = json!({ "effort": effort });
        }
        body
    }

    async fn stream_response(
        &self,
        response: reqwest::Response,
        cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError> {
        let mut stream = response.bytes_stream();
        let mut decoder = SseDecoder::default();
        let mut emitted = false;
        // Usage arrives in two pieces (`message_start` carries the input side, `message_delta`
        // the output side), and a consumer *replaces* its usage with each `Delta::Usage`. So the
        // full picture is re-sent on the second one rather than only what changed.
        let mut usage = RawUsage::default();
        // Tool arguments arrive either as JSON fragments or, whole, in the block's `input`. Which
        // one happened decides what is emitted for the block.
        let mut start_inputs: BTreeMap<u32, Value> = BTreeMap::new();
        let mut streamed: BTreeSet<u32> = BTreeSet::new();

        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(ModelError::Cancelled),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else {
                break;
            };
            let bytes = chunk.map_err(|error| ModelError::Http(error.to_string()))?;
            for payload in decoder.feed(&bytes) {
                let value: Value = serde_json::from_str(&payload)
                    .map_err(|error| ModelError::Parse(format!("{error}: {payload}")))?;

                if let Some(error) = value.get("error") {
                    return Err(ModelError::Other(
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("provider error")
                            .to_string(),
                    ));
                }

                match value.get("type").and_then(Value::as_str) {
                    Some("message_start") => {
                        let reported = value.pointer("/message/usage");
                        // `input_tokens` excludes both cache fields, and a cache read is billed at
                        // a discount; counting all three as input with the read marked cached is
                        // the closest this table gets without a cache-write rate of its own.
                        let field = |name: &str| {
                            reported
                                .and_then(|usage| usage.get(name))
                                .and_then(Value::as_u64)
                                .unwrap_or(0)
                        };
                        usage.cached_input_tokens = field("cache_read_input_tokens");
                        usage.input_tokens = field("input_tokens")
                            + field("cache_creation_input_tokens")
                            + usage.cached_input_tokens;
                        usage.output_tokens = field("output_tokens");
                        emitted = true;
                        on_delta(Delta::Usage(usage));
                    }
                    Some("content_block_start") => {
                        let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let block = value.get("content_block");
                        if block
                            .and_then(|block| block.get("type"))
                            .and_then(Value::as_str)
                            == Some("tool_use")
                        {
                            let id = block
                                .and_then(|block| block.get("id"))
                                .and_then(Value::as_str);
                            let name = block
                                .and_then(|block| block.get("name"))
                                .and_then(Value::as_str);
                            let input = block.and_then(|block| block.get("input"));
                            if let Some(input) = input.filter(|input| input.is_object()) {
                                start_inputs.insert(index, input.clone());
                            }
                            emitted = true;
                            on_delta(Delta::ToolCall {
                                index,
                                id: id.map(str::to_string),
                                name: name.map(str::to_string),
                                arguments: String::new(),
                            });
                        }
                    }
                    Some("content_block_delta") => {
                        let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
                        let delta = value.get("delta");
                        let kind = delta
                            .and_then(|delta| delta.get("type"))
                            .and_then(Value::as_str);
                        match kind {
                            Some("text_delta") => {
                                if let Some(text) = delta
                                    .and_then(|delta| delta.get("text"))
                                    .and_then(Value::as_str)
                                {
                                    if !text.is_empty() {
                                        emitted = true;
                                        on_delta(Delta::Text(text.to_string()));
                                    }
                                }
                            }
                            Some("thinking_delta") => {
                                if let Some(text) = delta
                                    .and_then(|delta| delta.get("thinking"))
                                    .and_then(Value::as_str)
                                {
                                    if !text.is_empty() {
                                        emitted = true;
                                        on_delta(Delta::Reasoning(text.to_string()));
                                    }
                                }
                            }
                            Some("input_json_delta") => {
                                let fragment = delta
                                    .and_then(|delta| delta.get("partial_json"))
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                streamed.insert(index);
                                emitted = true;
                                on_delta(Delta::ToolCall {
                                    index,
                                    id: None,
                                    name: None,
                                    arguments: fragment.to_string(),
                                });
                            }
                            // `signature_delta` and `citations_delta` are not part of a reply a
                            // caller can use; both are read and dropped.
                            _ => {}
                        }
                    }
                    Some("content_block_stop") => {
                        let index = value.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
                        // A block whose arguments arrived whole at its start never streamed a
                        // fragment, so this is where they are emitted — once, as valid JSON.
                        if !streamed.contains(&index)
                            && let Some(input) = start_inputs.get(&index)
                            && !input.as_object().is_some_and(|object| object.is_empty())
                        {
                            emitted = true;
                            on_delta(Delta::ToolCall {
                                index,
                                id: None,
                                name: None,
                                arguments: input.to_string(),
                            });
                        }
                    }
                    Some("message_delta") => {
                        if let Some(reason) =
                            value.pointer("/delta/stop_reason").and_then(Value::as_str)
                        {
                            on_delta(Delta::Finish(reason.to_string()));
                        }
                        let reported = value.get("usage");
                        if let Some(output) = reported
                            .and_then(|usage| usage.get("output_tokens"))
                            .and_then(Value::as_u64)
                        {
                            usage.output_tokens = output;
                        }
                        // Thinking is billed as output; this breakdown is what says how much of
                        // it was reasoning, and it only appears on this event.
                        usage.reasoning_tokens = reported
                            .and_then(|usage| {
                                usage.pointer("/output_tokens_details/thinking_tokens")
                            })
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        emitted = true;
                        on_delta(Delta::Usage(usage));
                    }
                    // `ping` and `message_stop` carry nothing a caller acts on.
                    _ => {}
                }
            }
        }

        // A stream that ended without a token and without an event is a dead connection, not an
        // empty answer.
        if !emitted {
            return Err(ModelError::Parse(
                "the stream ended before any content arrived".to_string(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl Model for AnthropicModel {
    fn id(&self) -> String {
        "anthropic".to_string()
    }

    async fn chat(
        &self,
        request: ChatRequest,
        cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError> {
        let body = self.body(&request);
        let mut attempt = 0;

        loop {
            attempt += 1;
            let send = self
                .client
                .post(format!("{}/messages", self.base_url))
                .bearer_auth(&self.api_key)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", API_VERSION)
                .header("accept", "text/event-stream")
                .json(&body)
                .send();

            let response = tokio::select! {
                _ = cancel.cancelled() => return Err(ModelError::Cancelled),
                response = send => response,
            };

            match response {
                Ok(response) if response.status().is_success() => {
                    return self.stream_response(response, cancel, on_delta).await;
                }
                Ok(response) => {
                    let status = response.status().as_u16();
                    let retry_after = retry_after_header(&response);
                    // 429 is the rate limit; 529 is Anthropic's overloaded, and every 5xx is
                    // worth one more try.
                    if is_retryable(status) && attempt < self.max_attempts {
                        let wait = retry_after
                            .map(Duration::from_secs)
                            .unwrap_or_else(|| jitter_delay(attempt, 250));
                        tokio::time::sleep(wait).await;
                        continue;
                    }
                    let body = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "<unreadable body>".to_string());
                    return Err(ModelError::Status {
                        status,
                        body: truncate(&body, 600),
                        retry_after,
                    });
                }
                Err(error) => {
                    let retryable = error.is_timeout() || error.is_connect() || error.is_request();
                    if retryable && attempt < self.max_attempts {
                        tokio::time::sleep(jitter_delay(attempt, 250)).await;
                        continue;
                    }
                    return Err(ModelError::Http(error.to_string()));
                }
            }
        }
    }
}

/// The system prompt (hoisted out of the message list) and the messages, in the shapes this API
/// accepts: alternating roles, tool calls as `tool_use` blocks on an assistant turn and their
/// results as `tool_result` blocks on the `user` turn that answers it.
fn encode_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<Value>) {
    let mut system: Vec<String> = Vec::new();
    let mut encoded: Vec<(&'static str, Vec<Value>)> = Vec::new();

    for message in messages {
        if message.role == "system" {
            if let Some(text) = message
                .content
                .as_deref()
                .filter(|text| !text.trim().is_empty())
            {
                system.push(text.to_string());
            }
            continue;
        }
        // A tool result is a `user` turn here, not a third role.
        let role = if message.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        let mut blocks: Vec<Value> = Vec::new();

        if let Some(id) = message.tool_call_id.as_deref() {
            blocks.push(json!({
                "type": "tool_result",
                "tool_use_id": id,
                "content": message.content.clone().unwrap_or_default(),
            }));
        } else if let Some(text) = message
            .content
            .as_deref()
            .filter(|text| !text.trim().is_empty())
        {
            blocks.push(json!({ "type": "text", "text": text }));
        }

        for call in message.tool_calls.iter().flatten() {
            blocks.push(json!({
                "type": "tool_use",
                "id": call.id,
                "name": call.name,
                // This API wants an object. A stored call whose arguments did not parse is sent
                // as `{}`: the tool result that follows already carries the failure, and the
                // pairing between the two has to stay valid.
                "input": serde_json::from_str::<Value>(&call.arguments).unwrap_or_else(|_| json!({})),
            }));
        }

        // An empty turn cannot be represented: no text, no call, no result.
        if blocks.is_empty() {
            continue;
        }
        // Consecutive turns of one role are **one** turn here, which is also what makes two tool
        // results in a row a single `user` message.
        match encoded.last_mut() {
            Some((last_role, last_blocks)) if *last_role == role => last_blocks.extend(blocks),
            _ => encoded.push((role, blocks)),
        }
    }

    let messages = encoded
        .into_iter()
        .map(|(role, blocks)| json!({ "role": role, "content": blocks }))
        .collect();
    let system = (!system.is_empty()).then(|| system.join("\n\n"));
    (system, messages)
}

fn is_retryable(status: u16) -> bool {
    status == 429 || status >= 500
}

fn retry_after_header(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use crate::agent::model::{ToolCall, ToolCallBuilder, ToolSpec};

    /// The events a real stream sends for `text "pong"`, one fragmented tool call and one whose
    /// arguments arrive whole, plus the two usage events.
    const STREAM: &str = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"role\":\"assistant\",\
         \"usage\":{\"input_tokens\":11,\"cache_creation_input_tokens\":4,\
         \"cache_read_input_tokens\":5,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\
         \"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\
         \"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hmm\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\
         \"delta\":{\"type\":\"text_delta\",\"text\":\"po\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\
         \"delta\":{\"type\":\"signature_delta\",\"signature\":\"abc\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\
         \"delta\":{\"type\":\"text_delta\",\"text\":\"ng\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\
         \"id\":\"toolu_7\",\"name\":\"read_file\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\
         \"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\
         \"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"b.txt\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\
         \"id\":\"toolu_8\",\"name\":\"list_files\",\"input\":{\"path\":\"src\"}}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\
         \"usage\":{\"output_tokens\":9,\"output_tokens_details\":{\"thinking_tokens\":3}}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    /// One request, read fully, answered with `answer`.
    async fn fake_anthropic(answer: &'static str) -> (String, Arc<Mutex<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(String::new()));
        let log = seen.clone();

        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = Vec::new();
            let mut buffer = vec![0u8; 4096];
            loop {
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if let Some(end) = find_headers_end(&request) {
                    let head = String::from_utf8_lossy(&request[..end]).to_string();
                    if request.len() >= end + content_length(&head).unwrap_or(0) {
                        break;
                    }
                }
            }
            *log.lock().unwrap() = String::from_utf8_lossy(&request).to_string();

            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                answer.len()
            );
            let _ = socket.write_all(header.as_bytes()).await;
            let _ = socket.write_all(answer.as_bytes()).await;
            let _ = socket.shutdown().await;
        });

        (format!("http://{address}"), seen)
    }

    fn find_headers_end(request: &[u8]) -> Option<usize> {
        request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|index| index + 4)
    }

    fn content_length(head: &str) -> Option<usize> {
        head.lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse().ok())
    }

    /// The streamed reply, as a caller sees it.
    struct Collected {
        text: String,
        reasoning: String,
        calls: Vec<ToolCall>,
        usage: RawUsage,
        finished: Option<String>,
    }

    fn collect(deltas: &mut Vec<Delta>, usage: RawUsage, finished: Option<String>) -> Collected {
        let mut builder = ToolCallBuilder::default();
        let mut text = String::new();
        let mut reasoning = String::new();
        for delta in deltas.drain(..) {
            match delta {
                Delta::Text(chunk) => text.push_str(&chunk),
                Delta::Reasoning(chunk) => reasoning.push_str(&chunk),
                Delta::ToolCall {
                    index,
                    id,
                    name,
                    arguments,
                } => builder.apply(index, id.as_deref(), name.as_deref(), &arguments),
                _ => {}
            }
        }
        Collected {
            text,
            reasoning,
            calls: builder.finish(),
            usage,
            finished,
        }
    }

    fn request(model: &str, messages: Vec<ChatMessage>, tools: Vec<ToolSpec>) -> ChatRequest {
        ChatRequest {
            model: model.to_string(),
            messages,
            tools,
            max_tokens: Some(512),
            // Removed from this API version: it must not reach the wire.
            temperature: 0.2,
            effort: None,
        }
    }

    #[test]
    fn the_body_omits_temperature_and_sends_effort_as_output_config() {
        let model = AnthropicModel::new("sk-ant-test");
        let mut asked = request(
            "claude-opus-5-5",
            vec![ChatMessage::system("be brief"), ChatMessage::user("hi")],
            vec![ToolSpec {
                name: "read_file".to_string(),
                description: "read a file".to_string(),
                parameters: json!({ "type": "object", "properties": {} }),
            }],
        );
        asked.effort = Some("high".to_string());

        let body = model.body(&asked);
        // API version 2026-10-21 answers a request carrying any of these with a 400.
        for removed in ["temperature", "top_p", "top_k"] {
            assert!(body.get(removed).is_none(), "{removed} must not be sent");
        }
        assert_eq!(body["output_config"]["effort"], json!("high"));
        assert!(
            body.get("thinking").is_none(),
            "adaptive thinking needs none"
        );
        assert_eq!(body["system"], json!("be brief"));
        assert_eq!(body["max_tokens"], json!(512));
        assert_eq!(body["stream"], json!(true));
        // The model list travels without the OpenAI wrapper.
        assert_eq!(body["tools"][0]["name"], json!("read_file"));
        assert_eq!(body["tools"][0]["input_schema"]["type"], json!("object"));
        assert_eq!(body["tool_choice"]["type"], json!("auto"));
        assert_eq!(body["messages"][0]["role"], json!("user"));
    }

    #[test]
    fn max_tokens_has_a_default_because_this_api_requires_one() {
        let model = AnthropicModel::new("sk-ant-test");
        let mut asked = request("claude-opus-5-5", vec![ChatMessage::user("hi")], Vec::new());
        asked.max_tokens = None;
        assert_eq!(model.body(&asked)["max_tokens"], json!(DEFAULT_MAX_TOKENS));
    }

    #[test]
    fn tool_results_and_consecutive_turns_become_one_user_message() {
        let (system, messages) = encode_messages(&[
            ChatMessage::system("first"),
            ChatMessage::system("second"),
            ChatMessage::user("read two files"),
            ChatMessage::assistant(
                None,
                vec![
                    ToolCall {
                        id: "toolu_1".to_string(),
                        name: "read_file".to_string(),
                        arguments: "{\"path\":\"a.txt\"}".to_string(),
                    },
                    ToolCall {
                        id: "toolu_2".to_string(),
                        name: "read_file".to_string(),
                        // Arguments that did not parse still have to pair with their result.
                        arguments: "{not json".to_string(),
                    },
                ],
            ),
            ChatMessage::tool("toolu_1", "alpha"),
            ChatMessage::tool("toolu_2", "beta"),
            // An empty turn cannot be represented and is dropped.
            ChatMessage::assistant(None, Vec::new()),
            ChatMessage::user("now edit"),
        ]);

        assert_eq!(system.as_deref(), Some("first\n\nsecond"));
        assert_eq!(messages.len(), 3, "{messages:?}");
        assert_eq!(messages[0]["role"], json!("user"));
        assert_eq!(messages[0]["content"][0]["text"], json!("read two files"));
        assert_eq!(messages[1]["role"], json!("assistant"));
        assert_eq!(messages[1]["content"][0]["type"], json!("tool_use"));
        assert_eq!(messages[1]["content"][0]["input"]["path"], json!("a.txt"));
        assert_eq!(messages[1]["content"][1]["input"], json!({}));
        // Both results answer the same assistant turn, so they are one message — and the next
        // user turn joins it, because this API wants the roles to alternate.
        assert_eq!(messages[2]["role"], json!("user"));
        assert_eq!(messages[2]["content"].as_array().unwrap().len(), 3);
        assert_eq!(messages[2]["content"][0]["type"], json!("tool_result"));
        assert_eq!(messages[2]["content"][1]["tool_use_id"], json!("toolu_2"));
        assert_eq!(messages[2]["content"][1]["content"], json!("beta"));
        assert_eq!(messages[2]["content"][2]["text"], json!("now edit"));
    }

    #[tokio::test]
    async fn a_turn_streams_text_tool_calls_and_cumulative_usage() {
        let (base, seen) = fake_anthropic(STREAM).await;
        let model = AnthropicModel::with_base_url("sk-ant-test", base);
        assert_eq!(model.id(), "anthropic");

        let asked = request(
            "claude-opus-5-5",
            vec![
                ChatMessage::system("be brief"),
                ChatMessage::user("read b.txt"),
                ChatMessage::assistant(
                    None,
                    vec![ToolCall {
                        id: "toolu_1".to_string(),
                        name: "read_file".to_string(),
                        arguments: "{\"path\":\"b.txt\"}".to_string(),
                    }],
                ),
                ChatMessage::tool("toolu_1", "beta"),
            ],
            vec![ToolSpec {
                name: "read_file".to_string(),
                description: "read a file".to_string(),
                parameters: json!({ "type": "object", "properties": {} }),
            }],
        );

        let cancel = CancellationToken::new();
        let mut deltas = Vec::new();
        let mut usage = RawUsage::default();
        let mut finished = None;
        {
            let mut sink = |delta: Delta| {
                match &delta {
                    Delta::Usage(raw) => usage = *raw,
                    Delta::Finish(reason) => finished = Some(reason.clone()),
                    _ => {}
                }
                deltas.push(delta);
            };
            model.chat(asked, &cancel, &mut sink).await.unwrap();
        }
        let collected = collect(&mut deltas, usage, finished);

        assert_eq!(collected.text, "pong");
        assert_eq!(collected.reasoning, "hmm");
        assert_eq!(collected.finished.as_deref(), Some("tool_use"));
        // Both assembly paths: fragments streamed, and arguments that arrived whole.
        assert_eq!(collected.calls.len(), 2);
        assert_eq!(collected.calls[0].name, "read_file");
        assert_eq!(collected.calls[0].arguments, "{\"path\":\"b.txt\"}");
        assert_eq!(collected.calls[1].name, "list_files");
        assert_eq!(collected.calls[1].arguments, "{\"path\":\"src\"}");
        // Input counts the cache fields; the read is marked cached; thinking is reported.
        assert_eq!(collected.usage.input_tokens, 20);
        assert_eq!(collected.usage.cached_input_tokens, 5);
        assert_eq!(collected.usage.output_tokens, 9);
        assert_eq!(collected.usage.reasoning_tokens, 3);
        assert_eq!(
            collected.usage.cost_usd, None,
            "this API bills, it does not price"
        );

        let request = seen.lock().unwrap().clone();
        assert!(request.starts_with("POST /messages HTTP/1.1"), "{request}");
        // Both documented credential forms, so a deployment that honours only one still serves
        // the turn.
        let lower = request.to_lowercase();
        assert!(
            lower.contains("authorization: bearer sk-ant-test"),
            "{request}"
        );
        assert!(lower.contains("x-api-key: sk-ant-test"), "{request}");
        assert!(
            lower.contains(&format!("anthropic-version: {API_VERSION}")),
            "{request}"
        );
        assert!(
            request.contains("\"model\":\"claude-opus-5-5\""),
            "{request}"
        );
        assert!(request.contains("\"system\":\"be brief\""), "{request}");
        assert!(request.contains("\"type\":\"tool_result\""), "{request}");
        assert!(!request.contains("temperature"), "{request}");
    }

    #[tokio::test]
    async fn a_fragmented_stream_retries_a_429_once_and_honours_retry_after() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();

        tokio::spawn(async move {
            for _ in 0..2 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = vec![0u8; 8192];
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                log.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buffer[..read]).to_string());

                if log.lock().unwrap().len() == 1 {
                    let _ = socket
                        .write_all(
                            b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await;
                } else {
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        STREAM.len()
                    );
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket.write_all(STREAM.as_bytes()).await;
                }
                let _ = socket.shutdown().await;
            }
        });

        let model = AnthropicModel::with_base_url("sk-ant-test", format!("http://{address}"));
        let cancel = CancellationToken::new();
        let mut text = String::new();
        let mut sink = |delta: Delta| {
            if let Delta::Text(chunk) = delta {
                text.push_str(&chunk);
            }
        };
        model
            .chat(
                request("claude-opus-5-5", vec![ChatMessage::user("hi")], Vec::new()),
                &cancel,
                &mut sink,
            )
            .await
            .unwrap();

        assert_eq!(text, "pong");
        assert_eq!(seen.lock().unwrap().len(), 2, "the 429 was retried once");
    }

    #[tokio::test]
    async fn an_error_event_ends_the_turn_with_the_providers_message() {
        let error = concat!(
            "event: error\n",
            "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\
             \"message\":\"Overloaded\"}}\n\n",
        );
        let (base, _) = fake_anthropic(error).await;
        let model = AnthropicModel::with_base_url("sk-ant-test", base);
        let cancel = CancellationToken::new();
        let mut sink = |_: Delta| {};
        let error = model
            .chat(
                request("claude-opus-5-5", vec![ChatMessage::user("hi")], Vec::new()),
                &cancel,
                &mut sink,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(&error, ModelError::Other(message) if message == "Overloaded"),
            "{error}"
        );
    }

    /// A model id another door owns must not reach this endpoint: the table replaces it with the
    /// provider's default before the body is built. This test pins the id that *is* sent.
    #[tokio::test]
    async fn a_batch_suffix_never_reaches_the_endpoint() {
        let (base, seen) = fake_anthropic(STREAM).await;
        let model = AnthropicModel::with_base_url("sk-ant-test", base);
        let cancel = CancellationToken::new();
        let mut sink = |_: Delta| {};
        model
            .chat(
                request(
                    "claude-opus-5-5:batch",
                    vec![ChatMessage::user("hi")],
                    Vec::new(),
                ),
                &cancel,
                &mut sink,
            )
            .await
            .unwrap();

        let request = seen.lock().unwrap().clone();
        assert!(
            request.contains("\"model\":\"claude-opus-5-5\""),
            "{request}"
        );
        assert!(!request.contains(":batch"), "{request}");
    }
}

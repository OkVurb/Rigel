//! OpenRouter chat completions, streamed (§7.5).
//!
//! Two rules shape the implementation:
//!
//! * **Never retry after the first token.** A retried stream that already emitted is a
//!   duplicated answer in the transcript, which is worse than an error.
//! * **`Retry-After` wins over jitter.** A provider that says when to come back knows more
//!   than an exponential curve does.

use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::model::{
    ChatRequest, Delta, DeltaSink, Model, ModelError, RawUsage, SseDecoder, jitter_delay,
};

pub const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";
pub const DEFAULT_MODEL: &str = "openai/gpt-4.1-mini";

pub struct OpenRouterModel {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    max_attempts: u32,
}

impl OpenRouterModel {
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

    /// For tests and self-hosted gateways: same wire contract, different origin.
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        let mut model = Self::new(api_key);
        model.base_url = base_url.into().trim_end_matches('/').to_string();
        model
    }

    pub fn with_attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = attempts.max(1);
        self
    }

    fn body(&self, request: &ChatRequest) -> Value {
        let mut body = json!({
            "model": request.model,
            "messages": request.messages,
            "stream": true,
            "temperature": request.temperature,
            "usage": { "include": true },
            "stream_options": { "include_usage": true },
        });
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(request.tools.iter().map(|spec| spec.openai()).collect());
            body["tool_choice"] = json!("auto");
        }
        if let Some(tokens) = request.max_tokens {
            body["max_tokens"] = json!(tokens);
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
                if payload.trim() == "[DONE]" {
                    return Ok(());
                }
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

                if let Some(usage) = value.get("usage").and_then(parse_usage) {
                    emitted = true;
                    on_delta(Delta::Usage(usage));
                }

                let Some(choice) = value
                    .get("choices")
                    .and_then(Value::as_array)
                    .and_then(|choices| choices.first())
                else {
                    continue;
                };
                let Some(delta) = choice.get("delta") else {
                    continue;
                };

                if let Some(text) = delta.get("content").and_then(Value::as_str) {
                    if !text.is_empty() {
                        emitted = true;
                        on_delta(Delta::Text(text.to_string()));
                    }
                }
                let reasoning = delta
                    .get("reasoning")
                    .or_else(|| delta.get("reasoning_content"))
                    .and_then(Value::as_str);
                if let Some(text) = reasoning {
                    if !text.is_empty() {
                        emitted = true;
                        on_delta(Delta::Reasoning(text.to_string()));
                    }
                }

                if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for (fallback_index, call) in calls.iter().enumerate() {
                        let index = call
                            .get("index")
                            .and_then(Value::as_u64)
                            .unwrap_or(fallback_index as u64)
                            as u32;
                        let id = call.get("id").and_then(Value::as_str);
                        let function = call.get("function");
                        let name = function.and_then(|f| f.get("name")).and_then(Value::as_str);
                        let arguments = function
                            .and_then(|f| f.get("arguments"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        emitted = true;
                        on_delta(Delta::ToolCall {
                            index,
                            id: id.map(str::to_string),
                            name: name.map(str::to_string),
                            arguments: arguments.to_string(),
                        });
                    }
                }

                if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                    on_delta(Delta::Finish(reason.to_string()));
                }
            }
        }

        // A stream that ends without a token and without [DONE] is a dead connection, not an
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
impl Model for OpenRouterModel {
    fn id(&self) -> String {
        "openrouter".to_string()
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
                .post(format!("{}/chat/completions", self.base_url))
                .bearer_auth(&self.api_key)
                .header("X-Title", "Rigel")
                .json(&body)
                .send();

            let response = tokio::select! {
                _ = cancel.cancelled() => return Err(ModelError::Cancelled),
                response = send => response,
            };

            match response {
                Ok(response) if response.status().is_success() => {
                    let result = self.stream_response(response, cancel, on_delta).await;
                    return result;
                }
                Ok(response) => {
                    let status = response.status().as_u16();
                    let retry_after = retry_after_header(&response);
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

fn parse_usage(value: &Value) -> Option<RawUsage> {
    if value.is_null() {
        return None;
    }
    let get = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
    let cached = value
        .pointer("/prompt_tokens_details/cached_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let reasoning = value
        .pointer("/completion_tokens_details/reasoning_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Some(RawUsage {
        input_tokens: get("prompt_tokens"),
        cached_input_tokens: cached,
        output_tokens: get("completion_tokens"),
        reasoning_tokens: reasoning,
        cost_usd: value.get("cost").and_then(Value::as_f64),
    })
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

    use crate::agent::model::{ChatMessage, ToolCallBuilder};

    /// A two-connection server: 429 with Retry-After on the first request, SSE on the second.
    async fn fake_provider() -> (String, Arc<Mutex<Vec<String>>>) {
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
                    let events = concat!(
                        "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"a.txt\\\"}\"}}]}}]}\n\n",
                        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":7,\"cost\":0.00025}}\n\n",
                        "data: [DONE]\n\n",
                    );
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        events.len()
                    );
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket.write_all(events.as_bytes()).await;
                }
                let _ = socket.shutdown().await;
            }
        });

        (format!("http://{address}"), seen)
    }

    #[tokio::test]
    async fn retries_a_429_and_streams_text_tool_calls_and_cost() {
        let (base, seen) = fake_provider().await;
        let model = OpenRouterModel::with_base_url("test-key", base);
        let request = ChatRequest {
            model: "openai/gpt-4.1-mini".to_string(),
            messages: vec![ChatMessage::user("hi")],
            tools: vec![],
            max_tokens: Some(64),
            temperature: 0.0,
        };

        let mut text = String::new();
        let mut builder = ToolCallBuilder::default();
        let mut usage = None;
        let cancel = CancellationToken::new();
        let mut sink = |delta: Delta| match delta {
            Delta::Text(chunk) => text.push_str(&chunk),
            Delta::ToolCall {
                index,
                id,
                name,
                arguments,
            } => builder.apply(index, id.as_deref(), name.as_deref(), &arguments),
            Delta::Usage(raw) => usage = Some(raw),
            _ => {}
        };

        model.chat(request, &cancel, &mut sink).await.unwrap();
        assert_eq!(text, "Hello");
        let calls = builder.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments, "{\"path\":\"a.txt\"}");
        let usage = usage.expect("usage chunk");
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.cost_usd, Some(0.00025));
        assert_eq!(seen.lock().unwrap().len(), 2, "the 429 was retried once");
    }

    #[tokio::test]
    async fn a_cancelled_request_returns_cancelled_without_retrying() {
        let (base, seen) = fake_provider().await;
        let model = OpenRouterModel::with_base_url("test-key", base).with_attempts(1);
        let request = ChatRequest {
            model: "openai/gpt-4.1-mini".to_string(),
            messages: vec![ChatMessage::user("hi")],
            tools: vec![],
            max_tokens: None,
            temperature: 0.0,
        };
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut sink = |_: Delta| {};
        let error = model.chat(request, &cancel, &mut sink).await.unwrap_err();
        assert!(matches!(error, ModelError::Cancelled));
        assert!(seen.lock().unwrap().is_empty());
    }
}

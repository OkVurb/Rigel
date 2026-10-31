//! The model contract, and the parts of a streamed reply that are provider-independent.
//!
//! A reply arrives as deltas: text, reasoning, partial tool-call JSON, and finally usage.
//! Tool calls are the interesting part — OpenAI-shaped providers stream the *arguments* as a
//! string in pieces, keyed by index, so the caller must concatenate before parsing. That
//! assembly lives here rather than in each provider so a bug is fixed once.
//!
//! Retry lives in the provider, not in the caller: only the provider knows whether a stream
//! has already emitted a token, and a retried stream that had is a duplicated answer.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::domain::Usage;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON as the model emitted it. Parsed by the tool registry, which reports a bad
    /// argument object as a tool error the model can read and correct.
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            tool_call_id: None,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolSpec {
    pub fn openai(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: Option<u32>,
    pub temperature: f32,
    /// Sent as `reasoning_effort`, separate from the model id (D28).
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RawUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    /// `usage.cost` from the provider, in USD. OpenRouter reports this; most providers do not.
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
pub enum Delta {
    Text(String),
    Reasoning(String),
    ToolCall {
        index: u32,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
    Usage(RawUsage),
    Finish(String),
}

/// The stream callback. Synchronous on purpose: every consumer either appends to a buffer
/// or forwards to an event channel, and neither may block the network read.
pub type DeltaSink<'a> = dyn FnMut(Delta) + Send + 'a;

#[async_trait]
pub trait Model: Send + Sync {
    fn id(&self) -> String;

    /// Stream a reply, calling `on_delta` as pieces arrive.
    ///
    /// Returns only after the stream ends. On cancellation the implementation must stop
    /// reading and return [`ModelError::Cancelled`].
    async fn chat(
        &self,
        request: ChatRequest,
        cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("cancelled")]
    Cancelled,

    #[error("no API key for {0}; set it with `rigel keys set {0}`")]
    NoKey(String),

    #[error("http error: {0}")]
    Http(String),

    #[error("provider returned {status}: {body}")]
    Status {
        status: u16,
        body: String,
        retry_after: Option<u64>,
    },

    #[error("could not parse the stream: {0}")]
    Parse(String),

    #[error("the model script has no step left")]
    ScriptExhausted,

    #[error("{0}")]
    Other(String),
}

impl ModelError {
    pub fn is_retryable_status(&self) -> bool {
        matches!(self, Self::Status { status, .. } if *status == 429 || *status >= 500)
    }
}

/// Assembles streamed tool-call fragments. `arguments` is a JSON string split arbitrarily
/// across chunks; `id` and `name` usually arrive once, on the first fragment.
#[derive(Default)]
pub struct ToolCallBuilder {
    slots: BTreeMap<u32, PartialToolCall>,
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl ToolCallBuilder {
    pub fn apply(&mut self, index: u32, id: Option<&str>, name: Option<&str>, arguments: &str) {
        let slot = self.slots.entry(index).or_default();
        if let Some(id) = id {
            slot.id = id.to_string();
        }
        if let Some(name) = name {
            slot.name.push_str(name);
        }
        slot.arguments.push_str(arguments);
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn finish(self) -> Vec<ToolCall> {
        self.slots
            .into_values()
            .filter(|slot| !slot.name.is_empty())
            .map(|slot| ToolCall {
                id: if slot.id.is_empty() {
                    format!("call_{}", uuid::Uuid::new_v4())
                } else {
                    slot.id
                },
                name: slot.name,
                arguments: if slot.arguments.is_empty() {
                    "{}".to_string()
                } else {
                    slot.arguments
                },
            })
            .collect()
    }
}

/// Incremental `text/event-stream` decoder.
///
/// The spec allows CRLF and multi-line `data:` fields; both are handled. Events other than
/// `data` are ignored, and a comment (`: keep-alive`) is not an event.
#[derive(Default)]
pub struct SseDecoder {
    buffer: String,
}

impl SseDecoder {
    /// Feed bytes, get the data payloads of every complete event.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.push_str(&String::from_utf8_lossy(bytes));
        let mut events = Vec::new();

        while let Some((block, rest)) = split_event(&self.buffer) {
            self.buffer = rest;
            let mut data = Vec::new();
            for line in block.lines() {
                let line = line.strip_suffix('\r').unwrap_or(line);
                if let Some(value) = line.strip_prefix("data:") {
                    data.push(value.strip_prefix(' ').unwrap_or(value).to_string());
                }
            }
            if !data.is_empty() {
                events.push(data.join("\n"));
            }
        }
        events
    }
}

fn split_event(buffer: &str) -> Option<(String, String)> {
    let lf = buffer.find("\n\n").map(|index| (index, 2));
    let crlf = buffer.find("\r\n\r\n").map(|index| (index, 4));
    let (index, skip) = match (lf, crlf) {
        (Some(a), Some(b)) => {
            if a.0 <= b.0 {
                a
            } else {
                b
            }
        }
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    };
    Some((
        buffer[..index].to_string(),
        buffer[index + skip..].to_string(),
    ))
}

/// Full jitter, without a random-number dependency: nanoseconds are not cryptographic, and
/// this only spreads retries apart.
pub fn jitter_delay(attempt: u32, base_ms: u64) -> Duration {
    let ceiling = base_ms.saturating_mul(1u64 << attempt.min(6)).min(8_000);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    Duration::from_millis(50 + nanos % (ceiling.max(1) + 1))
}

/// Convert a provider's USD figure to micro-dollars. Rounding happens once, here.
pub fn usd_to_micros(usd: f64) -> i64 {
    (usd * 1_000_000.0).round() as i64
}

/// The suffix a gateway uses to name an execution mode that is **not** `/chat/completions`.
const BATCH_SUFFIX: &str = ":batch";

/// The id a chat turn may put on the wire.
///
/// A provider's roster carries ids for other endpoints as well as the chat one. The id that
/// reached the refiner was `google/gemini-4-argon:batch`, and the answer was
/// `404 … cannot be used with the chat/completions endpoint (adapter VertexGeminiBatchAdapter)`:
/// the provider routed it to its batch adapter because the id said batch, not because anything
/// was wrong with the request.
///
/// So an id that names another execution mode is never sent. The chat twin of a batch id is the
/// same id without the suffix, which is the only substitution made here — `:free` and the other
/// routing variants are chat ids and pass through untouched.
pub fn chat_model_id(model: &str) -> &str {
    let trimmed = model.trim();
    let bytes = trimmed.as_bytes();
    let suffix = BATCH_SUFFIX.as_bytes();
    if bytes.len() > suffix.len()
        && bytes[bytes.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
    {
        return trimmed[..trimmed.len() - suffix.len()].trim_end();
    }
    trimmed
}

pub fn usage_from_raw(raw: RawUsage, model: &str) -> Usage {
    let provider_micros = raw.cost_usd.map(usd_to_micros);
    // Price the id that was actually asked for, so a stored `…:batch` is billed as the model
    // that served the turn.
    let estimate = crate::agent::pricing::estimate(chat_model_id(model), &raw);
    Usage {
        input_tokens: raw.input_tokens,
        cached_input_tokens: raw.cached_input_tokens,
        output_tokens: raw.output_tokens,
        reasoning_tokens: raw.reasoning_tokens,
        cost_micros: provider_micros.unwrap_or(estimate),
        provider_micros,
        source: if provider_micros.is_some() {
            crate::domain::CostSource::Provider
        } else {
            crate::domain::CostSource::Estimated
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_events_survive_arbitrary_chunk_boundaries() {
        let mut decoder = SseDecoder::default();
        let mut events = Vec::new();
        for chunk in ["data: {\"a\":", "1}\n", "\ndata: [DONE]\n\n"] {
            events.extend(decoder.feed(chunk.as_bytes()));
        }
        assert_eq!(events, vec!["{\"a\":1}".to_string(), "[DONE]".to_string()]);
    }

    #[test]
    fn crlf_and_keepalives_are_handled() {
        let mut decoder = SseDecoder::default();
        let events = decoder.feed(b": keep-alive\r\n\r\ndata: one\r\ndata: two\r\n\r\n");
        assert_eq!(events, vec!["one\ntwo".to_string()]);
    }

    #[test]
    fn split_tool_arguments_are_assembled_in_index_order() {
        let mut builder = ToolCallBuilder::default();
        builder.apply(0, Some("call_a"), Some("read_"), "{\"pa");
        builder.apply(0, None, Some("file"), "th\":\"a\"}");
        builder.apply(1, Some("call_b"), Some("budget"), "{}");
        let calls = builder.finish();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments, "{\"path\":\"a\"}");
        assert_eq!(calls[1].id, "call_b");
    }

    #[test]
    fn dollars_become_micro_dollars_once() {
        assert_eq!(usd_to_micros(0.0000042), 4);
        assert_eq!(usd_to_micros(0.25), 250_000);
        assert_eq!(usd_to_micros(1.0), 1_000_000);
    }

    #[test]
    fn a_batch_only_id_is_never_the_id_on_the_wire() {
        assert_eq!(
            chat_model_id("google/gemini-4-argon:batch"),
            "google/gemini-4-argon"
        );
        assert_eq!(chat_model_id("gemini-4-argon:BATCH"), "gemini-4-argon");
        assert_eq!(
            chat_model_id(" google/gemini-4-argon:batch "),
            "google/gemini-4-argon"
        );
        // Dropping the suffix must not eat a chat id that merely ends in those letters.
        assert_eq!(chat_model_id("vendor/prebatch"), "vendor/prebatch");
        // The other routing variants a roster uses are chat ids.
        assert_eq!(
            chat_model_id("baseten/mammoth-1:free"),
            "baseten/mammoth-1:free"
        );
        assert_eq!(chat_model_id("openai/gpt-4.1-mini"), "openai/gpt-4.1-mini");
        assert_eq!(chat_model_id(":batch"), ":batch");
    }
}

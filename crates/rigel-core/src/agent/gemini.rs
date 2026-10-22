//! Google Gemini through its OpenAI-compatible endpoint.
//!
//! Gemini's compatibility layer accepts the same `/chat/completions` request shape OpenRouter
//! does, so the streaming decoder, retry policy and tool-call assembly are shared; only three
//! things differ:
//!
//! * the base URL and the bearer key (`GEMINI_API_KEY`, from AI Studio);
//! * the model id namespace — `gemini-4-argon`, not `openai/…`;
//! * the request profile. OpenRouter's `usage: { include: true }` is not part of the Gemini
//!   compatibility surface, so it is left out rather than sent as an unverified field.
//!
//! This is the API-key path. Vertex AI is a different authentication (a GCP project plus an
//! OAuth token), and is deliberately not pretended to work by pointing this module at it.

use super::openrouter::OpenRouterModel;

pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai";

/// The flagship reasoning model. A turn can override it with `--model`, or `Settings.model`.
pub const DEFAULT_MODEL: &str = "gemini-4-argon";

/// The API base, trailing slash trimmed. `RIGEL_GEMINI_BASE_URL` exists for tests and for a
/// gateway that speaks the same contract; it does not change the key in use.
pub fn base_url() -> String {
    std::env::var("RIGEL_GEMINI_BASE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

/// A model pointed at Gemini. The key is the Gemini API key, not a project credential.
pub fn model(api_key: impl Into<String>) -> OpenRouterModel {
    OpenRouterModel::with_base_url(api_key, base_url())
        .with_provider("gemini")
        // Gemini does not document OpenRouter's cost-request field; omit it instead of
        // sending something the compatibility layer may reject.
        .without_usage_request()
}

/// Whether an id belongs to this provider. Settings carry one model across a provider switch,
/// so a stale OmniRoute id must not be sent to Gemini.
pub fn is_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("gemini")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use crate::agent::model::{ChatMessage, ChatRequest, Delta, Model};
    use tokio_util::sync::CancellationToken;

    /// One request, read fully, answered with the shapes Gemini's compatibility layer emits.
    async fn fake_gemini() -> (String, Arc<Mutex<String>>) {
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
                    let length = content_length(&head).unwrap_or(0);
                    if request.len() >= end + length {
                        break;
                    }
                }
            }
            *log.lock().unwrap() = String::from_utf8_lossy(&request).to_string();

            let events = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"po\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"ng\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_9\",\
                 \"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"a.txt\\\"}\"}}]}}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":2,\
                 \"prompt_tokens_details\":{\"cached_tokens\":3}}}\n\n",
                "data: [DONE]\n\n",
            );
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                events.len()
            );
            let _ = socket.write_all(header.as_bytes()).await;
            let _ = socket.write_all(events.as_bytes()).await;
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

    #[tokio::test]
    async fn a_turn_streams_text_and_tool_calls_from_the_gemini_profile() {
        let (base, seen) = fake_gemini().await;
        let model = OpenRouterModel::with_base_url("g-key", base)
            .with_provider("gemini")
            .without_usage_request();
        assert_eq!(model.id(), "gemini");

        let request = ChatRequest {
            model: DEFAULT_MODEL.to_string(),
            messages: vec![ChatMessage::user("say pong")],
            tools: vec![],
            max_tokens: Some(64),
            temperature: 0.0,
            // Gemini has no reasoning_effort field; the profile ignores it.
            effort: None,
        };
        let cancel = CancellationToken::new();
        let mut text = String::new();
        let mut calls = Vec::new();
        let mut usage = None;
        let mut sink = |delta: Delta| match delta {
            Delta::Text(chunk) => text.push_str(&chunk),
            Delta::ToolCall {
                index,
                id,
                name,
                arguments,
            } => calls.push((index, id, name, arguments)),
            Delta::Usage(raw) => usage = Some(raw),
            _ => {}
        };
        model.chat(request, &cancel, &mut sink).await.unwrap();

        assert_eq!(text, "pong");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1.as_deref(), Some("call_9"));
        assert_eq!(calls[0].2.as_deref(), Some("read_file"));

        let usage = usage.expect("usage chunk");
        assert_eq!(usage.input_tokens, 9);
        assert_eq!(usage.cached_input_tokens, 3);
        assert_eq!(usage.output_tokens, 2);

        let request = seen.lock().unwrap().clone();
        assert!(
            request.starts_with("POST /chat/completions HTTP/1.1"),
            "{request}"
        );
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer g-key"),
            "{request}"
        );
        assert!(
            request.contains("\"model\":\"gemini-4-argon\""),
            "{request}"
        );
        // The OpenRouter cost field is not sent to Gemini.
        assert!(!request.contains("\"usage\""), "{request}");
    }

    #[test]
    fn only_gemini_ids_are_treated_as_this_providers_models() {
        assert!(is_model("gemini-4-argon"));
        assert!(is_model(" gemini-3.8-flash"));
        assert!(!is_model("gpt-5.6-sol-medium"));
        assert!(!is_model("openai/gpt-4.1-mini"));
    }
}

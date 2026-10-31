//! Gemini on Vertex AI: the same wire shape as the other adapters, a different door.
//!
//! Vertex is not AI Studio with another URL. Three things differ, and each of them is a way to
//! get a 404 if it is guessed:
//!
//! * the base carries the **project and the location**, and a non-global location is served from
//!   its own host (`us-central1-aiplatform.googleapis.com`), not from the global one;
//! * the credential is an **OAuth access token**, not an API key, and it expires — an hour, by
//!   default;
//! * the model ids carry the **publisher prefix**, `google/gemini-4-argon`, which is the very
//!   namespace the refiner once sent to the wrong adapter.
//!
//! Everything a chat turn needs beyond that is shared with the other adapters: the request body,
//! the streaming decoder, the retry policy and the tool-call assembly all live in
//! [`super::openrouter`], so a bug fixed there is fixed here too.
//!
//! Tokens are never refreshed in the background. Rigel holds no Google SDK, so a token comes
//! from the stored `rigel/vertex` entry (`VERTEX_ACCESS_TOKEN`), or from
//! `gcloud auth print-access-token` when the Google Cloud CLI is installed; a turn that fails
//! says which of the two to use.

use std::process::Stdio;
use std::time::Duration;

use crate::credentials::{self, CredentialStore};

use super::openrouter::OpenRouterModel;

/// The location the OpenAI-compatible route is served from by default. `global` is what Google's
/// own example uses; a regional location is set with `VERTEX_LOCATION`.
pub const DEFAULT_LOCATION: &str = "global";

/// The flagship reasoning model, in the publisher-prefixed form Vertex requires.
pub const DEFAULT_MODEL: &str = "google/gemini-4-argon";

const TOKEN_COMMAND: [&str; 2] = ["auth", "print-access-token"];
const TOKEN_TIMEOUT: Duration = Duration::from_secs(20);

/// The project the endpoint is billed to. The same three variables the Google tools read, with
/// Rigel's own name first.
pub fn project() -> Option<String> {
    [
        "RIGEL_VERTEX_PROJECT",
        "VERTEX_PROJECT",
        "GOOGLE_CLOUD_PROJECT",
    ]
    .iter()
    .find_map(|name| non_empty(name))
}

/// `global`, or `us-central1` and friends.
pub fn location() -> String {
    ["RIGEL_VERTEX_LOCATION", "VERTEX_LOCATION"]
        .iter()
        .find_map(|name| non_empty(name))
        .unwrap_or_else(|| DEFAULT_LOCATION.to_string())
}

/// The API base, trailing slash trimmed. `RIGEL_VERTEX_BASE_URL` skips the project lookup
/// entirely, which is what a test and a gateway that fronts Vertex both need.
pub fn base_url() -> Result<String, String> {
    if let Some(override_url) = non_empty("RIGEL_VERTEX_BASE_URL") {
        return Ok(override_url);
    }
    let project = project().ok_or_else(|| {
        "no Vertex project: set VERTEX_PROJECT (or GOOGLE_CLOUD_PROJECT) to the project the \
         endpoint is billed to — `gcloud config get-value project` prints one"
            .to_string()
    })?;
    Ok(endpoint_for(&project, &location()))
}

/// The documented shape of the OpenAI-compatible route. Only a non-global location gets the
/// regional host prefix, which is how Vertex serves it.
fn endpoint_for(project: &str, location: &str) -> String {
    let host = if location == DEFAULT_LOCATION {
        "aiplatform.googleapis.com".to_string()
    } else {
        format!("{location}-aiplatform.googleapis.com")
    };
    format!("https://{host}/v1/projects/{project}/locations/{location}/endpoints/openapi")
}

/// A model pointed at Vertex. `token` is an OAuth access token.
pub fn model(token: impl Into<String>) -> Result<OpenRouterModel, String> {
    let base = base_url()?;
    Ok(model_at(token, base))
}

/// The same profile at an explicit base: tests, and a gateway that speaks the same contract.
pub fn model_at(token: impl Into<String>, base: impl Into<String>) -> OpenRouterModel {
    OpenRouterModel::with_base_url(token, base)
        .with_provider("vertex")
        // Vertex does not document OpenRouter's cost-request field; omit it rather than send
        // something the endpoint may reject.
        .without_usage_request()
}

/// A token, from the stored credential or from `gcloud`. The error names both ways to get one.
pub async fn resolve_token(store: &dyn CredentialStore) -> Result<String, String> {
    if let Ok(secret) = credentials::get_from(store, credentials::VERTEX) {
        return Ok(secret.expose().to_string());
    }
    gcloud_token().await.map_err(|error| {
        format!(
            "no Vertex access token: set VERTEX_ACCESS_TOKEN, or store one for `vertex` in \
             Settings, or let Rigel read one from the Google Cloud CLI ({error})"
        )
    })
}

/// `gcloud auth print-access-token`, with a timeout so a CLI that hangs on a login prompt cannot
/// hang a turn. Windows installs these as `.cmd` shims, which `Command::new` cannot execute.
async fn gcloud_token() -> Result<String, String> {
    let mut command = gcloud_command();
    command
        .args(TOKEN_COMMAND)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = tokio::time::timeout(TOKEN_TIMEOUT, command.output())
        .await
        .map_err(|_| "`gcloud auth print-access-token` did not answer".to_string())?
        .map_err(|error| format!("`gcloud auth print-access-token` could not run: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.lines().last().unwrap_or("no output").trim();
        return Err(format!("gcloud exited with {}: {detail}", output.status));
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return Err("gcloud printed an empty token; run `gcloud auth login`".to_string());
    }
    Ok(token)
}

fn gcloud_command() -> tokio::process::Command {
    #[cfg(windows)]
    {
        let mut command = tokio::process::Command::new("cmd");
        command.args(["/C", "gcloud"]);
        crate::process::hide_tokio(&mut command);
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = tokio::process::Command::new("gcloud");
        crate::process::hide_tokio(&mut command);
        command
    }
}

fn non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_util::sync::CancellationToken;

    use crate::agent::model::{ChatMessage, ChatRequest, Delta, Model};
    use crate::credentials::MemoryCredentialStore;

    /// One request, read fully, answered with the shapes an OpenAI-compatible endpoint emits.
    async fn fake_vertex() -> (String, Arc<Mutex<String>>) {
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

            let events = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"po\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"ng\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_7\",\
                 \"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"b.txt\\\"}\"}}]}}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":11,\"completion_tokens\":3}}\n\n",
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

    #[test]
    fn the_route_carries_the_project_the_location_and_the_regional_host() {
        assert_eq!(
            endpoint_for("rigel-prod", "us-central1"),
            "https://us-central1-aiplatform.googleapis.com/v1/projects/rigel-prod/locations/\
             us-central1/endpoints/openapi"
        );
        assert_eq!(
            endpoint_for("rigel-prod", DEFAULT_LOCATION),
            "https://aiplatform.googleapis.com/v1/projects/rigel-prod/locations/global/\
             endpoints/openapi"
        );
    }

    #[tokio::test]
    async fn a_stored_token_is_used_without_asking_gcloud() {
        let store = MemoryCredentialStore::with(credentials::VERTEX, "ya29.stored");
        let token = resolve_token(&store).await.unwrap();
        assert_eq!(token, "ya29.stored");
    }

    #[tokio::test]
    async fn a_turn_streams_text_and_tool_calls_from_the_vertex_profile() {
        let (base, seen) = fake_vertex().await;
        let model = model_at("ya29.test", base);
        assert_eq!(model.id(), "vertex");

        let request = ChatRequest {
            model: DEFAULT_MODEL.to_string(),
            messages: vec![ChatMessage::user("say pong")],
            tools: vec![],
            max_tokens: Some(64),
            temperature: 0.0,
            effort: None,
        };
        let cancel = CancellationToken::new();
        let mut text = String::new();
        let mut calls = Vec::new();
        let mut sink = |delta: Delta| match delta {
            Delta::Text(chunk) => text.push_str(&chunk),
            Delta::ToolCall {
                index,
                id,
                name,
                arguments,
            } => calls.push((index, id, name, arguments)),
            _ => {}
        };
        model.chat(request, &cancel, &mut sink).await.unwrap();

        assert_eq!(text, "pong");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].2.as_deref(), Some("read_file"));

        let request = seen.lock().unwrap().clone();
        assert!(
            request.starts_with("POST /chat/completions HTTP/1.1"),
            "{request}"
        );
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer ya29.test"),
            "{request}"
        );
        assert!(
            request.contains("\"model\":\"google/gemini-4-argon\""),
            "{request}"
        );
        // The OpenRouter cost field is not part of Vertex's compatibility surface.
        assert!(!request.contains("\"usage\""), "{request}");
    }
}

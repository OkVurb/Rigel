//! The window's client for the API server's Concord inbox.
//!
//! The API owns the one long-lived Concord session; opening a second one from the window would
//! create a second participant and two pollers. So the window reads the API's cache and asks the
//! API to send replies, using the token and address the server wrote to `api.json`.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

const INBOX_TIMEOUT: Duration = Duration::from_secs(20);
/// A reply can wait through one `npx` cold start and Concord's two-minute tool deadline.
const REPLY_TIMEOUT: Duration = Duration::from_secs(140);

fn config_path() -> PathBuf {
    std::env::var_os("RIGEL_API_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::index_dir().join("api.json"))
}

/// The token stays in this process: it is read from the server's config file, sent as a bearer
/// header, and never returned to the renderer.
fn connection() -> Result<(String, String), String> {
    let mut base = std::env::var("RIGEL_API_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty());
    let mut token = std::env::var("RIGEL_API_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if base.is_none() || token.is_none() {
        let path = config_path();
        let text = std::fs::read_to_string(&path).map_err(|error| {
            format!(
                "Could not read {}: {error}. Start the Rigel API once so it can write its address.",
                path.display()
            )
        })?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|error| format!("{} is not valid JSON: {error}", path.display()))?;
        if base.is_none() {
            let host = value
                .get("host")
                .and_then(Value::as_str)
                .unwrap_or("127.0.0.1")
                .trim();
            let host = if matches!(host, "0.0.0.0" | "::" | "[::]") {
                "127.0.0.1"
            } else {
                host
            };
            let port = value.get("port").and_then(Value::as_u64).unwrap_or(20140);
            let authority = if host.contains(':') && !host.starts_with('[') {
                format!("[{host}]")
            } else {
                host.to_string()
            };
            base = Some(format!("http://{authority}:{port}"));
        }
        if token.is_none() {
            token = value
                .get("token")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
        }
    }

    let base = base.ok_or_else(|| "The Rigel API address is not configured.".to_string())?;
    let token = token.ok_or_else(|| "The Rigel API token is not configured.".to_string())?;
    Ok((base, token))
}

async fn request(
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
    timeout: Duration,
) -> Result<Value, String> {
    let (base, token) = connection()?;
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|error| format!("Could not build the API client: {error}"))?;
    let mut request = client
        .request(method, format!("{base}{path}"))
        .bearer_auth(token);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.map_err(|error| {
        format!(
            "The Rigel API is not reachable at {base}: {error}. Start it with the Rigel API shortcut."
        )
    })?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("The Rigel API returned no body: {error}"))?;
    let value: Value =
        serde_json::from_str(&text).unwrap_or_else(|_| json!({ "error": { "message": text } }));
    if !status.is_success() {
        let detail = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| value.pointer("/error").and_then(Value::as_str))
            .or_else(|| value.get("message").and_then(Value::as_str))
            .unwrap_or("the Rigel API refused the request");
        return Err(format!("{detail} ({status})"));
    }
    Ok(value)
}

/// The cached inbox plus every receipt Concord recorded. The cache is the API poller's, so this
/// call does not start Concord or mark anything read.
#[tauri::command]
pub async fn concord_inbox() -> Result<Value, String> {
    request(
        reqwest::Method::GET,
        "/v1/concord/inbox?since=0&limit=200",
        None,
        INBOX_TIMEOUT,
    )
    .await
}

/// Reply as `muse`. The API builds `update_work` from the message id; the window never writes a
/// tool call or chooses an idempotency key.
#[tauri::command(rename_all = "snake_case")]
pub async fn concord_reply(message_id: String, content: String) -> Result<Value, String> {
    if content.trim().is_empty() {
        return Err("Type a reply first.".to_string());
    }
    request(
        reqwest::Method::POST,
        "/v1/concord/reply",
        Some(json!({ "message_id": message_id, "content": content })),
        REPLY_TIMEOUT,
    )
    .await
}

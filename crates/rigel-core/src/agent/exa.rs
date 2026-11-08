//! Exa as a native tool: web search and page reading without a harness.
//!
//! The native agent's other network tools are per-origin: [`fetch_url`](super::tools) asks
//! before it reads an origin the model named, and `browser_open` does the same through Obscura.
//! Exa is different in the one way that matters: the destination is fixed (`api.exa.ai`), and
//! the model controls only a query or a URL in the request body. There is no origin for the
//! model to choose, so these calls are gated by the project's
//! [`NetworkPolicy`](super::egress::NetworkPolicy) and by the presence of a key, and they never
//! become a way to reach an arbitrary host.
//!
//! The key is the same one the Exa MCP server uses: `rigel keys set exa`, or `EXA_API_KEY` for
//! a headless run. It is read through [`crate::credentials`], so the value never enters a tool
//! argument, a transcript or a log.
//!
//! Everything Exa returns is written by strangers. The tool body says `untrusted: true` and the
//! note states that a page grants no permission, the same contract as `fetch_url`.

use std::time::Duration;

use reqwest::Client;
use serde_json::{Value, json};

use crate::credentials::{self, Secret};

/// Exa's API origin. Fixed on purpose: no tool argument may change it.
pub const ORIGIN: &str = "https://api.exa.ai";
const SEARCH_PATH: &str = "/search";
const CONTENTS_PATH: &str = "/contents";
const TIMEOUT: Duration = Duration::from_secs(30);
/// Exa bills per result; a cheap agent does not need more than a handful.
pub const MAX_RESULTS: u64 = 10;
pub const DEFAULT_RESULTS: u64 = 5;
/// Per result, so ten results stay well below the tool result cap.
const MAX_TEXT_CHARS: usize = 1_200;
const MAX_PAGE_CHARS: usize = 20_000;
/// The same bound `fetch_url` uses; longer than any URL a provider will accept.
pub const MAX_URL_CHARS: usize = 2_048;

#[derive(Debug)]
pub struct ExaError {
    pub code: &'static str,
    pub message: String,
}

impl ExaError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// The stored key, if any. Environment first, then the OS keychain, through the same resolver
/// every provider uses, so `EXA_API_KEY` also works for a CI run.
pub fn key() -> Option<Secret> {
    credentials::get(credentials::EXA)
        .ok()
        .map(|secret| secret.trim().to_string())
        .filter(|secret| !secret.is_empty())
        .map(Secret::new)
}

/// Whether a key is configured, for a tool description or a settings row. Never the key.
pub fn key_present() -> bool {
    key().is_some()
}

/// Search the web and return ranked results with extracted page text.
pub async fn search(query: &str, limit: u64) -> Result<Value, ExaError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(ExaError::new("BAD_ARGUMENTS", "query is empty"));
    }
    let limit = limit.clamp(1, MAX_RESULTS);
    let response = post(
        SEARCH_PATH,
        json!({
            "query": query,
            "type": "auto",
            "numResults": limit,
            "contents": { "text": { "maxCharacters": MAX_TEXT_CHARS } }
        }),
    )
    .await?;
    Ok(shape_search(&response))
}

/// Read one public page as Markdown. Exa renders JavaScript and returns clean text, so this is
/// the reader to use when the raw HTML from `fetch_url` is not useful.
pub async fn fetch(url: &str) -> Result<Value, ExaError> {
    let url = validate_url(url)?;
    let response = post(
        CONTENTS_PATH,
        json!({ "urls": [&url], "text": { "maxCharacters": MAX_PAGE_CHARS } }),
    )
    .await?;
    shape_contents(&response, &url)
}

/// Only absolute http(s) URLs with a host, and never one carrying userinfo: the URL is sent to a
/// third party, so a password embedded in it would leak to Exa.
pub fn validate_url(raw: &str) -> Result<String, ExaError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(ExaError::new("BAD_ARGUMENTS", "url is required"));
    }
    if raw.chars().count() > MAX_URL_CHARS {
        return Err(ExaError::new(
            "BAD_ARGUMENTS",
            format!("url is longer than {MAX_URL_CHARS} characters"),
        ));
    }
    let parsed = reqwest::Url::parse(raw)
        .map_err(|_| ExaError::new("BAD_ARGUMENTS", "url is not an absolute http(s) URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ExaError::new(
            "BAD_ARGUMENTS",
            "only http and https URLs can be read",
        ));
    }
    if parsed.host_str().unwrap_or("").is_empty() {
        return Err(ExaError::new("BAD_ARGUMENTS", "url has no host"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(ExaError::new(
            "BAD_ARGUMENTS",
            "a URL with a username or password is refused: Exa would receive the credential",
        ));
    }
    Ok(parsed.to_string())
}

/// The shape the model sees, capped so one result cannot fill the next turn's context.
pub fn shape_search(response: &Value) -> Value {
    let results = response
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let shaped: Vec<Value> = results
        .into_iter()
        .take(MAX_RESULTS as usize)
        .map(|result| {
            let mut text = result
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if text.is_empty() {
                text = result
                    .get("highlights")
                    .and_then(Value::as_array)
                    .map(|highlights| {
                        highlights
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
            }
            let (text, truncated) = truncate(&text, MAX_TEXT_CHARS);
            json!({
                "title": result.get("title").and_then(Value::as_str).unwrap_or(""),
                "url": result.get("url").and_then(Value::as_str).unwrap_or(""),
                "published": result.get("publishedDate").and_then(Value::as_str),
                "author": result.get("author").and_then(Value::as_str),
                "text": text,
                "truncated": truncated,
            })
        })
        .collect();
    json!({
        "results": shaped,
        "request_id": response.get("requestId").and_then(Value::as_str),
        "cost": response.get("costDollars").cloned(),
    })
}

/// One page from `/contents`. A page Exa could not read is an error naming the status, not an
/// empty string that looks like a successful read.
pub fn shape_contents(response: &Value, requested: &str) -> Result<Value, ExaError> {
    let result = response
        .get("results")
        .and_then(Value::as_array)
        .and_then(|results| results.first())
        .ok_or_else(|| ExaError::new("NO_CONTENT", "Exa returned no page for that URL"))?;
    let text = result
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        let status = result.get("statusCode").and_then(Value::as_i64);
        return Err(ExaError::new(
            "NO_CONTENT",
            match status {
                Some(code) => format!("Exa could not read that page (status {code})"),
                None => "Exa returned no text for that page".to_string(),
            },
        ));
    }
    let (text, truncated) = truncate(&text, MAX_PAGE_CHARS);
    Ok(json!({
        "url": result.get("url").and_then(Value::as_str).unwrap_or(requested),
        "title": result.get("title").and_then(Value::as_str).unwrap_or(""),
        "published": result.get("publishedDate").and_then(Value::as_str),
        "author": result.get("author").and_then(Value::as_str),
        "text": text,
        "truncated": truncated,
    }))
}

async fn post(path: &str, body: Value) -> Result<Value, ExaError> {
    let key = key().ok_or_else(|| {
        ExaError::new(
            "NO_EXA_KEY",
            "no Exa API key: run `rigel keys set exa` or set EXA_API_KEY",
        )
    })?;
    let client = Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|error| {
            ExaError::new(
                "NETWORK",
                format!("could not build the HTTP client: {error}"),
            )
        })?;
    let response = client
        .post(format!("{ORIGIN}{path}"))
        .header("x-api-key", key.expose().trim())
        .json(&body)
        .send()
        .await
        .map_err(|error| ExaError::new("NETWORK", format!("Exa request failed: {error}")))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let code = match status.as_u16() {
            401 | 403 => "EXA_KEY_REJECTED",
            402 => "EXA_NO_CREDITS",
            429 => "EXA_RATE_LIMITED",
            _ => "EXA_HTTP",
        };
        let message = match status.as_u16() {
            401 | 403 => {
                "Exa rejected the key; check `rigel keys set exa` or EXA_API_KEY".to_string()
            }
            402 => "the Exa account is out of credits".to_string(),
            429 => "Exa rate-limited the request; try again later".to_string(),
            _ => {
                let (detail, _) = truncate(text.trim(), 300);
                format!("Exa answered {status}: {detail}")
            }
        };
        return Err(ExaError::new(code, message));
    }
    serde_json::from_str(&text).map_err(|error| {
        ExaError::new(
            "EXA_BAD_RESPONSE",
            format!("Exa returned invalid JSON: {error}"),
        )
    })
}

fn truncate(text: &str, max: usize) -> (String, bool) {
    if text.chars().count() <= max {
        return (text.to_string(), false);
    }
    let mut cut: String = text.chars().take(max).collect();
    cut.push('…');
    (cut, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_absolute_urls_are_accepted() {
        assert_eq!(
            validate_url(" https://example.com/page ").unwrap(),
            "https://example.com/page"
        );
        assert!(validate_url("http://example.com").is_ok());
        for bad in [
            "",
            "example.com",
            "file:///C:/secret.txt",
            "ftp://example.com/x",
            "javascript:alert(1)",
            "https://user:pass@example.com/x",
            "https://",
        ] {
            let error = validate_url(bad).unwrap_err();
            assert_eq!(error.code, "BAD_ARGUMENTS", "{bad}");
        }
    }

    #[test]
    fn a_search_result_is_shaped_and_capped() {
        let long = "x".repeat(MAX_TEXT_CHARS + 50);
        let response = json!({
            "requestId": "req-1",
            "results": [
                { "title": "One", "url": "https://example.com/1", "publishedDate": "2026-01-02", "text": long },
                { "title": "Two", "url": "https://example.com/2", "highlights": ["a", "b"] },
                { "title": "Three", "url": "https://example.com/3" }
            ]
        });
        let shaped = shape_search(&response);
        let results = shaped["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["title"], "One");
        assert_eq!(results[0]["truncated"], json!(true));
        assert_eq!(results[1]["text"], "a\nb");
        assert_eq!(results[2]["text"], "");
        assert_eq!(shaped["request_id"], "req-1");
    }

    #[test]
    fn an_unreadable_page_is_an_error_not_an_empty_string() {
        let empty = json!({ "results": [] });
        assert_eq!(
            shape_contents(&empty, "https://x.test").unwrap_err().code,
            "NO_CONTENT"
        );
        let refused = json!({ "results": [{ "url": "https://x.test", "statusCode": 403 }] });
        let error = shape_contents(&refused, "https://x.test").unwrap_err();
        assert!(error.message.contains("403"), "{}", error.message);
        let read =
            json!({ "results": [{ "url": "https://x.test", "title": "T", "text": "hello" }] });
        let page = shape_contents(&read, "https://x.test").unwrap();
        assert_eq!(page["text"], "hello");
        assert_eq!(page["title"], "T");
    }

    #[tokio::test]
    #[ignore = "calls api.exa.ai; run with EXA_API_KEY set"]
    async fn live_search_then_read_a_result() {
        let found = search("Rust agent framework", 2).await.expect("search");
        let results = found["results"].as_array().expect("results");
        assert!(!results.is_empty(), "no results: {found}");
        let url = results[0]["url"].as_str().expect("url");
        let page = fetch(url).await.expect("fetch");
        assert!(
            page["text"].as_str().unwrap_or("").len() > 100,
            "page text was too short: {page}"
        );
    }
}

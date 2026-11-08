//! A user's own OpenAI-compatible door (roadmap 28).
//!
//! The configuration is one JSON record in the OS keychain, under the `custom` entry: base URL,
//! header pairs and the model ids its probe returned. Header values are secrets, so they never
//! enter `settings.toml` and never come back to the renderer — [`MaskedDoor`] carries names only.
//!
//! A door is only saved after [`probe`] has listed its models. A URL that would send a bearer
//! header in clear text to another machine is refused; plain HTTP is allowed only on loopback.

use serde::{Deserialize, Serialize};

use super::local;
use super::openrouter::OpenRouterModel;
use crate::credentials::{self, CredentialStore};

/// The provider id, which is also its keychain entry name.
pub const ID: &str = "custom";
/// A headless configuration: the same JSON record, already encoded.
pub const ENV: &str = "RIGEL_CUSTOM_DOOR";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderPair {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CustomDoor {
    #[serde(default)]
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub headers: Vec<HeaderPair>,
    #[serde(default)]
    pub models: Vec<String>,
}

/// What the renderer may see after a save: no header value ever leaves the keychain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskedDoor {
    pub name: String,
    pub base_url: String,
    pub header_names: Vec<String>,
    pub models: Vec<String>,
}

impl CustomDoor {
    /// Validate a complete door, including the model list a probe produced.
    pub fn validate(&self) -> Result<(), String> {
        validate_base(&self.base_url)?;
        validate_headers(&self.headers)?;
        if self.models.iter().all(|model| model.trim().is_empty()) {
            return Err("probe the door first: it has no models to send a turn to".to_string());
        }
        Ok(())
    }

    /// The keychain record. Validation runs first, so an invalid door is never written.
    pub fn encode(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|error| format!("could not encode the door: {error}"))
    }

    pub fn masked(&self) -> MaskedDoor {
        MaskedDoor {
            name: self.name.clone(),
            base_url: self.base_url.clone(),
            header_names: self.headers.iter().map(|pair| pair.name.clone()).collect(),
            models: self.models.clone(),
        }
    }

    /// The model a turn uses. The custom door is keyless in the bearer sense: its credential is
    /// whichever header the user configured, and those headers travel on every request.
    pub fn model(&self) -> OpenRouterModel {
        OpenRouterModel::with_base_url("", self.base_url.trim_end_matches('/'))
            .with_provider(ID)
            .with_headers(
                self.headers
                    .iter()
                    .map(|pair| (pair.name.clone(), pair.value.clone())),
            )
    }
}

fn validate_base(value: &str) -> Result<String, String> {
    let value = value.trim().trim_end_matches('/');
    let url =
        reqwest::Url::parse(value).map_err(|error| format!("`{value}` is not a URL: {error}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("the base URL must start with http:// or https://".to_string());
    }
    if url.host_str().is_none() {
        return Err("the base URL needs a host".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("put the credential in a header, not in the URL".to_string());
    }
    if url.scheme() == "http" && !local::is_loopback(value) {
        return Err(
            "use https for a door on another machine; plain http is loopback only".to_string(),
        );
    }
    Ok(value.to_string())
}

fn validate_headers(headers: &[HeaderPair]) -> Result<(), String> {
    for pair in headers {
        reqwest::header::HeaderName::from_bytes(pair.name.trim().as_bytes())
            .map_err(|_| format!("`{}` is not a header name", pair.name))?;
        reqwest::header::HeaderValue::from_str(&pair.value)
            .map_err(|_| format!("the `{}` header value is not valid", pair.name))?;
    }
    Ok(())
}

/// The probe: `GET {base}/models` with the configured headers, returning the ids it lists.
pub async fn probe(base_url: &str, headers: &[HeaderPair]) -> Result<Vec<String>, String> {
    let base = validate_base(base_url)?;
    validate_headers(headers)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|error| format!("could not build an HTTP client: {error}"))?;
    let mut request = client.get(format!("{base}/models"));
    for pair in headers {
        request = request.header(pair.name.trim(), pair.value.as_str());
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("the door did not answer: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("the door answered {}", response.status()));
    }
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("the door returned unreadable data: {error}"))?;
    let mut models: Vec<String> = value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("id").and_then(serde_json::Value::as_str))
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    models.dedup();
    if models.is_empty() {
        return Err("the door answered but listed no models".to_string());
    }
    Ok(models)
}

/// Load the saved door: the environment first (headless runs), then the keychain.
pub fn load_from(store: &dyn CredentialStore) -> Result<Option<CustomDoor>, String> {
    if let Ok(value) = std::env::var(ENV)
        && !value.trim().is_empty()
    {
        return serde_json::from_str(value.trim())
            .map(Some)
            .map_err(|error| format!("{ENV} is not a door record: {error}"));
    }
    match store.lookup(ID) {
        None => Ok(None),
        Some(secret) => serde_json::from_str(secret.expose())
            .map(Some)
            .map_err(|error| format!("the saved custom door is unreadable: {error}")),
    }
}

pub fn load() -> Result<Option<CustomDoor>, String> {
    load_from(&credentials::KeychainCredentialStore)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn door(base_url: &str) -> CustomDoor {
        CustomDoor {
            name: "gateway".to_string(),
            base_url: base_url.to_string(),
            headers: vec![HeaderPair {
                name: "x-gateway-token".to_string(),
                value: "secret".to_string(),
            }],
            models: vec!["model-a".to_string()],
        }
    }

    #[test]
    fn a_remote_plain_http_door_is_refused_and_userinfo_is_not_a_header() {
        assert!(door("http://gateway.example/v1").validate().is_err());
        assert!(door("http://127.0.0.1:8081/v1").validate().is_ok());
        assert!(
            door("https://user:pass@gateway.example/v1")
                .validate()
                .is_err()
        );
        assert!(door("ftp://gateway.example/v1").validate().is_err());
    }

    #[test]
    fn the_saved_record_is_masked_before_it_reaches_the_renderer() {
        let masked = door("https://gateway.example/v1").masked();
        let text = serde_json::to_string(&masked).unwrap();
        assert!(
            !text.contains("secret"),
            "a header value reached the renderer"
        );
        assert!(text.contains("x-gateway-token"));
        assert!(text.contains("model-a"));
    }

    #[tokio::test]
    async fn the_probe_lists_models_and_sends_the_configured_header() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(String::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = [0u8; 4096];
            let read = socket.read(&mut buffer).await.unwrap_or(0);
            *log.lock().unwrap() = String::from_utf8_lossy(&buffer[..read]).to_string();
            let body = r#"{"data":[{"id":"model-b"},{"id":"model-a"},{"id":"model-a"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });

        let base = format!("http://{address}/v1");
        let models = probe(&base, &door(&base).headers)
            .await
            .expect("the probe lists");
        assert_eq!(models, vec!["model-a", "model-b"]);
        let request = seen.lock().unwrap().clone();
        assert!(request.starts_with("GET /v1/models"), "{request}");
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-gateway-token: secret"),
            "{request}"
        );
    }

    #[tokio::test]
    async fn a_turn_to_the_saved_door_carries_the_header_and_the_model() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(String::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = [0u8; 8192];
            let read = socket.read(&mut buffer).await.unwrap_or(0);
            *log.lock().unwrap() = String::from_utf8_lossy(&buffer[..read]).to_string();
            let _ = socket
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
        });

        let base = format!("http://{address}/v1");
        let model = door(&base).model();
        let _ = model
            .request(&json!({ "model": "model-a" }))
            .send()
            .await
            .expect("the door answers");
        let request = seen.lock().unwrap().clone();
        assert!(
            request.starts_with("POST /v1/chat/completions"),
            "{request}"
        );
        assert!(
            request
                .to_ascii_lowercase()
                .contains("x-gateway-token: secret"),
            "{request}"
        );
        assert!(request.contains("model-a"), "{request}");
    }
}

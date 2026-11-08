//! Local model doors (roadmap 27).
//!
//! Ollama and llamafile both speak the OpenAI-compatible contract on the loopback interface and
//! both are keyless. They are rows in [`super::providers`] like every other door, so the picker,
//! the catalogue and a turn reach them without a second code path — only the credential differs:
//! there is none, and [`super::openrouter`] omits the header rather than sending an empty one.
//!
//! The defaults are each project's documented port. `RIGEL_OLLAMA_BASE_URL` and
//! `RIGEL_LLAMAFILE_BASE_URL` move a door, which is what a remote ollama box or a non-default
//! llamafile port needs.

/// The Ollama door's provider id, which is also its keychain name (it holds nothing).
pub const OLLAMA: &str = "local";
/// The llamafile door's provider id.
pub const LLAMAFILE: &str = "llamafile";
/// Ollama's documented default.
pub const OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434/v1";
/// llamafile's documented default.
pub const LLAMAFILE_BASE_URL: &str = "http://127.0.0.1:8080/v1";

/// Whether this provider id is one of the local doors.
pub fn is_local(id: &str) -> bool {
    matches!(id.trim().to_ascii_lowercase().as_str(), OLLAMA | LLAMAFILE)
}

/// Whether a base URL points at this machine. The check is on the host alone, so a path or a
/// port cannot make a remote origin look local.
pub fn is_loopback(url: &str) -> bool {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = authority
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(authority)
        .trim_start_matches('[')
        .trim_end_matches(']');
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn both_local_doors_default_to_the_loopback_address() {
        for base in [OLLAMA_BASE_URL, LLAMAFILE_BASE_URL] {
            assert!(is_loopback(base), "{base} is not this machine");
        }
        assert!(!is_loopback("https://api.openrouter.ai/api/v1"));
        assert!(!is_loopback("http://127.0.0.1.example.com/v1"));
    }

    #[tokio::test]
    async fn a_local_turn_sends_no_authorization_and_stays_on_loopback() {
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
            let _ = socket
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
        });

        let base = format!("http://{address}/v1");
        assert!(is_loopback(&base));
        let model = super::super::openrouter::OpenRouterModel::with_base_url("", &base);
        let response = model
            .request(&json!({ "model": "llama3.2" }))
            .send()
            .await
            .expect("the local door answers");
        assert_eq!(response.status(), 401);

        let request = seen.lock().unwrap().clone();
        assert!(
            request.starts_with("POST /v1/chat/completions"),
            "a turn uses the chat path: {request}"
        );
        assert!(
            !request.to_ascii_lowercase().contains("authorization:"),
            "a keyless local door must not send an Authorization header: {request}"
        );
    }
}

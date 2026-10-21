//! OmniRoute: the local router Rigel talks to instead of a hosted provider.
//!
//! OmniRoute exposes an OpenAI-compatible surface at `/v1` and a public health probe at
//! `/api/health`, so the wire contract is [`super::openrouter::OpenRouterModel`] pointed at
//! loopback. The one thing a hosted provider never needs is **starting it**: if the router is
//! not answering, Rigel launches `omniroute serve --daemon --no-open` and waits for health.
//!
//! `RIGEL_OMNIROUTE_URL` overrides the API base (include the `/v1`); the origin used for
//! health and the port passed to the CLI are derived from it.

use std::time::Duration;

use serde::Serialize;

use super::openrouter::OpenRouterModel;

/// API base for a stock OmniRoute install on this machine.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:20128/v1";

/// The model Rigel asks for by default: GPT-5.6 Sol with medium reasoning. OmniRoute encodes
/// the effort in the model id, so a turn gets medium thinking without a second setting.
pub const DEFAULT_MODEL: &str = "gpt-5.6-sol-medium";

/// How long `ensure_server` waits for a freshly launched router to answer health.
const STARTUP_ATTEMPTS: u32 = 120;
const STARTUP_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    pub running: bool,
    pub origin: String,
    pub base_url: String,
}

/// The API base, trailing slash trimmed.
pub fn base_url() -> String {
    std::env::var("RIGEL_OMNIROUTE_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

/// `http://127.0.0.1:20128` for the default base; the `/v1` suffix is not part of an origin.
pub fn origin() -> String {
    origin_of(&base_url())
}

fn origin_of(base_url: &str) -> String {
    base_url
        .trim_end_matches('/')
        .strip_suffix("/v1")
        .unwrap_or(base_url.trim_end_matches('/'))
        .to_string()
}

fn port_of(origin: &str) -> u16 {
    reqwest::Url::parse(origin)
        .ok()
        .and_then(|url| url.port_or_known_default())
        .unwrap_or(20128)
}

/// A model pointed at the local router. `api_key` is OmniRoute's own key, not a provider's.
pub fn model(api_key: impl Into<String>) -> OpenRouterModel {
    OpenRouterModel::with_base_url(api_key, base_url())
}

fn health_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_millis(1_500))
        .connect_timeout(Duration::from_millis(800))
        .build()
        .unwrap_or_default()
}

async fn healthy(client: &reqwest::Client, origin: &str) -> bool {
    client
        .get(format!("{origin}/api/health"))
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

/// Something is listening on the router's port, even if health does not answer. That is a
/// stuck or starting instance, and launching a second one would not help: they share state.
async fn port_open(origin: &str) -> bool {
    let parsed = match reqwest::Url::parse(origin) {
        Ok(parsed) => parsed,
        Err(_) => return false,
    };
    let host = parsed.host_str().unwrap_or("127.0.0.1").to_string();
    let port = port_of(origin);
    tokio::time::timeout(
        Duration::from_millis(600),
        tokio::net::TcpStream::connect((host.as_str(), port)),
    )
    .await
    .map(|result| result.is_ok())
    .unwrap_or(false)
}

pub async fn status() -> ServerStatus {
    let client = health_client();
    let origin = origin();
    ServerStatus {
        running: healthy(&client, &origin).await,
        origin,
        base_url: base_url(),
    }
}

/// Health first, launch second. Returns an error only when a router was missing and could not
/// be brought up, so a caller can show one actionable message.
pub async fn ensure_server() -> Result<ServerStatus, String> {
    let client = health_client();
    let origin = origin();
    let base = base_url();

    if healthy(&client, &origin).await {
        return Ok(ServerStatus {
            running: true,
            origin,
            base_url: base,
        });
    }

    if port_open(&origin).await {
        // Give an instance that is already binding time to finish booting before declaring it
        // broken; a cold OmniRoute can take several seconds to serve.
        for _ in 0..STARTUP_ATTEMPTS {
            if healthy(&client, &origin).await {
                return Ok(ServerStatus {
                    running: true,
                    origin,
                    base_url: base,
                });
            }
            tokio::time::sleep(STARTUP_INTERVAL).await;
        }
        return Err(format!(
            "OmniRoute is listening at {origin} but is not answering /api/health. \
             Restart it, or set RIGEL_OMNIROUTE_URL to the right base URL."
        ));
    }

    spawn_server(&origin)?;

    for _ in 0..STARTUP_ATTEMPTS {
        if healthy(&client, &origin).await {
            return Ok(ServerStatus {
                running: true,
                origin,
                base_url: base,
            });
        }
        tokio::time::sleep(STARTUP_INTERVAL).await;
    }

    Err(format!(
        "OmniRoute did not answer at {origin} after it was started. \
         Check `omniroute serve` in a terminal, or set RIGEL_OMNIROUTE_URL."
    ))
}

/// Launch the router as its own daemon and return once the process is spawned. Readiness is
/// the caller's job: `--daemon` returns before the server is listening.
fn spawn_server(origin: &str) -> Result<(), String> {
    let port = port_of(origin).to_string();
    let mut command = server_command(&port);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command.spawn().map(|_| ()).map_err(|error| {
        format!(
            "could not start OmniRoute ({error}). Install it with `npm install -g omniroute`, \
             or start it yourself with `omniroute serve`."
        )
    })
}

/// Windows installs the CLI as a `.cmd` shim, which `Command::new` cannot execute directly.
fn server_command(port: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("cmd");
        command.args([
            "/C",
            "omniroute",
            "serve",
            "--port",
            port,
            "--daemon",
            "--no-open",
        ]);
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = std::process::Command::new("omniroute");
        command.args(["serve", "--port", port, "--daemon", "--no-open"]);
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_origin_drops_the_v1_suffix_and_trailing_slash() {
        assert_eq!(
            origin_of("http://127.0.0.1:20128/v1"),
            "http://127.0.0.1:20128"
        );
        assert_eq!(
            origin_of("http://localhost:9000/v1/"),
            "http://localhost:9000"
        );
        assert_eq!(origin_of("http://localhost:9000"), "http://localhost:9000");
    }

    #[test]
    fn the_default_model_is_sol_at_medium_effort() {
        assert_eq!(DEFAULT_MODEL, "gpt-5.6-sol-medium");
    }
}

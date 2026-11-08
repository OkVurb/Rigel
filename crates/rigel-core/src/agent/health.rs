//! Item 15: one health probe per credentialed door, at launch.
//!
//! The picker already reads every roster; this is the same request reduced to the one fact a
//! person needs before choosing a door: is it answering, how long did it take, and how many
//! models came back. A 401 and a 429 are named rather than folded into "offline", because the
//! remedy is different. Probes run concurrently and each failure is its own row, so one dead
//! door can neither empty the list nor delay a turn on another.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::credentials::{self, CredentialStore};

use super::providers;

/// What the door said when asked once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoorState {
    /// A roster came back.
    Ready,
    /// The credential was refused; the remedy is a new key or token.
    Auth,
    /// The door is reachable but asked us to wait.
    RateLimited,
    /// No answer, or an answer this probe does not understand.
    Unreachable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoorHealth {
    pub provider: String,
    pub name: String,
    pub state: DoorState,
    /// How many models the roster carried, when it carried one.
    pub models: Option<usize>,
    /// Wall time for the probe itself.
    pub latency_ms: Option<u64>,
    /// The provider's own status or the transport error, when there is one.
    pub detail: Option<String>,
}

/// One door to probe. Owned so a test can point the same probe at a scripted server without
/// touching the machine-wide environment.
pub struct ProbeTarget {
    pub id: String,
    pub name: String,
    pub base: String,
    pub key: Option<String>,
    pub headers: &'static [(&'static str, &'static str)],
}

impl ProbeTarget {
    fn row(
        &self,
        state: DoorState,
        models: Option<usize>,
        latency_ms: Option<u64>,
        detail: Option<String>,
    ) -> DoorHealth {
        DoorHealth {
            provider: self.id.clone(),
            name: self.name.clone(),
            state,
            models,
            latency_ms,
            detail,
        }
    }
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// `GET {base}/models` through the same headers and credential the catalogue uses.
pub async fn probe(target: ProbeTarget) -> DoorHealth {
    let started = Instant::now();
    let client = match reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(client) => client,
        Err(error) => {
            return target.row(
                DoorState::Unreachable,
                None,
                None,
                Some(format!("could not build an HTTP client: {error}")),
            );
        }
    };
    let url = format!("{}/models", target.base.trim_end_matches('/'));
    let mut request = client.get(&url);
    for (name, value) in target.headers {
        request = request.header(*name, *value);
    }
    if let Some(key) = target.key.clone() {
        request = request.bearer_auth(key);
    }

    match request.send().await {
        Ok(response) => {
            let status = response.status();
            let latency = Some(started.elapsed().as_millis() as u64);
            if status.is_success() {
                let models = response
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .and_then(|value| {
                        value
                            .get("data")
                            .and_then(|data| data.as_array())
                            .map(Vec::len)
                    });
                target.row(DoorState::Ready, models, latency, None)
            } else if status.as_u16() == 401 || status.as_u16() == 403 {
                target.row(
                    DoorState::Auth,
                    None,
                    latency,
                    Some(format!("provider returned {status}")),
                )
            } else if status.as_u16() == 429 {
                target.row(
                    DoorState::RateLimited,
                    None,
                    latency,
                    Some(format!("provider returned {status}")),
                )
            } else {
                target.row(
                    DoorState::Unreachable,
                    None,
                    latency,
                    Some(format!("provider returned {status}")),
                )
            }
        }
        Err(error) => target.row(
            DoorState::Unreachable,
            None,
            Some(started.elapsed().as_millis() as u64),
            Some(error.to_string()),
        ),
    }
}

/// Probe every target at once, preserving the order given. Each row stands alone: a failure is
/// data, never an error that would take the others with it.
pub async fn probe_targets(targets: Vec<ProbeTarget>) -> Vec<DoorHealth> {
    futures::future::join_all(targets.into_iter().map(probe)).await
}

/// Probe every door that holds a credential. A door whose base cannot be resolved is reported
/// with the resolver's sentence instead of being dropped.
pub async fn probe_all(store: &dyn CredentialStore) -> Vec<DoorHealth> {
    let mut targets = Vec::new();
    let mut unresolved = Vec::new();
    let mut rows = Vec::new();
    for spec in providers::all() {
        if !providers::present(store, spec) {
            continue;
        }
        // The remote agent has no roster to probe: the credential proves the relay, and the
        // first turn is what proves the remote poller. Report it ready without inventing a call.
        if spec.id == credentials::RIGEL {
            rows.push(DoorHealth {
                provider: spec.id.to_string(),
                name: spec.name.to_string(),
                state: DoorState::Ready,
                models: Some(1),
                latency_ms: None,
                detail: Some("relay credential present".to_string()),
            });
            continue;
        }
        match providers::base_url(spec) {
            Ok(base) => {
                let key = providers::credential(spec, store).await.ok();
                targets.push(ProbeTarget {
                    id: spec.id.to_string(),
                    name: spec.name.to_string(),
                    base,
                    key,
                    headers: spec.headers,
                });
            }
            Err(detail) => unresolved.push(DoorHealth {
                provider: spec.id.to_string(),
                name: spec.name.to_string(),
                state: DoorState::Unreachable,
                models: None,
                latency_ms: None,
                detail: Some(detail),
            }),
        }
    }
    rows.extend(probe_targets(targets).await);
    rows.extend(unresolved);
    rows
}

/// The launch pass. Called once by the window at start-up; the picker asks for the same rows
/// and receives the cached answer instead of spending another round trip per door.
pub async fn launch(store: &dyn CredentialStore) -> Vec<DoorHealth> {
    static LAUNCH: OnceLock<Vec<DoorHealth>> = OnceLock::new();
    if let Some(rows) = LAUNCH.get() {
        return rows.clone();
    }
    let rows = probe_all(store).await;
    let _ = LAUNCH.set(rows.clone());
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// One request, answered once with a fixed status and body.
    async fn serve(status: u16, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = vec![0u8; 4096];
            let _ = socket.read(&mut buffer).await;
            let response = format!(
                "HTTP/1.1 {status} Probe\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        });
        format!("http://{address}")
    }

    fn target(id: &str, base: String) -> ProbeTarget {
        ProbeTarget {
            id: id.to_string(),
            name: id.to_string(),
            base,
            key: Some("sk-test".to_string()),
            headers: &[],
        }
    }

    #[tokio::test]
    async fn a_ready_door_reports_its_roster_size_and_latency() {
        let base = serve(200, "{\"data\":[{},{},{}]}").await;
        let row = probe(target("openrouter", base)).await;
        assert_eq!(row.state, DoorState::Ready);
        assert_eq!(row.models, Some(3));
        assert!(row.latency_ms.is_some(), "the probe records wall time");
        assert!(row.detail.is_none());
    }

    #[tokio::test]
    async fn a_refused_key_is_named_auth_not_offline() {
        let base = serve(401, "{\"error\":\"bad key\"}").await;
        let row = probe(target("openrouter", base)).await;
        assert_eq!(row.state, DoorState::Auth);
        assert!(
            row.detail.as_deref().unwrap_or("").contains("401"),
            "{:?}",
            row.detail
        );
    }

    #[tokio::test]
    async fn a_throttled_door_is_named_rate_limited() {
        let base = serve(429, "{}").await;
        let row = probe(target("gemini", base)).await;
        assert_eq!(row.state, DoorState::RateLimited);
    }

    #[tokio::test]
    async fn a_closed_port_is_unreachable() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let row = probe(target("openrouter", format!("http://{address}"))).await;
        assert_eq!(row.state, DoorState::Unreachable);
        assert!(row.detail.is_some(), "the transport error is kept");
    }

    #[tokio::test]
    async fn one_dead_door_does_not_remove_another() {
        let dead = serve(401, "{}").await;
        let live = serve(200, "{\"data\":[{}]}").await;
        let rows = probe_targets(vec![target("openrouter", dead), target("gemini", live)]).await;
        assert_eq!(rows.len(), 2, "every target gets its own row");
        assert_eq!(rows[0].state, DoorState::Auth);
        assert_eq!(rows[1].state, DoorState::Ready);
        assert_eq!(rows[1].models, Some(1));
    }
}

//! The model catalogue the picker shows, and where each row came from.
//!
//! A picker that only knows the provider in Settings is a picker that hides every model the user
//! has already paid for: a stored key is the permission to use that door, so **every** provider
//! that holds a credential is asked for its roster. The rules are small enough to state:
//!
//! * the current provider's roster is read first, and every configured or local provider is read
//!   concurrently so Free mode and the picker see the same complete live set;
//! * OpenRouter's list is public, so it is offered even with no key stored — that is how an
//!   install on another door sees what it could switch to;
//! * a provider that publishes no roster of its own contributes exactly one row, its default,
//!   labelled as such;
//! * a failure is reported per provider and does not empty the catalogue: one dead roster must
//!   not take the picker down with it.
//!
//! This lives in the core rather than in the window so the same list can be printed headlessly
//! (`rigel models`) and so the HTTP half can be tested against a scripted server.

use serde::{Deserialize, Serialize};

use crate::credentials::{self, CredentialStore};

use super::custom;
use super::local;
use super::providers::{self, Endpoint, ProviderSpec};
use super::retry::{self, RetryClass};

fn tools_unknown() -> bool {
    true
}

/// One row in the picker. The provider travels with the id so choosing a model also chooses
/// where the turn goes; the caller never has to know which key is configured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelOption {
    pub id: String,
    pub provider: String,
    /// The provider's human name for the model, when it publishes one.
    pub name: Option<String>,
    /// A zero-priced row — OpenRouter marks them, and they stay visible rather than filtered.
    pub free: bool,
    /// Whether this row can answer a chat turn. The catalogue **keeps** the rest — the picker
    /// hides them behind a toggle and `rigel models --all` prints them — because a filter that
    /// deletes data is a filter nobody can audit.
    pub chat: bool,
    /// Whether the roster advertises tool/function calls. Free mode needs this because the
    /// coordinator, subagents and the swarm all act through tools; a text-only row would be
    /// chosen and then fail the first turn. A roster that does not publish the field is treated
    /// as unknown-but-usable rather than silently excluded.
    #[serde(default = "tools_unknown")]
    pub tools: bool,
    /// A model served by a keyless door on this machine (roadmap 27), so the picker can say so.
    #[serde(default)]
    pub local: bool,
    /// The vendor's own `input->output` description, when it publishes one. Carried so the
    /// [`chat`](Self::chat) rule can be audited from `rigel models --json` instead of guessed at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modality: Option<String>,
    /// Completion price in micro-dollars per million tokens, when the roster publishes one.
    /// Integer micro-units keep `Eq` and make the picker's relative cost hint serializable
    /// without carrying floating-point formatting noise into the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_micros_per_mtok: Option<u64>,
}

/// Ids that name something other than a chat model.
///
/// Read from the live rosters on 27 Oct 2026, not guessed: the local router serves 174
/// `aihorde/…` Stable-Diffusion checkpoints and four `veo-free/` / `veoaifree-web/` video aliases
/// among its 627 rows, and OpenRouter publishes two `google/lyria-…` music models. Everything
/// else in those two rosters answers a chat request, including the name-matched candidates that
/// a looser rule would have thrown away: `google/gemini-3-pro-image` and `openai/gpt-5-image`
/// report `text+image->text+image`, which is a chat model that can also draw.
/// Tokens are matched on **segment boundaries** of the id, never as bare substrings: the first
/// live run of this rule hid `dva/inkling-high` because it contains `kling`. Each token is a
/// phrase as it appears between separators, so `sora` matches `sora-2`, `veo` matches `veo-3`,
/// and `kling` matches `kling-2.5` and nothing else.
const NOT_CHAT: &[&str] = &[
    // Image checkpoints and community image clusters.
    "aihorde",
    "stable-diffusion",
    "sdxl",
    "midjourney",
    "dall-e",
    "gpt-image",
    "flux",
    // Video.
    "veo",
    "sora",
    "kling",
    "pixverse",
    "seedance",
    "wan2",
    "happyhorse",
    // Audio in and out.
    "lyria",
    "tts",
    "speech",
    "whisper",
    // Retrieval and classification endpoints.
    "embedding",
    "embed",
    "moderation",
    "rerank",
];

/// Output modalities that still mean "a model that can answer": the media-only rule below hides
/// a row only when **every** output it declares is something other than these.
const CHAT_OUTPUTS: &[&str] = &["text", "reasoning", "tool", "tools", "code", "json"];

/// Safety ceiling above the built-in provider count. Reads are concurrent, so this prevents an
/// accidentally unbounded future registry without hiding any provider Rigel currently supports.
pub const MAX_CATALOGS: usize = 32;

/// What a candidate key proved before Rigel writes it to the keychain (Q244).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyValidation {
    /// Whether the provider answered a request that carried the candidate key.
    pub checked: bool,
    /// How many models the roster returned; zero when no check was possible.
    pub models: usize,
    /// One sentence for Settings and `rigel keys set`.
    pub note: String,
}

/// The catalogue for one provider. The key is resolved here and only becomes a header; no caller
/// ever receives it.
pub async fn catalog(
    provider: &str,
    store: &dyn CredentialStore,
) -> Result<Vec<ModelOption>, String> {
    let spec = providers::find(provider).ok_or_else(|| providers::unknown_provider(provider))?;

    // A custom door's roster is the list its own probe returned on save; reading it from the
    // keychain keeps the picker usable without a second request and without header values.
    if spec.id == credentials::CUSTOM {
        let door = custom::load_from(store)?.ok_or_else(|| {
            "no custom door is saved; add its base URL and header in Settings → Providers"
                .to_string()
        })?;
        return Ok(door
            .models
            .iter()
            .filter(|id| !id.trim().is_empty())
            .map(|id| ModelOption {
                id: id.clone(),
                provider: spec.id.to_string(),
                name: None,
                free: false,
                chat: true,
                tools: true,
                local: false,
                modality: None,
                output_micros_per_mtok: None,
            })
            .collect());
    }

    // Google's two doors publish no roster on their chat route; the provider's own default is
    // still worth offering, so the picker says where a turn would go instead of showing nothing.
    if !spec.lists_models {
        return Ok(default_row(spec).into_iter().collect());
    }

    if matches!(spec.endpoint, Endpoint::Router) {
        // A router that is down is a process to launch, not a missing configuration.
        super::omniroute::ensure_server().await?;
    }

    let key = match providers::credential(spec, store).await {
        // A keyless local door has an empty credential, not a header to send.
        Ok(key) if key.is_empty() => None,
        Ok(key) => Some(key),
        // OpenRouter's roster is public: reading it is how an install on another provider sees
        // what it could switch to. Anywhere else the key is what the list needs.
        Err(_) if spec.id == credentials::OPENROUTER => None,
        Err(error) => return Err(error),
    };

    let url = providers::models_url(spec)?;
    let mut rows = read_roster(&url, spec, key).await?;
    if spec.id == credentials::FIREWORKS {
        add_fireworks_router_rows(&mut rows);
    }
    Ok(rows)
}

/// FireRouter is a routing id, not always a resource the `/models` roster returns. Offer it
/// explicitly so the picker can select it; the router reports the concrete served model in usage.
fn add_fireworks_router_rows(rows: &mut Vec<ModelOption>) {
    let present = rows
        .iter()
        .any(|row| row.id.eq_ignore_ascii_case("firerouter") || row.id.ends_with("/firerouter"));
    if !present {
        rows.push(ModelOption {
            id: "firerouter".to_string(),
            provider: credentials::FIREWORKS.to_string(),
            name: Some("FireRouter · auto".to_string()),
            free: false,
            chat: true,
            tools: true,
            local: false,
            modality: None,
            output_micros_per_mtok: None,
        });
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id));
}

/// Check a candidate provider key before it is stored (Q244).
///
/// The candidate goes into a throwaway store rather than the keychain, and the same roster call
/// the picker uses makes the decision, so saving a key and reading a roster cannot disagree. A
/// provider with no published roster (Gemini AI Studio, Vertex) is reported as unchecked instead
/// of being called valid: there is no model-list request that can prove those credentials.
pub async fn validate_key(provider: &str, value: &str) -> Result<KeyValidation, String> {
    let spec = providers::find(provider).ok_or_else(|| providers::unknown_provider(provider))?;
    let value = value.trim();
    if value.is_empty() {
        return Err("paste a key first".to_string());
    }
    if !spec.lists_models {
        return Ok(KeyValidation {
            checked: false,
            models: 0,
            note: format!(
                "{} publishes no model roster, so Rigel could not check the key with a model-list call.",
                spec.name
            ),
        });
    }
    // OpenRouter's `/models` list is public, so a 200 there says nothing about the candidate.
    // Its authenticated key record is the lightweight proof that the key was accepted.
    if spec.id == credentials::OPENROUTER {
        let url = format!("{}/key", providers::base_url(spec)?);
        return validate_openrouter(spec, value, &url).await;
    }
    if matches!(spec.endpoint, Endpoint::Router) {
        super::omniroute::ensure_server().await?;
    }
    let url = providers::models_url(spec)?;
    validate_at(spec, value, &url).await
}
/// OpenRouter's authenticated key record: `/api/v1/key` answers 401 for a rejected candidate
/// and 200 with a `data` object for a live one.
async fn validate_openrouter(
    spec: &ProviderSpec,
    value: &str,
    url: &str,
) -> Result<KeyValidation, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|error| format!("could not build an HTTP client: {error}"))?;
    let response = client
        .get(url)
        .bearer_auth(value)
        .send()
        .await
        .map_err(|error| format!("could not validate the key with {}: {error}", spec.name))?;
    if !response.status().is_success() {
        return Err(format!(
            "{} did not accept the key: {} answered {}",
            spec.name,
            spec.name,
            response.status()
        ));
    }
    let record: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("{} returned unreadable key data: {error}", spec.name))?;
    if record.get("data").is_none() {
        return Err(format!("{} returned no key record", spec.name));
    }
    Ok(KeyValidation {
        checked: true,
        models: 0,
        note: format!("{} accepted the key.", spec.name),
    })
}

/// The HTTP half of [`validate_key`], split out so a test can point it at a scripted server.
async fn validate_at(spec: &ProviderSpec, value: &str, url: &str) -> Result<KeyValidation, String> {
    let models = read_roster(url, spec, Some(value.to_string()))
        .await
        .map_err(|error| format!("{} did not accept the key: {error}", spec.name))?;
    Ok(KeyValidation {
        checked: true,
        models: models.len(),
        note: format!(
            "{} accepted the key and listed {} model{}.",
            spec.name,
            models.len(),
            if models.len() == 1 { "" } else { "s" }
        ),
    })
}

/// The merged list: the current provider first, then every other provider that holds a
/// credential, deduplicated per provider and ordered so the useful end stays reachable.
pub async fn roster(
    current: &str,
    store: &dyn CredentialStore,
) -> Result<Vec<ModelOption>, String> {
    let current = current.trim().to_lowercase();
    let mut targets: Vec<String> = Vec::new();
    for id in std::iter::once(current.clone())
        .chain(std::iter::once(credentials::OPENROUTER.to_string()))
        .chain(
            providers::all()
                .iter()
                .filter(|spec| local::is_local(spec.id) || providers::present(store, spec))
                .map(|spec| spec.id.to_string()),
        )
    {
        if !targets.contains(&id) {
            targets.push(id);
        }
    }
    targets.truncate(MAX_CATALOGS);

    // The doors are independent network round trips, so they are asked at once: in sequence the
    // picker waited for the sum (measured 650 ms + 941 ms + 171 ms on this machine) instead of
    // the slowest one. The merge below still runs in `targets` order, so the current provider
    // leads and a failure is still reported per provider.
    let reads = futures::future::join_all(targets.iter().map(|id| catalog(id, store))).await;
    let mut rows = Vec::new();
    let mut problems = Vec::new();
    for (id, result) in targets.iter().zip(reads) {
        match result {
            Ok(read) => rows.extend(read),
            Err(error) => problems.push(format!("{id}: {error}")),
        }
    }
    if rows.is_empty() {
        return Err(if problems.is_empty() {
            "no provider catalogue was reachable".to_string()
        } else {
            problems.join("; ")
        });
    }
    Ok(merged(&current, rows))
}

/// The provider's own default as a one-row catalogue. `None` when the provider has no default
/// and no roster to read, which is a turn that has to be told a model rather than guess one.
fn default_row(spec: &ProviderSpec) -> Option<ModelOption> {
    spec.default_model.map(|default| ModelOption {
        id: default.to_string(),
        provider: spec.id.to_string(),
        // The remote agent is named by its id, exactly; a provider default says whose it is.
        name: Some(if spec.id == credentials::RIGEL {
            default.to_string()
        } else {
            format!("{} default", spec.name)
        }),
        free: false,
        chat: true,
        tools: true,
        local: local::is_local(spec.id),
        modality: None,
        output_micros_per_mtok: Some(
            (super::pricing::price_for(default).output * 1_000_000.0).round() as u64,
        ),
    })
}

/// `GET {base}/models`, split from [`catalog`] so a test can point it at a scripted server and so
/// the parsing rule has one home.
async fn read_roster(
    url: &str,
    spec: &ProviderSpec,
    key: Option<String>,
) -> Result<Vec<ModelOption>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|error| format!("could not build an HTTP client: {error}"))?;

    // A roster is a GET: it is idempotent, so a 5xx or a lost connection may be retried without
    // duplicating anything a user can see. The retry is still bounded, and each wait is short.
    let mut attempt = 0;
    let response = loop {
        attempt += 1;
        let mut request = client.get(url);
        // Whatever the vendor needs on **every** request, the roster included: Anthropic dates
        // its API version, and a list without it is a 400 rather than a catalogue.
        for (name, value) in spec.headers {
            request = request.header(*name, *value);
        }
        if let Some(key) = key.clone() {
            request = request.bearer_auth(key);
        }

        match request.send().await {
            Ok(response) if response.status().is_success() => break response,
            Ok(response) => {
                let status = response.status().as_u16();
                if let Some(retry) = retry::decision(
                    RetryClass::Read,
                    attempt,
                    3,
                    Some(status),
                    false,
                    false,
                    None,
                    150,
                ) {
                    tokio::time::sleep(std::time::Duration::from_millis(retry.delay_ms)).await;
                    continue;
                }
                // Anthropic answers `x-api-key` and `anthropic-version`; a bare status is enough
                // here, and the caller already has the provider's name.
                return Err(format!("{} answered {status}", spec.name));
            }
            Err(error) => {
                let transport = error.is_timeout() || error.is_connect() || error.is_request();
                if let Some(retry) = retry::decision(
                    RetryClass::Read,
                    attempt,
                    3,
                    None,
                    transport,
                    false,
                    None,
                    150,
                ) {
                    tokio::time::sleep(std::time::Duration::from_millis(retry.delay_ms)).await;
                    continue;
                }
                return Err(format!("could not read the model catalogue: {error}"));
            }
        }
    };

    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("{} returned unreadable data: {error}", spec.name))?;

    let mut models: Vec<ModelOption> = value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let id = row.get("id").and_then(serde_json::Value::as_str)?;
                    let name = row
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                        .filter(|name| name != id);
                    let pricing = row.get("pricing");
                    let published_zero = is_zero(pricing.and_then(|price| price.get("prompt")))
                        && is_zero(pricing.and_then(|price| price.get("completion")));
                    // Published live prices outrank a stale `:free` suffix. The suffix is used only
                    // when the provider omitted pricing entirely; a newly nonzero row is paid now.
                    let free = local::is_local(spec.id)
                        || published_zero
                        || (pricing.is_none() && id.ends_with(":free"));
                    let output_micros_per_mtok = pricing
                        .and_then(|price| price.get("completion"))
                        .and_then(serde_json::Value::as_str)
                        .and_then(|value| value.parse::<f64>().ok())
                        .map(|per_token| (per_token * 1_000_000.0 * 1_000_000.0).round() as u64)
                        .filter(|micros| *micros > 0 || free);
                    let tools = row
                        .get("supported_parameters")
                        .and_then(serde_json::Value::as_array)
                        .map(|parameters| {
                            parameters
                                .iter()
                                .filter_map(serde_json::Value::as_str)
                                .any(|parameter| matches!(parameter, "tools" | "tool_choice"))
                        })
                        .unwrap_or(true);
                    Some(ModelOption {
                        id: id.to_string(),
                        provider: spec.id.to_string(),
                        name,
                        free,
                        chat: is_chat_like(id, row),
                        tools,
                        local: local::is_local(spec.id),
                        modality: modality_label(row),
                        output_micros_per_mtok,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    models.sort_by(|one, other| one.id.cmp(&other.id));
    models.dedup_by(|one, other| one.id == other.id);
    if models.is_empty() {
        return Err(format!("{} returned no models", spec.name));
    }
    Ok(models)
}
/// Whether a roster row can answer a chat turn. Two signals hide a row, and neither rescues one:
///
/// * the vendor's declaration, when **every** output it names is something other than text,
///   reasoning, a tool call or code — a model with no chat output at all;
/// * the id, against [`NOT_CHAT`].
///
/// The second signal is the one that catches `google/lyria-3-pro-preview`: it declares
/// `text+image->text+audio`, and a music model does return text about the track it made, so the
/// declaration alone cannot tell it from `openai/gpt-5-image` (`text+image+file->text+image`, a
/// chat model that can also draw). The id can: `lyria` is a family, `image` is not.
fn is_chat_like(id: &str, row: &serde_json::Value) -> bool {
    if let Some(outputs) = output_modalities(row)
        && outputs
            .iter()
            .all(|modality| !CHAT_OUTPUTS.contains(&modality.as_str()))
    {
        return false;
    }
    !NOT_CHAT
        .iter()
        .any(|token| matches_phrase(&id.to_ascii_lowercase(), token))
}

/// The vendor's declared outputs, from whichever shape it publishes them in: the array
/// (`architecture.output_modalities`, OpenRouter) or the single `input->output` string
/// (`architecture.modality`). A placeholder (an empty string) counts as publishing nothing.
fn output_modalities(row: &serde_json::Value) -> Option<Vec<String>> {
    let architecture = row.get("architecture");
    let mut named: Vec<String> = architecture
        .and_then(|value| value.get("output_modalities"))
        .and_then(serde_json::Value::as_array)
        .or_else(|| {
            row.get("output_modalities")
                .and_then(serde_json::Value::as_array)
        })
        .map(|outputs| {
            outputs
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let single = architecture
        .and_then(|value| value.get("modality"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| row.get("modality").and_then(serde_json::Value::as_str));
    if named.is_empty()
        && let Some(single) = single
        && let Some((_, output)) = single.split_once("->")
    {
        named = output
            .split('+')
            .map(str::trim)
            .map(str::to_string)
            .collect();
    }
    named.retain(|modality| !modality.trim().is_empty());
    (!named.is_empty()).then_some(named)
}

/// The vendor's own description, kept for the JSON face so the rule above can be audited: the
/// single `input->output` string when it publishes one, otherwise the declared outputs.
fn modality_label(row: &serde_json::Value) -> Option<String> {
    let architecture = row.get("architecture");
    let single = architecture
        .and_then(|value| value.get("modality"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| row.get("modality").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match single {
        Some(single) => Some(single.to_string()),
        None => output_modalities(row).map(|outputs| format!("output: {}", outputs.join("+"))),
    }
}

/// Whether `phrase` appears in `text` between separators — `sora-2` and `sora` yes, `inkling`
/// and `kling` no.
fn matches_phrase(text: &str, phrase: &str) -> bool {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .collect::<Vec<_>>()
        .windows(phrase.split('-').count())
        .any(|window| window.join("-") == phrase)
}

/// A price the vendor published as the string `"0"`, `"0.0"` or `"0.000000"`.
fn is_zero(value: Option<&serde_json::Value>) -> bool {
    value
        .and_then(serde_json::Value::as_str)
        .map(|price| price.trim().parse::<f64>().is_ok_and(|price| price == 0.0))
        .unwrap_or(false)
}

/// Deduplicate and order, with no network in it so the rule can be tested directly.
///
/// A model can exist on two doors — `openai/gpt-oss-120b` on Groq and on OpenRouter — and the
/// two are not interchangeable (different keys, different prices, sometimes different weights),
/// so a row is deduplicated per provider, not per id.
fn merged(current: &str, catalog: Vec<ModelOption>) -> Vec<ModelOption> {
    let mut seen = std::collections::HashSet::new();
    let mut catalog: Vec<ModelOption> = catalog
        .into_iter()
        .filter(|option| seen.insert((option.provider.clone(), option.id.clone())))
        .collect();
    // The current provider first, then free rows, then name order: the useful end of a long
    // roster stays reachable without typing a search first.
    catalog.sort_by(|one, other| {
        (one.provider != current)
            .cmp(&(other.provider != current))
            .then_with(|| other.free.cmp(&one.free))
            .then_with(|| one.id.to_lowercase().cmp(&other.id.to_lowercase()))
    });
    catalog
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryCredentialStore;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// One request, read fully, answered with the roster shape the vendors publish.
    async fn fake_roster(body: &'static str) -> (String, Arc<Mutex<String>>) {
        fake_roster_with(200, "OK", body).await
    }

    async fn fake_roster_with(
        status: u16,
        reason: &'static str,
        body: &'static str,
    ) -> (String, Arc<Mutex<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(String::new()));
        let log = seen.clone();

        tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let mut request = vec![0u8; 8192];
            let read = socket.read(&mut request).await.unwrap_or(0);
            *log.lock().unwrap() = String::from_utf8_lossy(&request[..read]).to_string();
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
        });

        (format!("http://{address}/models"), seen)
    }

    fn row(id: &str, provider: &str, free: bool) -> ModelOption {
        ModelOption {
            id: id.to_string(),
            provider: provider.to_string(),
            name: None,
            free,
            chat: true,
            tools: true,
            local: false,
            modality: None,
            output_micros_per_mtok: None,
        }
    }

    #[test]
    fn a_firerouter_row_is_offered_once_even_when_the_roster_omits_it() {
        let mut rows = vec![row("glm-5p3-flash", credentials::FIREWORKS, false)];
        add_fireworks_router_rows(&mut rows);
        add_fireworks_router_rows(&mut rows);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "firerouter");

        let mut full = vec![row(
            "accounts/fireworks/routers/firerouter",
            credentials::FIREWORKS,
            false,
        )];
        add_fireworks_router_rows(&mut full);
        assert_eq!(full.len(), 1, "the full router resource is the same row");
    }

    #[tokio::test]
    async fn a_roster_is_read_parsed_and_ordered() {
        let (url, seen) = fake_roster(
            r#"{"data":[
                {"id":"z-model","name":"Zed","pricing":{"prompt":"0.000001","completion":"0.000002"}},
                {"id":"a-model","name":"Aye"},
                {"id":"free-model:free","pricing":{"prompt":"0","completion":"0"}},
                {"id":"was-free:free","pricing":{"prompt":"0.000001","completion":"0.000002"}},
                {"id":"a-model","name":"Aye again"}
            ]}"#,
        )
        .await;

        let spec = providers::find(credentials::GEMINI).unwrap();
        let rows = read_roster(&url, spec, Some("test-key".to_string()))
            .await
            .expect("the roster reads");

        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            vec!["a-model", "free-model:free", "was-free:free", "z-model"],
            "sorted by id, the duplicate collapsed"
        );
        assert!(rows[1].free, "a `:free` id is free without pricing");
        assert_eq!(rows[1].output_micros_per_mtok, Some(0));
        assert!(
            !rows[2].free,
            "live nonzero pricing overrides a stale `:free` suffix"
        );
        assert_eq!(rows[2].output_micros_per_mtok, Some(2_000_000));
        assert_eq!(rows[3].output_micros_per_mtok, Some(2_000_000));
        // A vendor that publishes no pricing at all must not be read as free.
        assert!(!rows[0].free);
        assert_eq!(rows[0].output_micros_per_mtok, None);
        assert_eq!(rows[0].name.as_deref(), Some("Aye"));
        assert!(
            rows.iter().all(|row| row.chat),
            "a roster with no modality metadata keeps every row chat-eligible"
        );

        let request = seen.lock().unwrap().clone();
        assert!(request.starts_with("GET /models "), "{request}");
        assert!(
            request.contains("authorization: Bearer test-key"),
            "the key arrives as a bearer header: {request}"
        );
    }

    #[tokio::test]
    async fn a_roster_marks_tool_capable_rows_and_keeps_unknown_rows_usable() {
        let (url, _) = fake_roster(
            r#"{"data":[
                {"id":"tools-model","supported_parameters":["temperature","tools"]},
                {"id":"text-model","supported_parameters":["temperature"]},
                {"id":"unknown-model"}
            ]}"#,
        )
        .await;
        let spec = providers::find(credentials::GEMINI).unwrap();
        let rows = read_roster(&url, spec, Some("test-key".to_string()))
            .await
            .unwrap();
        let row = |id: &str| rows.iter().find(|row| row.id == id).unwrap();
        assert!(row("tools-model").tools);
        assert!(!row("text-model").tools);
        assert!(
            row("unknown-model").tools,
            "a roster that omits the field is unknown-but-usable, not silently excluded"
        );
    }

    /// The rows this rule exists for, taken from the live rosters: what a chat picker must not
    /// lead with, and the rows a looser rule hid by mistake on the first live run.
    #[test]
    fn the_chat_rule_reads_metadata_first_and_falls_back_to_the_id() {
        let empty = serde_json::json!({});
        let metadata = |outputs: &str| serde_json::json!({ "architecture": { "output_modalities": [outputs] } });

        // Router rows publish nothing, so the id decides.
        assert!(!is_chat_like("aihorde/AbyssOrangeMix-AfterDark", &empty));
        assert!(!is_chat_like("veo-free/veo-3", &empty));
        assert!(!is_chat_like("sora-2-pro", &empty));
        assert!(is_chat_like("dva/medium", &empty));
        assert!(is_chat_like("auto/coding:free", &empty));
        // The false positive the live run caught: `inkling` is not `kling`.
        assert!(is_chat_like("dva/inkling-high", &empty));
        assert!(is_chat_like("codex/gpt-5.6-luna", &empty));

        // A vendor that declares an output with no chat part at all needs no name rule, and
        // `text+image->text+image` is a chat model that can also draw — the row a looser name
        // rule gets wrong, because `image` is not a family the way `lyria` is.
        assert!(!is_chat_like(
            "google/lyria-3-pro-preview",
            &serde_json::json!({ "architecture": { "modality": "text+image->text+audio" } })
        ));
        assert!(is_chat_like(
            "openai/gpt-5-image",
            &serde_json::json!({ "architecture": { "modality": "text+image+file->text+image" } })
        ));
        assert!(is_chat_like(
            "google/gemini-3-pro-image",
            &serde_json::json!({ "architecture": { "output_modalities": ["text", "image"] } })
        ));
        // Only a **media-only** output hides a row: an alias that answers with reasoning is a
        // chat model, and the first live run must not hide the codex aliases.
        assert!(is_chat_like(
            "codex/gpt-5.6-sol",
            &serde_json::json!({ "architecture": { "output_modalities": ["reasoning"] } })
        ));
        assert!(!is_chat_like(
            "vendor/mystery-1",
            &serde_json::json!({ "architecture": { "output_modalities": ["audio"] } })
        ));
        // A provider that publishes an empty list is treated as publishing nothing, and the id
        // still decides.
        assert!(is_chat_like("vendor/plain-1", &metadata("")));
        assert!(is_chat_like("vendor/plain-2", &metadata("text")));
        assert!(!is_chat_like("vendor/flux-2", &empty));
        assert!(!is_chat_like("openai/text-embedding-4-large", &empty));
        assert!(!is_chat_like("openai/omni-moderation-latest", &empty));
        assert!(!is_chat_like("openai/gpt-image-1", &empty));
        assert_eq!(
            modality_label(&serde_json::json!({ "architecture": { "modality": "text->audio" } }))
                .as_deref(),
            Some("text->audio")
        );
    }

    #[test]
    fn a_grouped_provider_without_a_roster_offers_its_default() {
        // AI Studio's OpenAI-compatible route publishes no `/models`; the picker says where a
        // turn would go instead of showing an empty list.
        let spec = providers::find(credentials::GEMINI).unwrap();
        let rows = default_row(spec).expect("Gemini has a default");
        assert_eq!(rows.id, super::super::gemini::DEFAULT_MODEL);
        assert_eq!(rows.provider, credentials::GEMINI);
        assert_eq!(rows.name.as_deref(), Some("Google AI Studio default"));
    }

    #[test]
    fn the_merge_keeps_one_row_per_provider_and_leads_with_the_current_one() {
        let merged = merged(
            "groq",
            vec![
                row("openai/gpt-oss-120b", credentials::OPENROUTER, false),
                row("z-model", credentials::GROQ, false),
                row("openai/gpt-oss-120b", credentials::GROQ, false),
                row("a-free", credentials::GROQ, true),
                row("a-free", credentials::GROQ, true),
            ],
        );

        assert_eq!(
            merged
                .iter()
                .map(|option| (option.provider.as_str(), option.id.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("groq", "a-free"),
                ("groq", "openai/gpt-oss-120b"),
                ("groq", "z-model"),
                ("openrouter", "openai/gpt-oss-120b"),
            ],
            "the same id on two doors stays two rows; the current provider leads, free first"
        );
    }

    #[tokio::test]
    async fn a_candidate_key_is_checked_before_it_can_be_stored() {
        let (url, seen) = fake_roster(r#"{"data":[{"id":"a-model"},{"id":"b-model"}]}"#).await;
        let spec = providers::find(credentials::GROQ).unwrap();

        let check = validate_at(spec, "candidate-key", &url)
            .await
            .expect("the roster accepts the candidate");

        assert!(check.checked);
        assert_eq!(check.models, 2);
        assert!(
            check.note.contains("Groq accepted the key"),
            "{}",
            check.note
        );
        let request = seen.lock().unwrap().clone();
        assert!(
            request.contains("authorization: Bearer candidate-key"),
            "the candidate, not a keychain entry, is sent: {request}"
        );
    }

    #[tokio::test]
    async fn openrouter_uses_the_authenticated_key_record_not_the_public_roster() {
        let (url, seen) = fake_roster_with(200, "OK", r#"{"data":{"label":"candidate"}}"#).await;
        let url = url.replace("/models", "/key");
        let spec = providers::find(credentials::OPENROUTER).unwrap();

        let check = validate_openrouter(spec, "candidate-key", &url)
            .await
            .expect("the key record accepts the candidate");

        assert!(check.checked);
        assert_eq!(check.models, 0, "the key record lists no models");
        assert!(
            check.note.contains("OpenRouter accepted the key"),
            "{}",
            check.note
        );
        let request = seen.lock().unwrap().clone();
        assert!(request.starts_with("GET /key "), "{request}");
        assert!(
            request.contains("authorization: Bearer candidate-key"),
            "the candidate is what OpenRouter checks: {request}"
        );
    }

    #[tokio::test]
    async fn a_rejected_candidate_names_the_provider_and_never_reads_as_valid() {
        let (url, seen) = fake_roster_with(401, "Unauthorized", r#"{"error":"bad key"}"#).await;
        let spec = providers::find(credentials::GROQ).unwrap();

        let error = validate_at(spec, "bad-key", &url)
            .await
            .expect_err("a 401 is not a valid key");

        assert!(error.contains("Groq did not accept the key"), "{error}");
        assert!(error.contains("401"), "{error}");
        let request = seen.lock().unwrap().clone();
        assert!(
            request.contains("authorization: Bearer bad-key"),
            "the rejected candidate was the key sent: {request}"
        );
    }

    #[tokio::test]
    async fn a_provider_without_a_roster_is_reported_unchecked() {
        let check = validate_key(credentials::GEMINI, "candidate-key")
            .await
            .expect("a provider without a roster is not an error");

        assert!(!check.checked);
        assert_eq!(check.models, 0);
        assert!(check.note.contains("no model roster"), "{}", check.note);
    }

    #[tokio::test]
    async fn an_unknown_provider_is_named_rather_than_probed() {
        let store = MemoryCredentialStore::default();
        let error = catalog("nope", &store).await.expect_err("no such row");
        assert!(error.contains("omniroute"), "{error}");
    }

    #[tokio::test]
    async fn a_provider_without_a_credential_says_how_to_add_one() {
        let store = MemoryCredentialStore::default();
        // The environment is checked before any store, so this only means anything on a machine
        // that has no `GROQ_API_KEY` set either.
        if credentials::present_in(&store, credentials::GROQ) {
            return;
        }
        let error = catalog(credentials::GROQ, &store)
            .await
            .expect_err("no key, no roster");
        assert!(error.contains("rigel keys set groq"), "{error}");
    }
}

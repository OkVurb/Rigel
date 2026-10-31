//! The model catalogue the picker shows, and where each row came from.
//!
//! A picker that only knows the provider in Settings is a picker that hides every model the user
//! has already paid for: a stored key is the permission to use that door, so **every** provider
//! that holds a credential is asked for its roster. The rules are small enough to state:
//!
//! * the current provider's roster is read first, so a walk cut short by [`MAX_CATALOGS`] is cut
//!   at the end of the list rather than under the model the composer is showing;
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

use super::providers::{self, Endpoint, ProviderSpec};

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
    /// The vendor's own `input->output` description, when it publishes one. Carried so the
    /// [`chat`](Self::chat) rule can be audited from `rigel models --json` instead of guessed at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modality: Option<String>,
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

/// How many rosters one call may read. A cheap install can hold half a dozen keys, and a picker
/// that spends a minute on timeouts is worse than one that shows five rosters.
pub const MAX_CATALOGS: usize = 6;

/// The catalogue for one provider. The key is resolved here and only becomes a header; no caller
/// ever receives it.
pub async fn catalog(
    provider: &str,
    store: &dyn CredentialStore,
) -> Result<Vec<ModelOption>, String> {
    let spec = providers::find(provider).ok_or_else(|| providers::unknown_provider(provider))?;

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
        Ok(key) => Some(key),
        // OpenRouter's roster is public: reading it is how an install on another provider sees
        // what it could switch to. Anywhere else the key is what the list needs.
        Err(_) if spec.id == credentials::OPENROUTER => None,
        Err(error) => return Err(error),
    };

    let url = providers::models_url(spec)?;
    read_roster(&url, spec, key).await
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
                .filter(|spec| credentials::present_in(store, spec.id))
                .map(|spec| spec.id.to_string()),
        )
    {
        if !targets.contains(&id) {
            targets.push(id);
        }
    }
    targets.truncate(MAX_CATALOGS);

    let mut rows = Vec::new();
    let mut problems = Vec::new();
    for id in &targets {
        match catalog(id, store).await {
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
        name: Some(format!("{} default", spec.name)),
        free: false,
        chat: true,
        modality: None,
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

    let mut request = client.get(url);
    // Whatever the vendor needs on **every** request, the roster included: Anthropic dates its
    // API version, and a list without it is a 400 rather than a catalogue.
    for (name, value) in spec.headers {
        request = request.header(*name, *value);
    }
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("could not read the model catalogue: {error}"))?;
    if !response.status().is_success() {
        // Anthropic answers `x-api-key` and `anthropic-version`; a bare status is enough here,
        // and the caller already has the provider's name.
        return Err(format!("{} answered {}", spec.name, response.status()));
    }

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
                    let free = id.ends_with(":free")
                        || (is_zero(pricing.and_then(|price| price.get("prompt")))
                            && is_zero(pricing.and_then(|price| price.get("completion"))));
                    Some(ModelOption {
                        id: id.to_string(),
                        provider: spec.id.to_string(),
                        name,
                        free,
                        chat: is_chat_like(id, row),
                        modality: modality_label(row),
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
    if let Some(outputs) = output_modalities(row) {
        if outputs
            .iter()
            .all(|modality| !CHAT_OUTPUTS.contains(&modality.as_str()))
        {
            return false;
        }
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
    if named.is_empty() {
        if let Some(single) = single {
            if let Some((_, output)) = single.split_once("->") {
                named = output
                    .split('+')
                    .map(str::trim)
                    .map(str::to_string)
                    .collect();
            }
        }
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
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
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
            modality: None,
        }
    }

    #[tokio::test]
    async fn a_roster_is_read_parsed_and_ordered() {
        let (url, seen) = fake_roster(
            r#"{"data":[
                {"id":"z-model","name":"Zed","pricing":{"prompt":"0.000001","completion":"0.000002"}},
                {"id":"a-model","name":"Aye"},
                {"id":"free-model:free","pricing":{"prompt":"0","completion":"0"}},
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
            vec!["a-model", "free-model:free", "z-model"],
            "sorted by id, the duplicate collapsed"
        );
        assert!(rows[1].free, "a `:free` id is free without pricing");
        // A vendor that publishes no pricing at all must not be read as free.
        assert!(!rows[0].free);
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

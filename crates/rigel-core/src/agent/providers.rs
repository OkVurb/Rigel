//! Every provider adapter a chat turn can go through, in one table.
//!
//! Before this file the same three names were matched by hand in the desktop app **and** in the
//! CLI, and a key could exist in the keychain while `rigel keys status` never mentioned it. One
//! row per adapter removes both problems: the id, its base URL, its key and the namespace it
//! owns live here, and [`build`] turns a row into a [`Model`] with the credential resolved.
//!
//! Every row speaks the OpenAI-compatible contract — `POST {base}/chat/completions`, streamed —
//! except `anthropic`, whose Messages API is a contract of its own ([`anthropic`]) with its own
//! client. Its **row** lives here like everyone else's: the id, the base, the namespace, the key
//! and the hint, so Settings, `rigel keys status` and the picker need no special case.
//!
//! Three of them need more than a base URL and a key:
//!
//! * [`Endpoint::Router`] is a process on this machine, started on demand;
//! * [`Endpoint::Vertex`] carries a project and a location in its URL and takes an OAuth access
//!   token rather than an API key;
//! * `anthropic` takes a key but not this request shape, so [`build`] hands it to
//!   [`anthropic::AnthropicModel`].
//!
//! Base URLs were taken from each vendor's OpenAI-compatibility page (Oct 2026). The one row
//! whose model listing could not be confirmed on the same base is `fireworks`; a picker that
//! cannot read a roster shows the provider's default row and the error.

use std::sync::Arc;

use crate::credentials::{self, CredentialStore};

use super::model::Model;
use super::{anthropic, gemini, omniroute, openrouter, vertex};

/// Where a provider's `/chat/completions` lives.
pub enum Endpoint {
    /// The local router, whose base is `RIGEL_OMNIROUTE_URL` and which is started on demand.
    Router,
    /// Google AI Studio, whose base is `RIGEL_GEMINI_BASE_URL`.
    AiStudio,
    /// Vertex AI: the base carries the project and the location.
    Vertex,
    /// A fixed base URL, with `env` as the variable that overrides it.
    Fixed {
        base: &'static str,
        env: &'static str,
    },
}

pub struct ProviderSpec {
    /// Also the credential name: the keychain entry is `rigel/<id>`.
    pub id: &'static str,
    pub name: &'static str,
    pub endpoint: Endpoint,
    /// `Some` only where the vendor publishes a default that is safe to assume. Everywhere else
    /// the picker reads the live roster, and a turn with no model says so instead of guessing.
    pub default_model: Option<&'static str>,
    /// Whether the vendor documents OpenRouter's `usage: { include: true }` cost field.
    pub usage_request: bool,
    /// Whether `{base}/models` answers with the vendor's roster.
    pub lists_models: bool,
    /// The credential is a short-lived token rather than a key, and the copy says so.
    pub access_token: bool,
    /// A gateway takes any id: that is what a gateway is for.
    pub gateway: bool,
    /// The id prefixes this provider unmistakably owns. Empty means it serves other vendors'
    /// weights and takes any id.
    pub namespace: &'static [&'static str],
    /// One line for Settings, beside the key field.
    pub hint: &'static str,
    /// Where a key is created, when the vendor publishes a page for it.
    pub key_url: Option<&'static str>,
    /// Fixed headers every request to this provider carries, the model list included. Anthropic
    /// dates its API version per request; no other vendor here needs one.
    pub headers: &'static [(&'static str, &'static str)],
}

pub const PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        id: credentials::OMNIROUTE,
        name: "OmniRoute",
        endpoint: Endpoint::Router,
        default_model: Some(omniroute::DEFAULT_MODEL),
        usage_request: true,
        lists_models: true,
        access_token: false,
        gateway: true,
        namespace: &[],
        hint: "Local router · one key, any model",
        key_url: None,
        headers: &[],
    },
    ProviderSpec {
        id: credentials::OPENROUTER,
        name: "OpenRouter",
        endpoint: Endpoint::Fixed {
            base: openrouter::DEFAULT_BASE_URL,
            env: "RIGEL_OPENROUTER_BASE_URL",
        },
        default_model: Some(openrouter::DEFAULT_MODEL),
        usage_request: true,
        lists_models: true,
        access_token: false,
        gateway: true,
        namespace: &[],
        hint: "Hosted gateway · the widest roster",
        key_url: Some("https://openrouter.ai/keys"),
        headers: &[],
    },
    ProviderSpec {
        id: credentials::GEMINI,
        name: "Google AI Studio",
        endpoint: Endpoint::AiStudio,
        default_model: Some(gemini::DEFAULT_MODEL),
        usage_request: false,
        lists_models: false,
        access_token: false,
        gateway: false,
        namespace: &["gemini"],
        hint: "An AI Studio key · Google's own endpoint",
        key_url: Some("https://aistudio.google.com/apikey"),
        headers: &[],
    },
    ProviderSpec {
        id: credentials::VERTEX,
        name: "Google Vertex AI",
        endpoint: Endpoint::Vertex,
        default_model: Some(vertex::DEFAULT_MODEL),
        usage_request: false,
        lists_models: false,
        access_token: true,
        gateway: false,
        namespace: &["google/gemini"],
        hint: "Your GCP project · a short-lived access token",
        key_url: Some(
            "https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/start/openai",
        ),
        headers: &[],
    },
    ProviderSpec {
        id: credentials::ANTHROPIC,
        name: "Anthropic",
        endpoint: Endpoint::Fixed {
            base: anthropic::DEFAULT_BASE_URL,
            env: "RIGEL_ANTHROPIC_BASE_URL",
        },
        default_model: Some(anthropic::DEFAULT_MODEL),
        usage_request: false,
        // `GET {base}/models` answers `data[].id`, the same shape the picker already reads.
        lists_models: true,
        access_token: false,
        gateway: false,
        // `claude-…` is Anthropic's own (the OpenAI-compatible door for Anthropic's weights is
        // OpenRouter, which names them `anthropic/claude-…`).
        namespace: &["claude"],
        hint: "Claude's own endpoint · the Messages API",
        key_url: Some("https://platform.claude.com/settings/keys"),
        headers: anthropic::HEADERS,
    },
    ProviderSpec {
        id: credentials::OPENAI,
        name: "OpenAI",
        endpoint: Endpoint::Fixed {
            base: "https://api.openai.com/v1",
            env: "RIGEL_OPENAI_BASE_URL",
        },
        default_model: None,
        usage_request: false,
        lists_models: true,
        access_token: false,
        gateway: false,
        namespace: &["gpt", "o1", "o3", "o4", "chatgpt"],
        hint: "Direct · the same key Codex uses off the router",
        key_url: Some("https://platform.openai.com/api-keys"),
        headers: &[],
    },
    ProviderSpec {
        id: credentials::XAI,
        name: "xAI",
        endpoint: Endpoint::Fixed {
            base: "https://api.x.ai/v1",
            env: "RIGEL_XAI_BASE_URL",
        },
        default_model: None,
        usage_request: false,
        lists_models: true,
        access_token: false,
        gateway: false,
        namespace: &["grok"],
        hint: "Grok models, straight from xAI",
        key_url: Some("https://docs.x.ai/developers/quickstart"),
        headers: &[],
    },
    ProviderSpec {
        id: credentials::GROQ,
        name: "Groq",
        endpoint: Endpoint::Fixed {
            base: "https://api.groq.com/openai/v1",
            env: "RIGEL_GROQ_BASE_URL",
        },
        default_model: None,
        usage_request: false,
        lists_models: true,
        access_token: false,
        gateway: false,
        // Groq serves other vendors' weights: `openai/gpt-oss-…`, `meta-llama/…`, `qwen/…`.
        namespace: &[],
        hint: "Open-weight models on Groq hardware",
        key_url: Some("https://console.groq.com/docs/openai"),
        headers: &[],
    },
    ProviderSpec {
        id: credentials::DEEPSEEK,
        name: "DeepSeek",
        endpoint: Endpoint::Fixed {
            // DeepSeek's documented OpenAI-compatible base has no `/v1`, and its model list
            // lives at `{base}/models` — the same shape the catalogue reader uses.
            base: "https://api.deepseek.com",
            env: "RIGEL_DEEPSEEK_BASE_URL",
        },
        default_model: None,
        usage_request: false,
        lists_models: true,
        access_token: false,
        gateway: false,
        namespace: &["deepseek"],
        hint: "DeepSeek's own endpoint",
        key_url: Some("https://api-docs.deepseek.com/"),
        headers: &[],
    },
    ProviderSpec {
        id: credentials::MISTRAL,
        name: "Mistral",
        endpoint: Endpoint::Fixed {
            base: "https://api.mistral.ai/v1",
            env: "RIGEL_MISTRAL_BASE_URL",
        },
        default_model: None,
        usage_request: false,
        lists_models: true,
        access_token: false,
        gateway: false,
        namespace: &["mistral", "codestral", "magistral"],
        hint: "Mistral's own endpoint",
        key_url: Some(
            "https://docs.mistral.ai/getting-started/quickstarts/admin/manage-workspaces",
        ),
        headers: &[],
    },
    ProviderSpec {
        id: credentials::TOGETHER,
        name: "Together AI",
        endpoint: Endpoint::Fixed {
            base: "https://api.together.ai/v1",
            env: "RIGEL_TOGETHER_BASE_URL",
        },
        default_model: None,
        usage_request: false,
        lists_models: true,
        access_token: false,
        gateway: false,
        namespace: &[],
        hint: "Together's hosted roster",
        key_url: Some("https://docs.together.ai/docs/inference/openai-compatibility"),
        headers: &[],
    },
    ProviderSpec {
        id: credentials::FIREWORKS,
        name: "Fireworks AI",
        endpoint: Endpoint::Fixed {
            base: "https://api.fireworks.ai/inference/v1",
            env: "RIGEL_FIREWORKS_BASE_URL",
        },
        default_model: None,
        usage_request: false,
        lists_models: true,
        access_token: false,
        gateway: false,
        namespace: &[],
        hint: "Fireworks' inference endpoint",
        key_url: Some("https://docs.fireworks.ai/getting-started/quickstart"),
        headers: &[],
    },
];

pub fn all() -> &'static [ProviderSpec] {
    PROVIDERS
}

pub fn find(id: &str) -> Option<&'static ProviderSpec> {
    let id = id.trim().to_ascii_lowercase();
    PROVIDERS.iter().find(|spec| spec.id == id)
}

/// The message a typo gets, naming every provider that would have worked.
pub fn unknown_provider(id: &str) -> String {
    let names: Vec<&str> = PROVIDERS.iter().map(|spec| spec.id).collect();
    format!(
        "unknown provider `{}`; use one of {}",
        id.trim(),
        names.join(", ")
    )
}

/// The provider a model id unmistakably belongs to, when one does.
fn provider_owning(model: &str) -> Option<&'static str> {
    PROVIDERS.iter().find_map(|spec| {
        spec.namespace
            .iter()
            .any(|prefix| model.starts_with(prefix))
            .then_some(spec.id)
    })
}

/// Whether a saved model id can be sent to this provider.
///
/// Settings keep **one** model across a provider switch, which is how a Gemini id ends up in a
/// Groq request. The rule is a heuristic on purpose, and it has three shapes:
///
/// * a gateway takes any id, because routing ids is what a gateway is for;
/// * a door with a namespace of its own takes ids in that namespace and refuses another door's —
///   this is what keeps AI Studio's `gemini-4-argon` out of a Vertex request;
/// * a provider with no namespace serves other vendors' weights, so any id another door does not
///   claim is its business.
pub fn is_model(spec: &ProviderSpec, model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    if model.is_empty() {
        return false;
    }
    if spec.gateway {
        return true;
    }
    if !spec.namespace.is_empty() {
        return spec
            .namespace
            .iter()
            .any(|prefix| model.starts_with(prefix));
    }
    provider_owning(&model).is_none()
}

/// The model a turn with this provider will ask for: the saved id when it belongs to the
/// provider, otherwise the provider's own default — `""` when the picker has not chosen one yet,
/// which a turn reports rather than sending anyway.
pub fn configured_model(spec: &ProviderSpec, saved: &str) -> String {
    if is_model(spec, saved) {
        return saved.trim().to_string();
    }
    spec.default_model.unwrap_or_default().to_string()
}

pub fn base_url(spec: &ProviderSpec) -> Result<String, String> {
    match spec.endpoint {
        Endpoint::Router => Ok(omniroute::base_url()),
        Endpoint::AiStudio => Ok(gemini::base_url()),
        Endpoint::Vertex => vertex::base_url(),
        Endpoint::Fixed { base, env } => Ok(non_empty(env).unwrap_or_else(|| base.to_string())),
    }
}

/// `{base}/models`, for the picker.
pub fn models_url(spec: &ProviderSpec) -> Result<String, String> {
    Ok(format!("{}/models", base_url(spec)?))
}

/// The credential, resolved for this provider. Vertex is the one row that does not take a
/// key: its token, its project and its location each have their own rule.
pub async fn credential(
    spec: &ProviderSpec,
    store: &dyn CredentialStore,
) -> Result<String, String> {
    if matches!(spec.endpoint, Endpoint::Vertex) {
        return vertex::resolve_token(store).await;
    }
    credentials::get_from(store, spec.id)
        .map(|secret| secret.expose().to_string())
        .map_err(|error| format!("{error}. {}", key_help(spec)))
}
/// One sentence naming every way to give a provider a credential, and where one comes from.
pub fn key_help(spec: &ProviderSpec) -> String {
    let env =
        credentials::provider_environment_variable(spec.id).unwrap_or("its environment variable");
    let what = if spec.access_token { "token" } else { "key" };
    match spec.key_url {
        Some(url) => format!(
            "Add a {what} in Settings, store one with `rigel keys set {}`, set {env}, or create \
             one at {url}",
            spec.id
        ),
        None => format!(
            "Add a {what} in Settings, store one with `rigel keys set {}`, or set {env}",
            spec.id
        ),
    }
}

/// Resolve a provider into the model a turn will use.
pub async fn build(id: &str, store: &dyn CredentialStore) -> Result<Arc<dyn Model>, String> {
    let spec = find(id).ok_or_else(|| unknown_provider(id))?;
    if matches!(spec.endpoint, Endpoint::Router) {
        // A local router that is down is a process to launch, not a configuration error.
        omniroute::ensure_server().await?;
    }
    // The credential is resolved first: it is the one that expires, and naming it before the base
    // URL is what every other adapter's failure looks like.
    let key = credential(spec, store).await?;
    let base = base_url(spec)?;
    // One row is not an OpenAI-compatible door: the Messages API has its own request, its own
    // event stream and its own version header.
    if spec.id == credentials::ANTHROPIC {
        return Ok(Arc::new(anthropic::AnthropicModel::with_base_url(
            key, base,
        )));
    }
    let model = openrouter::OpenRouterModel::with_base_url(key, base).with_provider(spec.id);
    let model = if spec.usage_request {
        model
    } else {
        model.without_usage_request()
    };
    Ok(Arc::new(model))
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
    use crate::credentials::MemoryCredentialStore;

    fn spec(id: &str) -> &'static ProviderSpec {
        find(id).unwrap_or_else(|| panic!("{id} is a provider"))
    }

    /// `Result<Arc<dyn Model>, _>` has no `Debug` on its success side, so a failure is unwrapped
    /// by hand. The message is what the caller would have shown.
    async fn build_error(id: &str, store: &dyn CredentialStore) -> String {
        match build(id, store).await {
            Ok(_) => panic!("{id} built a model without a credential"),
            Err(error) => error,
        }
    }

    #[test]
    fn every_provider_has_a_name_a_hint_and_a_credential() {
        for spec in PROVIDERS {
            assert!(!spec.name.is_empty(), "{} has no name", spec.id);
            assert!(!spec.hint.is_empty(), "{} has no hint", spec.id);
            assert!(
                credentials::PROVIDERS.contains(&spec.id),
                "{} is not in the keychain list",
                spec.id
            );
            assert!(
                credentials::provider_environment_variable(spec.id).is_some(),
                "{} has no environment fallback",
                spec.id
            );
        }
    }

    #[test]
    fn ids_are_unique_and_looked_up_case_insensitively() {
        let mut seen = std::collections::HashSet::new();
        for spec in PROVIDERS {
            assert!(seen.insert(spec.id), "{} appears twice", spec.id);
        }
        assert_eq!(spec(" Vertex ").id, credentials::VERTEX);
        assert!(find("nope").is_none());
        assert!(unknown_provider("nope").contains("omniroute"));
    }

    #[test]
    fn the_fixed_bases_are_the_documented_ones() {
        assert_eq!(
            base_url(spec("openrouter")).unwrap(),
            "https://openrouter.ai/api/v1"
        );
        assert_eq!(
            base_url(spec("openai")).unwrap(),
            "https://api.openai.com/v1"
        );
        assert_eq!(base_url(spec("xai")).unwrap(), "https://api.x.ai/v1");
        assert_eq!(
            base_url(spec("groq")).unwrap(),
            "https://api.groq.com/openai/v1"
        );
        // DeepSeek's OpenAI-compatible base carries no `/v1`.
        assert_eq!(
            base_url(spec("deepseek")).unwrap(),
            "https://api.deepseek.com"
        );
        assert_eq!(
            base_url(spec("mistral")).unwrap(),
            "https://api.mistral.ai/v1"
        );
        assert_eq!(
            base_url(spec("together")).unwrap(),
            "https://api.together.ai/v1"
        );
        assert_eq!(
            base_url(spec("fireworks")).unwrap(),
            "https://api.fireworks.ai/inference/v1"
        );
        assert_eq!(
            base_url(spec("anthropic")).unwrap(),
            "https://api.anthropic.com/v1"
        );
        assert_eq!(
            models_url(spec("anthropic")).unwrap(),
            "https://api.anthropic.com/v1/models"
        );
        // The Messages API needs its dated version on every request, the roster included.
        assert_eq!(
            spec("anthropic").headers,
            &[("anthropic-version", anthropic::API_VERSION)]
        );
        assert!(spec("groq").headers.is_empty());
        assert_eq!(
            models_url(spec("groq")).unwrap(),
            "https://api.groq.com/openai/v1/models"
        );
    }

    #[test]
    fn a_stale_id_from_another_provider_is_not_sent() {
        // The bug this rule exists for: a Gemini id left in Settings met Groq's endpoint.
        assert_eq!(
            configured_model(spec("groq"), "gemini-4-argon"),
            "",
            "Gemini's id is not Groq's to send"
        );
        assert_eq!(
            configured_model(spec("vertex"), "gemini-4-argon"),
            "google/gemini-4-argon",
            "AI Studio's namespace is not Vertex's, so Vertex falls back to its own default"
        );
        assert_eq!(
            configured_model(spec("gemini"), "google/gemini-4-argon"),
            "gemini-4-argon"
        );
        // A provider that serves other vendors' weights keeps what it is given.
        assert_eq!(
            configured_model(spec("groq"), "openai/gpt-oss-120b"),
            "openai/gpt-oss-120b"
        );
        // A gateway is a gateway.
        assert_eq!(
            configured_model(spec("omniroute"), "gemini-4-argon"),
            "gemini-4-argon"
        );
        assert_eq!(
            configured_model(spec("openrouter"), "anthropic/claude-sonnet-4.5"),
            "anthropic/claude-sonnet-4.5"
        );
        // A direct vendor takes its own namespace.
        assert_eq!(configured_model(spec("xai"), "grok-4"), "grok-4");
        assert_eq!(
            configured_model(spec("openai"), "gpt-5.6-sol"),
            "gpt-5.6-sol"
        );
        // Vertex's namespace is the publisher-prefixed one, and it keeps its own ids.
        assert!(is_model(spec("vertex"), "google/gemini-4-argon"));
        assert!(!is_model(spec("vertex"), "openai/gpt-4.1-mini"));
        // Anthropic's direct ids are `claude-…`; the OpenRouter namespace for the same weights is
        // another door's id and is replaced rather than sent.
        assert!(is_model(spec("anthropic"), "claude-opus-5-5"));
        assert_eq!(
            configured_model(spec("anthropic"), "anthropic/claude-sonnet-4.5"),
            "claude-opus-5-5"
        );
        assert!(!is_model(spec("anthropic"), "gemini-4-argon"));
        // And the other way round: an Anthropic door does not take a `claude-` id from a gateway.
        assert_eq!(
            configured_model(spec("openrouter"), "claude-opus-5-5"),
            "claude-opus-5-5"
        );
        assert_eq!(
            configured_model(spec("vertex"), " google/gemini-4-argon "),
            "google/gemini-4-argon"
        );
        // Nothing saved, and no default: the turn must say so rather than ask for `""`.
        assert_eq!(configured_model(spec("groq"), ""), "");
        assert!(!is_model(spec("groq"), "   "));
    }

    #[tokio::test]
    async fn a_missing_key_names_the_provider_and_the_way_to_fix_it() {
        let store = MemoryCredentialStore::new();
        let error = build_error("xai", &store).await;
        assert!(error.contains("no key stored for xai"), "{error}");
        assert!(error.contains("XAI_API_KEY"), "{error}");
        assert!(error.contains("https://docs.x.ai"), "{error}");

        let error = build_error("not-a-provider", &store).await;
        assert!(error.contains("unknown provider"), "{error}");
    }

    #[tokio::test]
    async fn a_stored_key_builds_the_model_for_that_provider() {
        let store = MemoryCredentialStore::with(credentials::GROQ, "gsk-test");
        let model = build("groq", &store).await.unwrap();
        assert_eq!(model.id(), "groq");
    }

    /// The one row whose contract is not `/chat/completions` builds the Messages adapter.
    #[tokio::test]
    async fn the_anthropic_row_builds_the_messages_adapter() {
        let store = MemoryCredentialStore::with(credentials::ANTHROPIC, "sk-ant-test");
        let model = build("anthropic", &store).await.unwrap();
        assert_eq!(model.id(), "anthropic");
    }
}

//! Which provider door one turn actually goes through.
//!
//! Before this module the door was `Settings.provider` alone, and the model was whatever id the
//! caller happened to hold: a picker save that had not reached disk yet, an agent row written on
//! spawn, or a template could all send a model to the *previous* door. Vertex made that failure
//! look like a credential problem — its namespace is `google/gemini…`, which is also how
//! OpenRouter names Gemini, so a Gemini pick could be read as a Vertex request and answered with
//! `ACCESS_TOKEN_TYPE_UNSUPPORTED`.
//!
//! The rule is now one sentence: **the provider travels with the model, and a model id never
//! selects Vertex.** Vertex is reached only when the user selected it or named it with the model.
//! A model the named door cannot send is an error, not a quiet default, because a default is a
//! different model than the one that was picked.

use serde::{Deserialize, Serialize};

use super::aliases;
use super::providers::{self, Endpoint, ProviderSpec};

/// The door and the exact id the request will carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub provider: String,
    pub model: String,
}

/// Why the resolved pair won, as a value the window and the CLI can print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RouteReason {
    /// The caller named a provider with the model, so the pair is the picker's.
    Picker,
    /// Only a model was named and the saved door can send it.
    ModelOnly,
    /// A bare model named another door's namespace and was repaired to it.
    BareIdRepair,
    /// Nothing was named; the saved pair is the answer.
    Saved,
    /// A named preference (`fast`, `deep`, `cheap`) resolved to its concrete pair.
    Alias,
}

impl RouteReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Picker => "picker",
            Self::ModelOnly => "model-only",
            Self::BareIdRepair => "bare-id-repair",
            Self::Saved => "saved",
            Self::Alias => "alias",
        }
    }
}

/// The saved pair, the requested pair and the door that won, with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteExplanation {
    /// What Settings holds, exactly as this process read it.
    pub saved: Route,
    /// What the caller asked for, when it named anything.
    pub requested: Option<Route>,
    /// The pair the request will carry.
    pub resolved: Route,
    pub reason: RouteReason,
    /// One sentence naming what was asked for, what won and what Settings holds, when they
    /// differ. `None` means the request and the saved pair agree, so there is nothing to explain.
    pub mismatch: Option<String>,
}

/// Resolve one turn.
///
/// * `selected_provider` / `selected_model` — the saved Settings pair.
/// * `requested_provider` — the provider the renderer sent with the model, when it did. This is
///   the picker's other half, so a save that has not landed cannot send a model through the old
///   door.
/// * `requested_model` — the id the caller wants. `None` means "the saved model".
///
/// A bare model with no provider is repaired only through a door that unmistakably owns its
/// namespace, and never through Vertex. A conflict is refused with the model and door named.
pub fn resolve(
    selected_provider: &str,
    selected_model: &str,
    requested_provider: Option<&str>,
    requested_model: Option<&str>,
) -> Result<Route, String> {
    let requested_provider = requested_provider
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(str::to_ascii_lowercase);
    let requested_model = requested_model
        .map(str::trim)
        .filter(|model| !model.is_empty());

    // An alias names a preference, not a door. Only a lone alias resolves through it: a pair
    // that names a provider is the picker's pair, and a provider that cannot take the word is
    // still an error rather than a silent substitution. This runs before the saved door is
    // looked up, because an alias needs no saved door to be meaningful.
    if requested_provider.is_none()
        && let Some(alias) = requested_model.and_then(aliases::find)
    {
        let spec = providers::find(alias.provider)
            .ok_or_else(|| providers::unknown_provider(alias.provider))?;
        let model =
            providers::sendable(spec, alias.model).ok_or_else(|| mismatch(spec, alias.model))?;
        return Ok(Route {
            provider: spec.id.to_string(),
            model,
        });
    }

    let provider_id = requested_provider
        .clone()
        .unwrap_or_else(|| selected_provider.trim().to_ascii_lowercase());
    // An unknown requested provider is an error: falling back to the saved door here is exactly
    // how the window used to send a model somewhere the user did not choose.
    let spec =
        providers::find(&provider_id).ok_or_else(|| providers::unknown_provider(&provider_id))?;

    let (door, model) = match (requested_provider.is_some(), requested_model) {
        // The caller named a pair. The pair is the answer; a model this door cannot send is an
        // error rather than a default.
        (true, Some(model)) => (
            spec.id,
            providers::sendable(spec, model).ok_or_else(|| mismatch(spec, model))?,
        ),
        (true, None) => (spec.id, providers::configured_model(spec, selected_model)),
        // Only an id arrived. It may repair the *door* only when the selected door cannot take
        // it and the id's own namespace names a door that can — never Vertex. The repair has to
        // carry the owner's id too: keeping the selected spec here would send the repaired id
        // to the old door, which is the bug this module exists to remove.
        (false, Some(model)) => match providers::sendable(spec, model) {
            Some(model) => (spec.id, model),
            None => {
                let other = implied_door(model).ok_or_else(|| mismatch(spec, model))?;
                let model =
                    providers::sendable(other, model).ok_or_else(|| mismatch(other, model))?;
                (other.id, model)
            }
        },
        (false, None) => (spec.id, providers::configured_model(spec, selected_model)),
    };

    Ok(Route {
        provider: door.to_string(),
        model,
    })
}

/// [`resolve`], with the decision it made. The reason is read from the outcome rather than a
/// second copy of the rule: only the bare-id path can change the door, so a changed door is a
/// repair and everything else is the named pair or the saved one.
pub fn explain(
    selected_provider: &str,
    selected_model: &str,
    requested_provider: Option<&str>,
    requested_model: Option<&str>,
) -> Result<RouteExplanation, String> {
    let resolved = resolve(
        selected_provider,
        selected_model,
        requested_provider,
        requested_model,
    )?;
    let named_provider = requested_provider
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(str::to_ascii_lowercase);
    let named_model = requested_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string);
    let selected = selected_provider.trim().to_ascii_lowercase();
    let reason = match (&named_provider, &named_model) {
        (Some(_), _) => RouteReason::Picker,
        (None, Some(model)) if aliases::find(model).is_some() => RouteReason::Alias,
        (None, Some(_)) if resolved.provider != selected => RouteReason::BareIdRepair,
        (None, Some(_)) => RouteReason::ModelOnly,
        (None, None) => RouteReason::Saved,
    };
    let saved = Route {
        provider: selected.clone(),
        model: selected_model.trim().to_string(),
    };
    let mismatch = (resolved != saved).then(|| {
        let asked = match (&named_provider, &named_model) {
            (Some(provider), Some(model)) => format!("{provider} / {model}"),
            (Some(provider), None) => format!("a model on {provider}"),
            (None, Some(model)) => model.clone(),
            (None, None) => "nothing".to_string(),
        };
        format!(
            "asked for {asked}; resolved {} / {} ({}), while Settings holds {} / {}",
            resolved.provider,
            resolved.model,
            reason.as_str(),
            saved.provider,
            saved.model
        )
    });
    Ok(RouteExplanation {
        saved,
        // A named pair is what the caller asked for. A lone alias is named too: the word is
        // what the user typed, and the resolved pair is the answer, so the inspector shows
        // both instead of hiding the request.
        requested: match (&named_provider, &named_model) {
            (Some(provider), model) => Some(Route {
                provider: provider.clone(),
                model: model.clone().unwrap_or_default(),
            }),
            (None, Some(model)) if aliases::find(model).is_some() => Some(Route {
                provider: resolved.provider.clone(),
                model: model.clone(),
            }),
            _ => None,
        },
        resolved,
        reason,
        mismatch,
    })
}

/// The door a bare id unmistakably belongs to: a non-gateway, key-based door with a namespace of
/// its own. Vertex is excluded on purpose — `google/gemini…` is also OpenRouter's spelling, so a
/// model id must never be able to select it.
fn implied_door(model: &str) -> Option<&'static ProviderSpec> {
    let model = model.trim().to_ascii_lowercase();
    providers::all().iter().find(|spec| {
        !matches!(spec.endpoint, Endpoint::Vertex)
            && !spec.access_token
            && !spec.gateway
            && !spec.namespace.is_empty()
            && spec
                .namespace
                .iter()
                .any(|prefix| model.starts_with(prefix))
    })
}

fn mismatch(spec: &ProviderSpec, model: &str) -> String {
    format!(
        "`{model}` cannot be sent to {}; pick a model from that provider's own list, or pick the \
         model together with its provider",
        spec.name
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{self, MemoryCredentialStore};

    /// The two doors that produced the report: a credentialed Vertex row, broken, next to every
    /// other provider. Selecting anything else must never resolve to Vertex.
    fn broken_vertex() -> MemoryCredentialStore {
        // The shape that produced `ACCESS_TOKEN_TYPE_UNSUPPORTED`: an AI Studio key where a
        // Vertex OAuth token belongs.
        let store = MemoryCredentialStore::with(credentials::VERTEX, "AIzaSyBroken");
        store.insert(credentials::OPENROUTER, "sk-or-test");
        store.insert(credentials::GEMINI, "AIzaSyOther");
        store.insert(credentials::GROQ, "gsk-test");
        store
    }

    fn is_vertex(route: &Route) -> bool {
        providers::find(&route.provider)
            .map(|spec| matches!(spec.endpoint, Endpoint::Vertex))
            .unwrap_or_else(|| panic!("{} is a provider", route.provider))
    }

    #[test]
    fn a_broken_vertex_door_is_never_chosen_for_another_providers_model() {
        let store: MemoryCredentialStore = broken_vertex();
        // Present and credentialed is the premise; it must not matter to any other selection.
        assert!(credentials::present_in(&store, credentials::VERTEX));

        let picks = [
            ("openrouter", "google/gemini-3-flash:free"),
            ("openrouter", "openai/gpt-5-nano"),
            ("gemini", "gemini-4-argon"),
            ("groq", "openai/gpt-oss-120b"),
            ("openrouter", "anthropic/claude-sonnet-4.5"),
        ];
        for (provider, model) in picks {
            let route = resolve(
                "openrouter",
                "gpt-5.6-sol-medium",
                Some(provider),
                Some(model),
            )
            .unwrap_or_else(|error| panic!("{provider}/{model}: {error}"));
            assert_eq!(route.provider, provider, "{provider}/{model}");
            assert!(
                !is_vertex(&route),
                "selecting {model} on {provider} produced a Vertex request"
            );
        }
    }

    #[test]
    fn a_bare_model_never_repairs_itself_into_vertex() {
        // The saved door is Vertex, but the id is another door's. A bare id may repair the door
        // only through a non-gateway namespace owner; a vendor-prefixed OpenRouter id has none,
        // so the turn is refused instead of sent to Vertex.
        let error = resolve(
            "vertex",
            "google/gemini-4-argon",
            None,
            Some("openai/gpt-5-nano"),
        )
        .expect_err("an OpenRouter id is not Vertex's to send");
        assert!(error.contains("Vertex AI"), "{error}");

        // An id that does name another door repairs to that door...
        let route = resolve(
            "vertex",
            "google/gemini-4-argon",
            None,
            Some("deepseek-chat"),
        )
        .unwrap();
        assert_eq!(route.provider, "deepseek");
        assert_eq!(route.model, "deepseek-chat");

        // ...while a *selected* Vertex keeps its own bare id. The `google/` rewrite belongs to
        // the door the user chose; it is never an inference from the model name.
        let route = resolve(
            "vertex",
            "google/gemini-4-argon",
            None,
            Some("gemini-3.5-flash"),
        )
        .unwrap();
        assert_eq!(route.provider, "vertex");
        assert_eq!(route.model, "google/gemini-3.5-flash");
    }

    #[test]
    fn vertex_is_used_when_and_only_when_it_is_selected() {
        let route = resolve(
            "vertex",
            "google/gemini-4-argon",
            None,
            Some("gemini-3.5-flash"),
        )
        .unwrap();
        assert_eq!(route.provider, "vertex");
        assert_eq!(
            route.model, "google/gemini-3.5-flash",
            "the publisher prefix is the same model, not a substitution"
        );

        let route = resolve("openrouter", "x", Some("vertex"), Some("gemini-3.5-flash")).unwrap();
        assert_eq!(
            route.provider, "vertex",
            "an explicitly named Vertex is honoured"
        );
    }

    #[test]
    fn a_picker_pair_beats_a_stale_saved_pair() {
        // The picker saved openrouter, but the file still says vertex. The request carries the
        // picker's pair, so the turn uses it — this is the race that produced the report.
        let route = resolve(
            "vertex",
            "google/gemini-4-argon",
            Some("openrouter"),
            Some("openai/gpt-5-nano"),
        )
        .unwrap();
        assert_eq!(route.provider, "openrouter");
        assert_eq!(route.model, "openai/gpt-5-nano");
    }

    #[test]
    fn a_model_the_named_door_cannot_send_is_an_error_not_a_default() {
        let error = resolve("groq", "", Some("groq"), Some("gemini-4-argon"))
            .expect_err("Gemini's id is not Groq's to send");
        assert!(error.contains("Groq"), "{error}");
        assert!(error.contains("gemini-4-argon"), "{error}");

        let error = resolve("openrouter", "", Some("nope"), Some("openai/gpt-5-nano"))
            .expect_err("an unknown door is an error");
        assert!(error.contains("unknown provider"), "{error}");
    }

    #[test]
    fn a_gateway_keeps_the_ids_it_is_given() {
        let route = resolve("omniroute", "gpt-5.6-sol-medium", None, None).unwrap();
        assert_eq!(route.provider, "omniroute");
        assert_eq!(route.model, "gpt-5.6-sol-medium");

        let route = resolve("openrouter", "", None, Some("google/gemini-3-flash:free")).unwrap();
        assert_eq!(route.provider, "openrouter");
    }

    #[test]
    fn free_mode_and_swarm_pairs_survive_the_rule() {
        // Free mode writes the probed pair through Settings, and SubagentPolicy carries the same
        // model. Both are ordinary pairs here, so neither is rewritten.
        let route = resolve(
            "openrouter",
            "",
            Some("openrouter"),
            Some("baseten/mammoth-1:free"),
        )
        .unwrap();
        assert_eq!(route.provider, "openrouter");
        assert_eq!(route.model, "baseten/mammoth-1:free");

        let route = resolve("groq", "", Some("groq"), Some("openai/gpt-oss-120b")).unwrap();
        assert_eq!(route.provider, "groq");
        assert_eq!(route.model, "openai/gpt-oss-120b");
    }

    #[test]
    fn the_explanation_names_the_saved_requested_and_resolved_pairs() {
        let explanation = explain(
            "vertex",
            "google/gemini-4-argon",
            Some("openrouter"),
            Some("openai/gpt-5-nano"),
        )
        .unwrap();
        assert_eq!(explanation.saved.provider, "vertex");
        assert_eq!(explanation.saved.model, "google/gemini-4-argon");
        assert_eq!(
            explanation.requested.as_ref().unwrap().provider,
            "openrouter"
        );
        assert_eq!(
            explanation.requested.as_ref().unwrap().model,
            "openai/gpt-5-nano"
        );
        assert_eq!(explanation.resolved.provider, "openrouter");
        assert_eq!(explanation.reason, RouteReason::Picker);
    }

    #[test]
    fn a_repair_and_a_saved_turn_are_labelled_differently() {
        let repaired = explain(
            "vertex",
            "google/gemini-4-argon",
            None,
            Some("deepseek-chat"),
        )
        .unwrap();
        assert_eq!(repaired.reason, RouteReason::BareIdRepair);
        assert_eq!(repaired.resolved.provider, "deepseek");

        let saved = explain("groq", "openai/gpt-oss-120b", None, None).unwrap();
        assert_eq!(saved.reason, RouteReason::Saved);
        assert_eq!(saved.requested, None);

        let model_only = explain("groq", "llama-4", None, Some("openai/gpt-oss-120b")).unwrap();
        assert_eq!(model_only.reason, RouteReason::ModelOnly);
    }

    #[test]
    fn the_explanation_serialises_with_a_stable_reason_token() {
        let explanation = explain("groq", "llama-4", None, Some("deepseek-chat")).unwrap();
        let value = serde_json::to_value(&explanation).unwrap();
        assert_eq!(value["reason"], "bare-id-repair");
        assert_eq!(value["resolved"]["provider"], "deepseek");
    }
}

//! Roadmap 27 (local models) and 29 (aliases), through the resolvers the app calls.

use rigel_core::agent::routing::RouteReason;
use rigel_core::agent::{aliases, local, providers, routing};
use rigel_core::credentials::{self, MemoryCredentialStore};

#[test]
fn an_alias_resolves_to_the_concrete_pair_it_names() {
    let explanation = routing::explain("", "", None, Some("fast")).expect("fast is an alias");
    assert_eq!(explanation.reason, RouteReason::Alias);
    assert_eq!(
        explanation
            .requested
            .as_ref()
            .expect("the alias was named")
            .model,
        "fast"
    );
    assert_eq!(explanation.resolved.provider, credentials::OPENROUTER);
    assert_eq!(explanation.resolved.model, "openai/gpt-5-nano");
    let mismatch = explanation
        .mismatch
        .expect("the pair differs from Settings");
    assert!(mismatch.contains("fast"), "{mismatch}");
    assert!(mismatch.contains("openai/gpt-5-nano"), "{mismatch}");
}

#[test]
fn a_provider_named_with_the_word_alias_keeps_the_picker_pair() {
    let explanation = routing::explain("", "", Some(credentials::GROQ), Some("fast"))
        .expect("Groq takes the id as its own model");
    assert_eq!(explanation.reason, RouteReason::Picker);
    assert_eq!(explanation.resolved.provider, credentials::GROQ);
    assert_eq!(explanation.resolved.model, "fast");
}

#[test]
fn every_alias_names_a_pair_its_door_can_send() {
    for alias in aliases::ALIASES {
        let spec = providers::find(alias.provider)
            .unwrap_or_else(|| panic!("{} has no provider row", alias.name));
        assert_eq!(
            providers::sendable(spec, alias.model).as_deref(),
            Some(alias.model),
            "{} / {} cannot send its alias",
            alias.provider,
            alias.model
        );
        assert!(!alias.why.is_empty(), "{} has no explanation", alias.name);
    }
    assert!(aliases::find("FAST").is_some());
    assert!(aliases::find("not-an-alias").is_none());
}

#[tokio::test]
async fn the_local_doors_need_no_key_and_default_to_loopback() {
    let store = MemoryCredentialStore::new();
    for id in [credentials::LOCAL, credentials::LLAMAFILE] {
        let spec = providers::find(id).unwrap_or_else(|| panic!("{id} is not a provider row"));
        assert!(providers::keyless(id), "{id} must be keyless");
        assert_eq!(
            providers::credential(spec, &store).await.unwrap(),
            "",
            "{id} must not read a key it does not have"
        );
        assert!(
            local::is_loopback(&providers::base_url(spec).unwrap()),
            "{id} must default to this machine"
        );
        assert_eq!(
            providers::configured_model(spec, "llama3.2"),
            "llama3.2",
            "{id} takes the roster's own ids"
        );
    }
}

#[test]
fn the_picker_can_name_the_local_doors_without_a_credential() {
    for id in [credentials::LOCAL, credentials::LLAMAFILE] {
        assert!(
            credentials::PROVIDERS.contains(&id),
            "{id} must be visible to keys status"
        );
        assert!(
            credentials::provider_environment_variable(id).is_some(),
            "{id} must name the variable that moves it"
        );
    }
}

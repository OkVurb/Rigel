//! Item 18: what a picker model can do, with its source and date.
//!
//! The catalogue says what a provider *offers*; it does not say whether the model takes tools,
//! sees images, how wide its window is, or how long an answer may be. This registry answers
//! those questions for every row the picker holds. A fact this build does not know is `None`,
//! which the window renders as "unknown" — never `false`, because a guess that a model cannot
//! do something is as wrong as a guess that it can.
//!
//! Facts come from three places, in order: the row the provider just published (modality and
//! completion price), the local price table, and a small table of vendor-published limits. The
//! last is deliberately short: only models whose figures are stable enough to repeat are listed,
//! and everything else stays unknown until a row is added.

use serde::{Deserialize, Serialize};

use super::pricing;

/// The day the vendor limits below were read from the providers' model pages.
pub const AS_OF: &str = "2026-11-05";

/// Tools, context and maximum output for models Rigel sends tool schemas to. Everything not
/// listed stays unknown rather than being inferred from the model's name.
const KNOWN: &[(&str, u64, u64)] = &[
    ("anthropic/claude-sonnet-4.5", 200_000, 64_000),
    ("anthropic/claude-haiku-4.5", 200_000, 64_000),
    ("openai/gpt-5.4-mini", 400_000, 128_000),
    ("openai/gpt-5-nano", 400_000, 128_000),
    ("openai/gpt-5.4-nano", 400_000, 128_000),
    ("google/gemini-2.5-flash-lite", 1_000_000, 65_536),
    ("google/gemini-3-flash-preview", 1_000_000, 65_536),
    ("google/gemini-4-argon", 1_000_000, 65_536),
];

/// What the picker knows about one row. Every field is optional because a missing fact is a
/// real answer, not a default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRow {
    pub provider: String,
    pub model: String,
    pub tools: Option<bool>,
    pub vision: Option<bool>,
    pub context_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub streaming: Option<bool>,
    pub output_micros_per_mtok: Option<u64>,
    /// Where the row's facts came from, shown beside them.
    pub source: String,
    /// The day those facts were read.
    pub as_of: String,
}

/// What the window sends for one picker row. It is exactly the catalogue row's public facts, so
/// the registry never has to guess at a model it was not shown.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub modality: Option<String>,
    #[serde(default)]
    pub output_micros_per_mtok: Option<u64>,
    #[serde(default)]
    pub chat: Option<bool>,
}

pub fn describe(input: &ModelRef) -> CapabilityRow {
    let known = KNOWN.iter().find(|(model, _, _)| *model == input.model);
    let vision = input
        .modality
        .as_deref()
        .map(|modality| modality.to_ascii_lowercase().contains("image"));
    // Every chat adapter in this tree streams, including the ones with no model list. A row the
    // provider marks as not a chat model has no chat stream to describe.
    let streaming = if input.chat == Some(false) {
        None
    } else {
        Some(true)
    };
    let price = input.output_micros_per_mtok.or_else(|| {
        pricing::is_listed(&input.model)
            .then(|| (pricing::price_for(&input.model).output * 1_000_000.0).round() as u64)
    });
    let from_catalog = input.modality.is_some() || input.output_micros_per_mtok.is_some();
    CapabilityRow {
        provider: input.provider.clone(),
        model: input.model.clone(),
        tools: known.map(|_| true),
        vision,
        context_tokens: known.map(|(_, context, _)| *context),
        max_output_tokens: known.map(|(_, _, max)| *max),
        streaming,
        output_micros_per_mtok: price,
        source: if known.is_some() {
            "provider model pages".to_string()
        } else if from_catalog {
            "catalog row; tools and limits unpublished in this build".to_string()
        } else {
            "no published capability row in this build".to_string()
        },
        as_of: AS_OF.to_string(),
    }
}

/// One row for every picker row, in the order given. Unknown models are described too; the
/// caller never has to special-case a model the registry has not seen.
pub fn for_models(models: Vec<ModelRef>) -> Vec<CapabilityRow> {
    models.iter().map(describe).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(provider: &str, model: &str) -> ModelRef {
        ModelRef {
            provider: provider.to_string(),
            model: model.to_string(),
            modality: None,
            output_micros_per_mtok: None,
            chat: None,
        }
    }

    #[test]
    fn a_known_model_records_its_limits_and_price() {
        let described = describe(&row("anthropic", "anthropic/claude-sonnet-4.5"));
        assert_eq!(described.tools, Some(true));
        assert_eq!(described.context_tokens, Some(200_000));
        assert_eq!(described.max_output_tokens, Some(64_000));
        assert_eq!(described.streaming, Some(true));
        assert_eq!(
            described.output_micros_per_mtok,
            Some(15_000_000),
            "the local table prices it when the catalogue did not"
        );
        assert_eq!(described.source, "provider model pages");
        assert_eq!(described.as_of, AS_OF);
    }

    #[test]
    fn an_unknown_model_is_unknown_never_false() {
        let described = describe(&row("openrouter", "someone/new-model"));
        assert_eq!(described.tools, None);
        assert_eq!(described.vision, None);
        assert_eq!(described.context_tokens, None);
        assert_eq!(described.max_output_tokens, None);
        assert_eq!(described.output_micros_per_mtok, None);
        assert_eq!(described.streaming, Some(true), "a chat turn streams");

        let json = serde_json::to_value(&described).unwrap();
        assert!(
            json["tools"].is_null(),
            "missing is null, not false: {json}"
        );
        assert!(json["vision"].is_null(), "{json}");
        assert!(json["context_tokens"].is_null(), "{json}");
    }

    #[test]
    fn every_picker_row_gets_a_row_in_order() {
        let rows = for_models(vec![
            row("openrouter", "openai/gpt-5.4-mini"),
            row("openrouter", "unknown/model"),
            row("omniroute", "gpt-5.6-sol-medium"),
        ]);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].model, "openai/gpt-5.4-mini");
        assert_eq!(rows[1].model, "unknown/model");
        assert_eq!(rows[2].model, "gpt-5.6-sol-medium");
        assert_eq!(rows[1].tools, None);
        assert_eq!(rows[2].tools, None);
    }

    #[test]
    fn vision_comes_from_the_catalog_modality() {
        let mut multimodal = row("openrouter", "google/gemini-3-flash-preview");
        multimodal.modality = Some("text+image->text+image".to_string());
        assert_eq!(describe(&multimodal).vision, Some(true));

        let mut text_only = row("openrouter", "openai/gpt-4.1");
        text_only.modality = Some("text->text".to_string());
        assert_eq!(describe(&text_only).vision, Some(false));
    }

    #[test]
    fn a_non_chat_row_has_no_stream_to_promise() {
        let mut image = row("omniroute", "aihorde/AbyssOrangeMix");
        image.chat = Some(false);
        assert_eq!(describe(&image).streaming, None);
    }
}

//! Preflight context estimation (roadmap 51).
//!
//! The provider is the authority on tokenisation, and it only answers after the request. This
//! module gives a deliberately conservative local estimate before sending: a provider-specific
//! characters-per-token ratio, a 15% margin, and a low bound. Only a request whose *low* bound is
//! already larger than the window is refused; an estimate near the edge is sent and left to the
//! provider, because refusing a valid turn is worse than one provider error.

use serde::Serialize;

use super::model::{ChatRequest, ToolSpec};

/// 15% is the margin advertised in the estimate row. It is applied symmetrically so the low and
/// high bounds are both visible.
pub const MARGIN_PCT: usize = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ContextEstimate {
    pub tokens: usize,
    /// The low bound: `tokens` minus the margin.
    pub low: usize,
    /// The high bound: `tokens` plus the margin.
    pub high: usize,
    pub window: usize,
    pub margin_pct: usize,
    pub method: &'static str,
    pub provider: &'static str,
    /// The high bound fits. A false value is advisory; see [`Self::guaranteed_overflow`].
    pub fits: bool,
}

impl ContextEstimate {
    /// The low bound is already past the window: no plausible tokenisation can fit.
    pub fn guaranteed_overflow(self) -> bool {
        self.low > self.window
    }
}

pub fn estimate_request(request: &ChatRequest) -> ContextEstimate {
    let mut chars = 0usize;
    for message in &request.messages {
        if let Some(content) = &message.content {
            chars += content.chars().count();
        }
        // Role, separators and message framing cost a few tokens each. Four is the documented
        // constant in the row's method, not a claim that every provider uses exactly four.
        chars += 4;
        if let Some(calls) = &message.tool_calls {
            for call in calls {
                chars += call.name.chars().count() + call.arguments.chars().count() + 8;
            }
        }
    }
    for tool in &request.tools {
        chars += tool_schema_chars(tool);
    }
    estimate_chars(chars, &request.model)
}

pub fn tool_schema_chars(tool: &ToolSpec) -> usize {
    serde_json::to_string(&tool.openai())
        .map(|json| json.chars().count())
        .unwrap_or_else(|_| tool.name.len() + tool.description.len())
}

pub fn estimate_chars(chars: usize, model: &str) -> ContextEstimate {
    let provider = provider_of(model);
    let ratio_x10 = ratio_x10(provider);
    let tokens = chars.saturating_mul(10).div_ceil(ratio_x10);
    let low = tokens.saturating_mul(100 - MARGIN_PCT) / 100;
    let high = tokens.saturating_mul(100 + MARGIN_PCT) / 100;
    let window = window_for(provider, model);
    ContextEstimate {
        tokens,
        low,
        high,
        window,
        margin_pct: MARGIN_PCT,
        method: match provider {
            "anthropic" => "chars/3.5 + 4 per message; low/high bound at 15%",
            _ => "chars/4 + 4 per message; low/high bound at 15%",
        },
        provider,
        fits: high <= window,
    }
}

pub fn provider_of(model: &str) -> &'static str {
    let lower = model.to_ascii_lowercase();
    if lower.contains("claude") || lower.contains("anthropic") {
        "anthropic"
    } else if lower.contains("gemini") || lower.contains("vertex") {
        "gemini"
    } else if lower.contains("grok") || lower.contains("xai") {
        "xai"
    } else if lower.contains("gpt") || lower.contains("openai") || lower.contains("o1") {
        "openai"
    } else {
        "other"
    }
}

fn ratio_x10(provider: &str) -> usize {
    match provider {
        "anthropic" => 35,
        _ => 40,
    }
}

fn window_for(provider: &str, model: &str) -> usize {
    let lower = model.to_ascii_lowercase();
    match provider {
        "anthropic" => 200_000,
        "gemini" => 1_000_000,
        "xai" => 131_072,
        // Known long-context OpenAI ids. The provider still answers the final word.
        "openai" if lower.contains("gpt-4.1") || lower.contains("gpt-5") => 400_000,
        "openai" => 128_000,
        _ => 128_000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::model::ChatMessage;

    #[test]
    fn the_estimate_is_provider_specific_and_has_bounds() {
        let chars = 40_000;
        let openai = estimate_chars(chars, "openai/gpt-5-mini");
        let claude = estimate_chars(chars, "anthropic/claude-sonnet-4.5");
        assert_eq!(openai.tokens, 10_000);
        assert!(
            claude.tokens > openai.tokens,
            "Claude packs fewer chars/token"
        );
        assert!(openai.low < openai.tokens && openai.high > openai.tokens);
        assert_eq!(openai.margin_pct, 15);
        assert!(openai.fits);
    }

    #[test]
    fn only_the_low_bound_past_the_window_is_a_guaranteed_overflow() {
        let window = estimate_chars(10, "openai/gpt-5-mini").window;
        let near = estimate_chars(window * 4, "openai/gpt-5-mini");
        assert!(!near.guaranteed_overflow(), "a near-edge request is sent");

        let past = estimate_chars(window * 5, "openai/gpt-5-mini");
        assert!(past.guaranteed_overflow());
        assert!(!past.fits);
    }

    #[test]
    fn request_estimation_counts_messages_and_schemas() {
        let request = ChatRequest {
            model: "openai/gpt-5-mini".to_string(),
            messages: vec![
                ChatMessage::system("x".repeat(400)),
                ChatMessage::user("hello"),
            ],
            tools: vec![ToolSpec {
                name: "read_file".to_string(),
                description: "read".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }],
            max_tokens: Some(100),
            temperature: 0.2,
            effort: None,
        };
        let estimate = estimate_request(&request);
        assert!(estimate.tokens > 100);
        assert!(estimate.fits);
    }
}

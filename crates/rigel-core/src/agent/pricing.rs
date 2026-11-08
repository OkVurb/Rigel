//! Local price fallback (§7.5).
//!
//! Used **only** when the provider does not return `usage.cost`. The result is marked
//! `CostSource::Estimated` in the usage row, so reconcile and the UI can tell a bill from an
//! estimate. Prices are dollars per million tokens, which is numerically the same as
//! micro-dollars per token — the unit the budget service accounts in.
//!
//! This table is a floor, not a promise: models are added as they are used, and a model
//! missing from it is priced at the conservative default rather than at zero.

use serde::Serialize;

use super::model::RawUsage;

/// The day the rates below were last read from the vendors' own price pages.
pub const FETCHED_ON: &str = "2026-11-05";
/// Where the rates came from. Shown with the age when the table is old.
pub const SOURCE: &str =
    "vendor price pages (OpenAI, Anthropic, Google, DeepSeek, Meta, Fireworks)";
/// Prices move rarely; a quarter is long enough for a stale row to be a real risk and short
/// enough that the warning still means something.
pub const STALE_AFTER_DAYS: i64 = 90;

/// How old the table is, and whether that is old enough to warn about. The cost surface shows
/// this beside the estimate so a stale number is never silently accepted as current.
#[derive(Debug, Clone, Serialize)]
pub struct Freshness {
    pub fetched_on: String,
    pub age_days: i64,
    pub source: String,
    pub stale: bool,
}

pub fn freshness(now_ms: i64) -> Freshness {
    let fetched = days_from_civil(2026, 11, 5);
    let age_days = (now_ms.div_euclid(86_400_000) - fetched).max(0);
    Freshness {
        fetched_on: FETCHED_ON.to_string(),
        age_days,
        source: SOURCE.to_string(),
        stale: age_days > STALE_AFTER_DAYS,
    }
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = (month + 9) % 12;
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

pub struct Price {
    /// Dollars per million input tokens.
    pub input: f64,
    /// Dollars per million cached input tokens.
    pub cached_input: f64,
    /// Dollars per million output tokens.
    pub output: f64,
}

/// Unknown models are priced like a mid-range model. A wrong estimate is visible as
/// `estimated`; a zero would look free, which is worse.
const DEFAULT: Price = Price {
    input: 1.0,
    cached_input: 0.1,
    output: 3.0,
};

/// Every model `SUBAGENT_MODELS` offers must appear here: a parent can name it, so the bill
/// must be able to price it without falling back to [`DEFAULT`].
const TABLE: &[(&str, Price)] = &[
    (
        // The default subagent model: the cheapest OpenAI entry in the allowlist, and enough
        // instruction-following to return a clean answer. Qwen3 Coder is cheaper on output.
        "openai/gpt-5-nano",
        Price {
            input: 0.05,
            cached_input: 0.005,
            output: 0.40,
        },
    ),
    (
        "openai/gpt-5.4-nano",
        Price {
            input: 0.20,
            cached_input: 0.02,
            output: 1.25,
        },
    ),
    (
        "google/gemini-2.5-flash-lite",
        Price {
            input: 0.10,
            cached_input: 0.01,
            output: 0.40,
        },
    ),
    (
        // The Gemini API names the same model without the OpenRouter vendor prefix.
        "gemini-2.5-flash-lite",
        Price {
            input: 0.10,
            cached_input: 0.01,
            output: 0.40,
        },
    ),
    (
        // Cached reads are free on this model, per the provider's listed rate.
        "deepseek/deepseek-v4.1-flash",
        Price {
            input: 0.024,
            cached_input: 0.0,
            output: 0.375,
        },
    ),
    (
        // No cached-input rate is published; charging it as an uncached read is conservative.
        "qwen/qwen3-coder-30b-a3b-instruct",
        Price {
            input: 0.07,
            cached_input: 0.07,
            output: 0.28,
        },
    ),
    (
        "anthropic/claude-haiku-5.5",
        Price {
            input: 0.10,
            cached_input: 0.01,
            output: 0.50,
        },
    ),
    (
        "google/gemini-3-flash-preview",
        Price {
            input: 0.50,
            cached_input: 0.05,
            output: 3.00,
        },
    ),
    (
        "openai/gpt-5.4-mini",
        Price {
            input: 0.75,
            cached_input: 0.075,
            output: 4.50,
        },
    ),
    (
        // Older models stay priced: a session record can still name one.
        "openai/gpt-4.1-nano",
        Price {
            input: 0.10,
            cached_input: 0.025,
            output: 0.40,
        },
    ),
    (
        "openai/gpt-4.1-mini",
        Price {
            input: 0.40,
            cached_input: 0.10,
            output: 1.60,
        },
    ),
    (
        "openai/gpt-4.1",
        Price {
            input: 2.00,
            cached_input: 0.50,
            output: 8.00,
        },
    ),
    (
        "openai/gpt-4o-mini",
        Price {
            input: 0.15,
            cached_input: 0.075,
            output: 0.60,
        },
    ),
    (
        "anthropic/claude-sonnet-4.5",
        Price {
            input: 3.00,
            cached_input: 0.30,
            output: 15.00,
        },
    ),
    (
        "anthropic/claude-haiku-4.5",
        Price {
            input: 1.00,
            cached_input: 0.10,
            output: 5.00,
        },
    ),
    (
        "google/gemini-2.5-flash",
        Price {
            input: 0.30,
            cached_input: 0.03,
            output: 2.50,
        },
    ),
    (
        // Gemini's direct API id, not the OpenRouter namespace. Introductory rate; cached
        // input is the published 95% discount. Raise this row if the promo price ends.
        "gemini-4-argon",
        Price {
            input: 2.00,
            cached_input: 0.10,
            output: 10.00,
        },
    ),
    (
        // Anthropic's own endpoint names the same models without a vendor prefix, and the
        // Messages adapter sends ids in that namespace. Cache reads are the published 0.1× of
        // input. Models the picker lists but this table does not are priced at the conservative
        // fallback until a row is added.
        "claude-opus-5-5",
        Price {
            input: 4.00,
            cached_input: 0.40,
            output: 20.00,
        },
    ),
    (
        // Meta Model API standard rate for Muse Spark. Contributor mode is the same model at a
        // training-data discount; its cached-input rate is not published, so it is priced at
        // input rather than guessed lower.
        "muse-spark-1.3",
        Price {
            input: 1.25,
            cached_input: 0.15,
            output: 4.25,
        },
    ),
    (
        "muse-spark-1.3-contributor",
        Price {
            input: 0.10,
            cached_input: 0.10,
            output: 0.20,
        },
    ),
    (
        "deepseek/deepseek-chat",
        Price {
            input: 0.27,
            cached_input: 0.07,
            output: 1.10,
        },
    ),
    // Fireworks' OpenAI-compatible endpoint accepts both the short id shown in its docs and the
    // resource name its model roster returns. Keeping both rows means a direct call and a
    // FireRouter turn that reports the served model are priced alike.
    (
        "glm-5p3-flash",
        Price {
            input: 0.15,
            cached_input: 0.03,
            output: 0.50,
        },
    ),
    (
        "accounts/fireworks/models/glm-5p3-flash",
        Price {
            input: 0.15,
            cached_input: 0.03,
            output: 0.50,
        },
    ),
    (
        "glm-5p3",
        Price {
            input: 1.40,
            cached_input: 0.26,
            output: 4.40,
        },
    ),
    (
        "accounts/fireworks/models/glm-5p3",
        Price {
            input: 1.40,
            cached_input: 0.26,
            output: 4.40,
        },
    ),
    (
        "deepseek-v4p1-flash",
        Price {
            input: 0.30,
            cached_input: 0.006,
            output: 1.20,
        },
    ),
    (
        "accounts/fireworks/models/deepseek-v4p1-flash",
        Price {
            input: 0.30,
            cached_input: 0.006,
            output: 1.20,
        },
    ),
    (
        "kimi-k3p1-pro",
        Price {
            input: 2.00,
            cached_input: 0.20,
            output: 10.00,
        },
    ),
    (
        "accounts/fireworks/models/kimi-k3p1-pro",
        Price {
            input: 2.00,
            cached_input: 0.20,
            output: 10.00,
        },
    ),
    (
        "qwen3p8-max",
        Price {
            input: 2.00,
            cached_input: 0.25,
            output: 6.00,
        },
    ),
    (
        "accounts/fireworks/models/qwen3p8-max",
        Price {
            input: 2.00,
            cached_input: 0.25,
            output: 6.00,
        },
    ),
    (
        "gpt-oss-120b",
        Price {
            input: 0.15,
            cached_input: 0.015,
            output: 0.60,
        },
    ),
    (
        "accounts/fireworks/models/gpt-oss-120b",
        Price {
            input: 0.15,
            cached_input: 0.015,
            output: 0.60,
        },
    ),
];

pub fn price_for(model: &str) -> &'static Price {
    TABLE
        .iter()
        .find(|(id, _)| *id == model)
        .map(|(_, price)| price)
        .unwrap_or(&DEFAULT)
}

/// Whether this model has its own row rather than the conservative fallback.
pub fn is_listed(model: &str) -> bool {
    TABLE.iter().any(|(id, _)| *id == model)
}

/// Estimated cost in micro-dollars: `tokens × dollars-per-million-token`.
pub fn estimate(model: &str, usage: &RawUsage) -> i64 {
    cost_micros(
        model,
        usage.input_tokens,
        usage.cached_input_tokens,
        usage.output_tokens + usage.reasoning_tokens,
    )
}

/// Cost of a token count at this model's prices, in micro-dollars.
///
/// Exposed separately from [`estimate`] so a request that has not happened yet can be
/// priced: the pre-turn quote knows tokens, not usage.
pub fn cost_micros(
    model: &str,
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
) -> i64 {
    let price = price_for(model);
    let uncached = input_tokens.saturating_sub(cached_input_tokens) as f64;
    let cached = cached_input_tokens as f64;
    let output = output_tokens as f64;
    let micros = uncached * price.input + cached * price.cached_input + output * price.output;
    micros.round() as i64
}

/// Four characters per token, rounded up. A heuristic, not a tokenizer: it exists only so a
/// turn can be quoted before its first request, and the provider's real usage replaces it as
/// each step lands.
pub fn tokens_for(chars: usize) -> u64 {
    (chars as u64).div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_older_than_the_release_names_its_age_and_source() {
        let fresh = freshness(days_from_civil(2026, 11, 6) * 86_400_000);
        assert_eq!(fresh.age_days, 1, "one day after the fetch date");
        assert!(!fresh.stale, "a day-old table is current");
        assert_eq!(fresh.fetched_on, FETCHED_ON);

        let old = freshness(days_from_civil(2027, 3, 1) * 86_400_000);
        assert!(old.stale, "a table past the margin must warn");
        assert!(old.age_days > STALE_AFTER_DAYS, "{}", old.age_days);
        assert_eq!(
            old.source, SOURCE,
            "the warning names where the row came from"
        );
    }

    #[test]
    fn a_known_model_prices_its_own_tokens() {
        let usage = RawUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..RawUsage::default()
        };
        // $0.75 + $4.50 = $5.25 = 5,250,000 micro-dollars.
        assert_eq!(estimate("openai/gpt-5.4-mini", &usage), 5_250_000);
    }

    #[test]
    fn cached_input_is_cheaper_and_counted_once() {
        let usage = RawUsage {
            input_tokens: 1_000_000,
            cached_input_tokens: 1_000_000,
            ..RawUsage::default()
        };
        assert_eq!(estimate("openai/gpt-5.4-mini", &usage), 75_000);
    }

    #[test]
    fn fireworks_short_ids_and_resource_names_are_priced() {
        for (short, full, input, cached, output) in [
            (
                "glm-5p3-flash",
                "accounts/fireworks/models/glm-5p3-flash",
                0.15,
                0.03,
                0.50,
            ),
            (
                "deepseek-v4p1-flash",
                "accounts/fireworks/models/deepseek-v4p1-flash",
                0.30,
                0.006,
                1.20,
            ),
            (
                "kimi-k3p1-pro",
                "accounts/fireworks/models/kimi-k3p1-pro",
                2.00,
                0.20,
                10.00,
            ),
        ] {
            assert!(is_listed(short), "{short} has no Fireworks row");
            assert!(is_listed(full), "{full} has no Fireworks row");
            assert_eq!(price_for(short).input, input);
            assert_eq!(price_for(full).cached_input, cached);
            assert_eq!(price_for(full).output, output);
        }

        // 200k uncached + 800k cached input + 200k output for GLM 5.3 Flash:
        // $0.03 + $0.024 + $0.10 = $0.154.
        assert_eq!(
            cost_micros(
                "accounts/fireworks/models/glm-5p3-flash",
                1_000_000,
                800_000,
                200_000
            ),
            154_000
        );
    }

    #[test]
    fn the_anthropic_default_is_priced_by_its_own_row() {
        // It is the model a turn runs when nothing has been chosen, so it must never be billed
        // at the fallback.
        assert!(is_listed(crate::agent::anthropic::DEFAULT_MODEL));
    }

    #[test]
    fn an_unknown_model_is_not_free() {
        let usage = RawUsage {
            input_tokens: 1000,
            ..RawUsage::default()
        };
        assert!(estimate("some/new-model", &usage) > 0);
    }

    #[test]
    fn a_quote_prices_tokens_it_has_not_sent() {
        // Sonnet 4.5: $3/M in + $15/M out. 10k in + 1k out = $0.03 + $0.015.
        assert_eq!(
            cost_micros("anthropic/claude-sonnet-4.5", 10_000, 0, 1_000),
            45_000
        );
        assert_eq!(tokens_for(0), 0);
        assert_eq!(tokens_for(1), 1);
        assert_eq!(tokens_for(4), 1);
        assert_eq!(tokens_for(5), 2);
    }
}

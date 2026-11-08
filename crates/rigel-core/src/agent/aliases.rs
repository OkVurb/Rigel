//! Model aliases (roadmap 29).
//!
//! `fast`, `deep` and `cheap` are preferences, not model ids. The resolver turns one into the
//! concrete provider/model pair here, so a turn, the transcript and `rigel route explain` all
//! carry the exact pair that was sent instead of the word the user typed. Keeping the table in
//! one place also means an alias can never mean two things in two call sites.

use crate::credentials;

pub struct Alias {
    pub name: &'static str,
    pub provider: &'static str,
    pub model: &'static str,
    /// One line for the explanation, so a preference is auditable.
    pub why: &'static str,
}

pub const ALIASES: &[Alias] = &[
    Alias {
        name: "fast",
        provider: credentials::OPENROUTER,
        model: "openai/gpt-5-nano",
        why: "the cheapest fast tier on the saved gateway",
    },
    Alias {
        name: "deep",
        provider: credentials::OPENROUTER,
        model: "anthropic/claude-sonnet-4.5",
        why: "the strongest general model on the same gateway",
    },
    Alias {
        name: "cheap",
        provider: credentials::OPENROUTER,
        model: "google/gemini-2.5-flash-lite",
        why: "the lowest published price on the same gateway",
    },
];

/// The alias with this name, case-insensitively. An absent name is an ordinary model id.
pub fn find(name: &str) -> Option<&'static Alias> {
    let name = name.trim();
    ALIASES
        .iter()
        .find(|alias| alias.name.eq_ignore_ascii_case(name))
}

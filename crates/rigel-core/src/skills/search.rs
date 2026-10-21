//! Search over the local index, and the ordering contract.
//!
//! **The tiebreak is documented and stable**: safety grade first (A→F, unscanned last), then
//! stars, then recency, then name. FTS5's own `bm25` rank decides *membership* — whether a
//! row matches at all — but not order, because a keyword-stuffed description should not be
//! able to outrank a reviewed skill. This is P2.4's "documented tiebreak (grade → stars →
//! recency)"; changing it means changing this comment and the test that pins the order.
//!
//! Queries are never string-built into SQL. The user's words become FTS5 *quoted prefix
//! tokens* (`"pdf"*`) and are bound as one parameter, so punctuation, `*`, `NEAR` and column
//! filters in the input are treated as text rather than syntax.

use serde::{Deserialize, Serialize};

/// Kept as a type rather than a constant so the intent survives refactors: there is exactly
/// one supported order in Phase 2, and it is part of the app's contract, not an implementation
/// detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchOrder {
    /// grade → stars → recency → name.
    GradeStarsRecency,
}

impl SearchOrder {
    pub const DEFAULT: SearchOrder = SearchOrder::GradeStarsRecency;

    /// The SQL fragment. `s` is the `skills` alias.
    pub fn order_by(self) -> &'static str {
        match self {
            SearchOrder::GradeStarsRecency => {
                "CASE s.grade
                   WHEN 'A' THEN 0 WHEN 'B' THEN 1 WHEN 'C' THEN 2
                   WHEN 'D' THEN 3 WHEN 'F' THEN 4 ELSE 5 END ASC,
                 COALESCE(s.stars, -1) DESC,
                 COALESCE(s.last_seen_at, 0) DESC,
                 s.name COLLATE NOCASE ASC,
                 s.id ASC"
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub repo: Option<String>,
    pub path: Option<String>,
    pub url: Option<String>,
    /// `None` means **not scanned**, and the shelf says so rather than guessing a letter.
    pub grade: Option<String>,
    pub grade_scope: Option<String>,
    pub stars: Option<i64>,
    pub installs: Option<i64>,
    pub tokens: Option<i64>,
    pub cluster_id: Option<String>,
    /// How many rows are in this row's cluster, including itself. Never used to hide a row.
    pub variants: i64,
    pub last_seen_at: Option<i64>,
    /// True when the description advertises a number nobody has measured (P2.7).
    pub unmeasured: bool,
    pub has_body: bool,
}

/// Turn free text into an FTS5 MATCH expression, or `None` when there is nothing to match.
///
/// Every token becomes a quoted prefix (`"wor"*`) so typing "wor" finds "workflow"; tokens
/// are ANDed, which is FTS5's default between adjacent strings.
pub fn fts_query(raw: &str) -> Option<String> {
    // Punctuation *separates* tokens rather than being deleted, so `name:pdf` searches for
    // "name" and "pdf" instead of the nonsense token "namepdf".
    let tokens: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"*", token.to_lowercase()))
        .collect();

    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_become_quoted_prefix_tokens() {
        assert_eq!(fts_query("pdf ocr").as_deref(), Some("\"pdf\"* \"ocr\"*"));
        assert_eq!(fts_query("  PDF ").as_deref(), Some("\"pdf\"*"));
    }

    #[test]
    fn fts_syntax_in_the_input_is_neutralised() {
        assert_eq!(
            fts_query("name:pdf OR *").as_deref(),
            Some("\"name\"* \"pdf\"* \"or\"*")
        );
        assert_eq!(
            fts_query("\"quoted\" (x)").as_deref(),
            Some("\"quoted\"* \"x\"*")
        );
    }

    #[test]
    fn nothing_meaningful_means_no_match_expression() {
        assert_eq!(fts_query("   "), None);
        assert_eq!(fts_query("***"), None);
    }

    #[test]
    fn the_order_is_grade_then_stars_then_recency() {
        let order = SearchOrder::DEFAULT.order_by();
        let grade = order.find("s.grade").unwrap();
        let stars = order.find("s.stars").unwrap();
        let recency = order.find("s.last_seen_at").unwrap();
        assert!(
            grade < stars && stars < recency,
            "ordering drifted: {order}"
        );
    }
}

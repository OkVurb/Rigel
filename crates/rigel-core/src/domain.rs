//! Types shared by the agent loop, the budget service and the UI.
//!
//! Money is in **micro-dollars** (1 µ$ = $0.000001), not cents. A provider reports
//! `usage.cost` as a float in USD, and a single cheap turn is routinely below one cent;
//! storing cents per turn would lose the bill. `spend_cents` in SQLite is the rounded,
//! display-only projection of `spend_micros` (see `migrations/project/0002_phase3.sql`).

use serde::{Deserialize, Serialize};

use crate::session::Item;

/// 100 cents = 1 dollar = 1,000,000 micro-dollars.
pub const MICROS_PER_CENT: i64 = 10_000;

/// Where a turn's cost figure came from. The distinction is the whole point of the
/// reconcile check: a locally estimated price is not a bill.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    /// The provider returned `usage.cost`, or a price the provider is known to charge.
    Provider,
    /// A local price table was used because the response carried no cost.
    #[default]
    Estimated,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub cached_input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    /// The authoritative cost for this turn, in micro-dollars.
    #[serde(default)]
    pub cost_micros: i64,
    /// What the provider said, when it said anything at all.
    #[serde(default)]
    pub provider_micros: Option<i64>,
    #[serde(default)]
    pub source: CostSource,
}

impl Usage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.reasoning_tokens
    }

    pub fn cents(&self) -> i64 {
        micros_to_cents(self.cost_micros)
    }

    pub fn add(&mut self, other: &Usage) {
        self.input_tokens += other.input_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.output_tokens += other.output_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.cost_micros += other.cost_micros;
        self.provider_micros = match (self.provider_micros, other.provider_micros) {
            (Some(a), Some(b)) => Some(a + b),
            (a, b) => a.or(b),
        };
        // A total is only as honest as its weakest part.
        if other.source == CostSource::Estimated {
            self.source = CostSource::Estimated;
        } else if self.source == CostSource::Estimated && other.cost_micros > 0 {
            self.source = CostSource::Provider;
        }
    }
}

/// Round to the nearest cent for display and for the `spend_cents` column.
pub fn micros_to_cents(micros: i64) -> i64 {
    (micros + MICROS_PER_CENT / 2).div_euclid(MICROS_PER_CENT)
}

pub fn cents_to_micros(cents: i64) -> i64 {
    cents.saturating_mul(MICROS_PER_CENT)
}

/// The ten item kinds from §5.2. Anything an adapter cannot map becomes `Error` with
/// `body.unmapped = true`, so it is visible rather than dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Message,
    Reasoning,
    Command,
    FileChange,
    McpCall,
    WebSearch,
    Plan,
    Approval,
    Usage,
    Error,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Reasoning => "reasoning",
            Self::Command => "command",
            Self::FileChange => "file_change",
            Self::McpCall => "mcp_call",
            Self::WebSearch => "web_search",
            Self::Plan => "plan",
            Self::Approval => "approval",
            Self::Usage => "usage",
            Self::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "message" => Self::Message,
            "reasoning" => Self::Reasoning,
            "command" => Self::Command,
            "file_change" => Self::FileChange,
            "mcp_call" => Self::McpCall,
            "web_search" => Self::WebSearch,
            "plan" => Self::Plan,
            "approval" => Self::Approval,
            "usage" => Self::Usage,
            "error" => Self::Error,
            _ => return None,
        })
    }
}

/// One normalised event on the `rigel://events` channel.
///
/// The renderer subscribes once and routes by `type`. Deltas exist so streaming text is
/// live; the persisted `Item` that follows is what replay reads, so a dropped delta costs
/// a repaint, not a transcript.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    TurnStarted {
        session_id: String,
        agent_id: String,
        turn_id: String,
        seq: i64,
        prompt: Option<String>,
    },
    Delta {
        turn_id: String,
        kind: ItemKind,
        text: String,
    },
    Item {
        item: Item,
    },
    TurnFinished {
        turn_id: String,
        status: String,
        stop: String,
        usage: Usage,
        session_spend_micros: i64,
        cap_micros: Option<i64>,
    },
    BudgetWarning {
        session_id: String,
        spend_micros: i64,
        cap_micros: i64,
        remaining_micros: i64,
    },
    BudgetExceeded {
        session_id: String,
        spend_micros: i64,
        cap_micros: i64,
    },
    ApprovalRequested {
        approval_id: String,
        kind: String,
        detail: serde_json::Value,
    },
    ApprovalResolved {
        approval_id: String,
        decision: String,
    },
    SessionUpdated {
        session: crate::session::Session,
    },
    Error {
        turn_id: Option<String>,
        message: String,
    },
}

/// Somewhere to put events: a Tauri window, a test vector, or `/dev/null`.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: AgentEvent);
}

pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: AgentEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cents_are_rounded_once_not_per_turn() {
        assert_eq!(micros_to_cents(4_999), 0);
        assert_eq!(micros_to_cents(5_000), 1);
        assert_eq!(micros_to_cents(14_000), 1);
        assert_eq!(micros_to_cents(1_000_000), 100);
        assert_eq!(cents_to_micros(25), 250_000);
    }

    #[test]
    fn a_total_is_estimated_if_any_part_is() {
        let mut total = Usage {
            cost_micros: 100,
            provider_micros: Some(100),
            source: CostSource::Provider,
            ..Usage::default()
        };
        total.add(&Usage {
            cost_micros: 500,
            source: CostSource::Estimated,
            ..Usage::default()
        });
        assert_eq!(total.source, CostSource::Estimated);
        assert_eq!(total.cost_micros, 600);
    }
}

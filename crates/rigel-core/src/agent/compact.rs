//! Context compaction at 80% of the model window (roadmap 31).
//!
//! The coordinator normally sends only the current prompt. Once this agent has enough prior
//! turns to cross 80% of the window, [`build`] keeps the most recent turns verbatim, replaces
//! older turns with an extractive summary, preserves every pinned skill hash, and records each
//! dropped turn's item ids. The record is written to the transcript before the next request.

use serde::Serialize;

use crate::session::{Item, SkillPin, TurnTranscript};

use super::model::ChatMessage;
use super::tokens;

/// The fraction of the provider window at which compaction starts.
pub const COMPACT_AT_PCT: usize = 80;
/// Recent turns kept verbatim instead of being folded into the summary.
pub const RETAIN_RECENT_TURNS: usize = 4;
/// The summary is bounded so compaction cannot simply recreate the history it replaced.
pub const MAX_SUMMARY_CHARS: usize = 12_000;
/// A single tool result is represented by a bounded prefix in the summary. The full result
/// remains in the transcript; the record names the item it came from.
pub const MAX_TOOL_RESULT_CHARS: usize = 600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DroppedSpan {
    pub turn_id: String,
    pub item_ids: Vec<String>,
    pub chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompactionRecord {
    pub summary: String,
    /// Every dropped span, with the turn id that owned it.
    pub dropped: Vec<DroppedSpan>,
    /// The recent turns kept as messages.
    pub retained_turns: Vec<String>,
    pub before_tokens: usize,
    pub after_tokens: usize,
    pub window: usize,
    pub threshold_pct: usize,
}

#[derive(Debug, Clone)]
pub struct Compaction {
    pub record: CompactionRecord,
    /// The recent turns, as messages the next request can actually send.
    pub recent: Vec<ChatMessage>,
}

impl Compaction {
    pub fn prompt(&self) -> String {
        format!(
            "Compacted history (roadmap 31). Older turns were replaced by this summary; the \
             exact dropped spans are recorded on the turn. Pinned facts and tool-result \
             prefixes are preserved.\n\n{}",
            self.record.summary
        )
    }

    pub fn record(&self) -> &CompactionRecord {
        &self.record
    }
}

pub fn build(history: &[TurnTranscript], pins: &[SkillPin], model: &str) -> Option<Compaction> {
    if history.len() < 2 {
        return None;
    }
    let before_chars = history_chars(history);
    let before = tokens::estimate_chars(before_chars, model);
    let threshold = before.window * COMPACT_AT_PCT / 100;
    if before.tokens <= threshold {
        return None;
    }

    let retain = RETAIN_RECENT_TURNS.min(history.len() / 2).max(1);
    let split = history.len() - retain;
    let dropped_turns = &history[..split];
    let recent_turns = &history[split..];
    let dropped = dropped_turns
        .iter()
        .map(|transcript| DroppedSpan {
            turn_id: transcript.turn.id.clone(),
            item_ids: transcript
                .items
                .iter()
                .map(|item| item.id.clone())
                .collect(),
            chars: turn_chars(transcript),
        })
        .collect::<Vec<_>>();
    let retained_turns = recent_turns
        .iter()
        .map(|transcript| transcript.turn.id.clone())
        .collect::<Vec<_>>();
    let summary = summarize(dropped_turns, recent_turns, pins);
    let after_chars = summary.chars().count() + recent_turns.iter().map(turn_chars).sum::<usize>();
    let after_tokens = tokens::estimate_chars(after_chars, model).tokens;
    let recent = recent_turns
        .iter()
        .flat_map(messages_for)
        .collect::<Vec<_>>();

    Some(Compaction {
        record: CompactionRecord {
            summary,
            dropped,
            retained_turns,
            before_tokens: before.tokens,
            after_tokens,
            window: before.window,
            threshold_pct: COMPACT_AT_PCT,
        },
        recent,
    })
}

fn history_chars(history: &[TurnTranscript]) -> usize {
    history.iter().map(turn_chars).sum()
}

fn turn_chars(transcript: &TurnTranscript) -> usize {
    transcript.turn.prompt.as_deref().map(str::len).unwrap_or(0)
        + transcript
            .items
            .iter()
            .map(|item| {
                serde_json::to_string(&item.body)
                    .map(|body| body.len())
                    .unwrap_or(0)
            })
            .sum::<usize>()
}

fn summarize(dropped: &[TurnTranscript], recent: &[TurnTranscript], pins: &[SkillPin]) -> String {
    let mut out = String::new();
    for transcript in dropped {
        out.push_str(&format!(
            "\n\nTurn {} (seq {}, status {}):",
            transcript.turn.id, transcript.turn.seq, transcript.turn.status
        ));
        if let Some(prompt) = &transcript.turn.prompt {
            out.push_str("\nUser: ");
            out.push_str(prompt);
        }
        for item in &transcript.items {
            append_item(&mut out, item);
        }
        if out.len() >= MAX_SUMMARY_CHARS {
            out.push_str(
                "\n\n[summary truncated at its documented bound; dropped spans remain recorded]",
            );
            break;
        }
    }

    // Tool results from retained turns are still part of the context's working set, so keep
    // their bounded prefixes here as well; the recent messages carry the prose.
    if !recent.is_empty() {
        out.push_str("\n\nRecent tool results kept from retained turns:");
        for transcript in recent {
            for item in &transcript.items {
                if is_tool(item) {
                    out.push_str(&format!("\n- turn {}: ", transcript.turn.id));
                    append_item(&mut out, item);
                }
            }
        }
    }

    // Pins are appended after the bound so truncation can never remove a fact that must
    // survive. The hash is the version boundary, not a mutable body.
    if pins.is_empty() {
        out.push_str("\n\nPinned facts: none.");
    } else {
        out.push_str("\n\nPinned facts (preserved):");
        for pin in pins {
            out.push_str(&format!("\n- {} @ {}", pin.skill_id, pin.body_sha));
        }
    }
    if out.chars().count() > MAX_SUMMARY_CHARS + 2_000 {
        // The per-item prefixes already bound each entry; this catches pathological prompt
        // sizes while retaining the final pin section.
        let cut = out
            .char_indices()
            .nth(MAX_SUMMARY_CHARS)
            .map(|(index, _)| index)
            .unwrap_or(out.len());
        let pins_at = out.rfind("\n\nPinned facts").unwrap_or(cut);
        let mut bounded = out[..cut.min(pins_at)].to_string();
        bounded.push_str("\n\n[summary truncated]");
        bounded.push_str(&out[pins_at..]);
        out = bounded;
    }
    out
}

fn append_item(out: &mut String, item: &Item) {
    match item.kind.as_str() {
        "message" => {
            if let Some(text) = item.body.get("text").and_then(|text| text.as_str()) {
                let role = item
                    .body
                    .get("role")
                    .and_then(|role| role.as_str())
                    .unwrap_or("assistant");
                out.push_str(&format!("\n{role}: {text}"));
            }
        }
        "error" => {
            if let Some(message) = item
                .body
                .get("message")
                .and_then(|message| message.as_str())
            {
                out.push_str(&format!("\nError: {message}"));
            }
        }
        "note" => {
            if let Some(text) = item.body.get("text").and_then(|text| text.as_str()) {
                out.push_str(&format!("\nUser intervention: {text}"));
            }
        }
        kind if is_tool_kind(kind) => {
            let tool = item
                .body
                .get("tool")
                .and_then(|tool| tool.as_str())
                .unwrap_or(item.kind.as_str());
            let result = item.body.get("result").unwrap_or(&item.body).to_string();
            out.push_str(&format!(
                "\nTool {tool}: {}",
                bound(&result, MAX_TOOL_RESULT_CHARS)
            ));
        }
        _ => {}
    }
}

fn is_tool(item: &Item) -> bool {
    is_tool_kind(&item.kind)
}

fn is_tool_kind(kind: &str) -> bool {
    matches!(kind, "mcp_call" | "command" | "file_change")
}

fn bound(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let end = text
        .char_indices()
        .nth(max)
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    format!("{}… [truncated; full result in transcript]", &text[..end])
}

fn messages_for(transcript: &TurnTranscript) -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    if let Some(prompt) = &transcript.turn.prompt {
        messages.push(ChatMessage::user(prompt));
    }
    for item in &transcript.items {
        if item.kind != "message" {
            continue;
        }
        if item.body.get("role").and_then(|role| role.as_str()) != Some("assistant") {
            continue;
        }
        if let Some(text) = item.body.get("text").and_then(|text| text.as_str()) {
            messages.push(ChatMessage::assistant(Some(text.to_string()), Vec::new()));
        }
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Usage;
    use crate::session::Turn;

    fn turn(id: &str, seq: i64, prompt: &str) -> TurnTranscript {
        TurnTranscript {
            turn: Turn {
                id: id.to_string(),
                agent_id: "a".to_string(),
                parent_turn_id: None,
                seq,
                status: "completed".to_string(),
                prompt: Some(prompt.to_string()),
                started_at: seq,
                completed_at: Some(seq),
                usage: Some(Usage::default()),
                stop_reason: Some("completed".to_string()),
            },
            items: vec![Item {
                id: format!("{id}-item"),
                turn_id: id.to_string(),
                seq: 1,
                kind: "message".to_string(),
                status: "complete".to_string(),
                body: serde_json::json!({ "role": "assistant", "text": "x".repeat(4_000) }),
                created_at: seq,
                raw: None,
            }],
        }
    }

    #[test]
    fn below_the_threshold_nothing_is_dropped() {
        let history = vec![turn("t1", 1, "one"), turn("t2", 2, "two")];
        assert!(build(&history, &[], "openai/gpt-5-mini").is_none());
    }

    #[test]
    fn a_large_history_drops_old_spans_and_keeps_pins() {
        let history = (1..=200)
            .map(|seq| turn(&format!("t{seq}"), seq, "do the work"))
            .collect::<Vec<_>>();
        let pins = vec![SkillPin {
            skill_id: "pdf-tools".to_string(),
            body_sha: "sha-pinned".to_string(),
            pinned_at: 1,
        }];
        let compaction = build(&history, &pins, "openai/gpt-4o").unwrap();
        assert_eq!(compaction.record.dropped.len(), 196);
        assert_eq!(compaction.record.retained_turns.len(), 4);
        assert!(
            compaction
                .record
                .dropped
                .iter()
                .all(|span| !span.turn_id.is_empty())
        );
        assert!(compaction.record.summary.contains("pdf-tools @ sha-pinned"));
        assert!(compaction.record.after_tokens < compaction.record.before_tokens);
        assert!(compaction.prompt().contains("Compacted history"));
    }
}

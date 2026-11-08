//! A bounded, rotating log of the last events (roadmap 4).
//!
//! The window and the coordinator can be noisy, and a support bundle needs the newest lines,
//! not a disk-filling file. The ring keeps a fixed number of entries *and* a fixed number of
//! bytes: a burst of long messages evicts the same way a burst of short ones does, and the
//! newest event always survives. Every message and span passes the shared redactor on the way
//! in, so a token a command printed cannot sit in memory waiting for a bundle to pick it up.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// The roadmap's entry cap: the last thousand events.
pub const DEFAULT_ENTRIES: usize = 1_000;
/// A quarter of a megabyte of text; long enough for a stack trace, small enough to paste.
pub const DEFAULT_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "trace",
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub at_ms: u128,
    pub level: Level,
    /// The span the event belongs to, when it has one: a turn id, a tool name, a window label.
    pub span: Option<String>,
    /// Already redacted; the ring never stores the raw text.
    pub message: String,
}

impl Event {
    fn weight(&self) -> usize {
        self.message.len() + self.span.as_ref().map(String::len).unwrap_or(0)
    }
}

/// The ring itself. Plain old data so a caller can own one in a test; [`shared`] is the
/// process-wide copy the crash path writes to.
pub struct Ring {
    entries: VecDeque<Event>,
    bytes: usize,
    max_entries: usize,
    max_bytes: usize,
}

impl Default for Ring {
    fn default() -> Self {
        Self::new()
    }
}

impl Ring {
    pub fn new() -> Self {
        Self::with_limits(DEFAULT_ENTRIES, DEFAULT_BYTES)
    }

    pub fn with_limits(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            max_entries: max_entries.max(1),
            max_bytes: max_bytes.max(1),
        }
    }

    /// Record one event. The message and span are redacted first, then the message is clamped
    /// to whatever byte budget the span leaves, so a single huge event cannot break the cap.
    pub fn record(&mut self, level: Level, span: Option<&str>, message: &str) {
        let message = crate::redaction::redact_text(message);
        let span = span
            .map(crate::redaction::redact_text)
            .filter(|span| !span.is_empty());
        let span_len = span.as_ref().map(String::len).unwrap_or(0);
        let budget = self.max_bytes.saturating_sub(span_len).max(1);
        let event = Event {
            at_ms: now_ms(),
            level,
            span,
            message: clamp(&message, budget),
        };
        self.bytes += event.weight();
        self.entries.push_back(event);
        // Keep the newest event even when it alone is at the cap; evict from the oldest end.
        while self.entries.len() > 1
            && (self.entries.len() > self.max_entries || self.bytes > self.max_bytes)
        {
            if let Some(oldest) = self.entries.pop_front() {
                self.bytes = self.bytes.saturating_sub(oldest.weight());
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    pub fn events(&self) -> impl Iterator<Item = &Event> {
        self.entries.iter()
    }

    pub fn snapshot(&self) -> Vec<Event> {
        self.entries.iter().cloned().collect()
    }

    /// One line per event, oldest first, in the shape a log paste expects.
    pub fn lines(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|event| {
                format!(
                    "{} {:<5} {} {}",
                    event.at_ms,
                    event.level.as_str(),
                    event.span.as_deref().unwrap_or("-"),
                    event.message
                )
            })
            .collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}

/// The process-wide ring. The crash hook writes here as well as to `crash.log`, so a later
/// support bundle can show the events around a failure without rereading the file.
pub fn shared() -> &'static Mutex<Ring> {
    static RING: OnceLock<Mutex<Ring>> = OnceLock::new();
    RING.get_or_init(|| Mutex::new(Ring::new()))
}

/// Record into [`shared`], ignoring a poisoned lock: diagnostics must never be the reason a
/// turn fails.
pub fn shared_record(level: Level, span: Option<&str>, message: &str) {
    if let Ok(mut ring) = shared().lock() {
        ring.record(level, span, message);
    }
}

fn clamp(text: &str, budget: usize) -> String {
    if text.len() <= budget {
        return text.to_string();
    }
    if budget <= 3 {
        return truncate(text, budget);
    }
    format!("{}...", truncate(text, budget - 3))
}

fn truncate(text: &str, budget: usize) -> String {
    let mut cut = budget.min(text.len());
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text[..cut].to_string()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_cap_evicts_the_oldest_and_keeps_the_newest() {
        let mut ring = Ring::with_limits(3, 1_000_000);
        for index in 0..6 {
            ring.record(Level::Info, None, &format!("event {index}"));
        }
        assert_eq!(ring.len(), 3);
        let messages: Vec<&str> = ring.events().map(|event| event.message.as_str()).collect();
        assert_eq!(messages, ["event 3", "event 4", "event 5"]);
    }

    #[test]
    fn the_byte_cap_holds_and_the_newest_event_survives() {
        let mut ring = Ring::with_limits(100, 120);
        for index in 0..10 {
            ring.record(
                Level::Info,
                None,
                &format!("message-{index}-{}", "x".repeat(30)),
            );
        }
        assert!(ring.bytes() <= 120, "bytes = {}", ring.bytes());
        assert!(ring.len() < 10);
        let newest = ring.events().last().expect("the newest event is kept");
        assert!(newest.message.starts_with("message-9-"));
    }

    #[test]
    fn one_oversized_message_is_clamped_rather_than_dropped() {
        let mut ring = Ring::with_limits(10, 40);
        ring.record(Level::Error, None, &"a".repeat(500));
        assert_eq!(ring.len(), 1);
        assert!(ring.bytes() <= 40, "bytes = {}", ring.bytes());
        let message = &ring.events().next().unwrap().message;
        assert!(message.ends_with("..."));
        assert!(message.len() <= 40);
    }

    #[test]
    fn a_span_counts_against_the_byte_budget_too() {
        let mut ring = Ring::with_limits(10, 30);
        ring.record(Level::Info, Some(&"s".repeat(20)), &"m".repeat(500));
        assert!(ring.bytes() <= 30, "bytes = {}", ring.bytes());
        assert_eq!(ring.len(), 1);
    }

    #[test]
    fn messages_and_spans_pass_the_shared_redactor_before_they_are_stored() {
        let mut ring = Ring::with_limits(10, 1_000);
        ring.record(
            Level::Warn,
            Some("turn sk-live-AbCdEfGhIjKlMnOp"),
            "request failed with sk-proj-AbCdEfGhIjKlMnOpQrStUvWx",
        );
        let event = ring.events().next().unwrap();
        let stored = format!("{} {}", event.message, event.span.as_deref().unwrap_or(""));
        assert!(!stored.contains("sk-proj-"), "{stored}");
        assert!(!stored.contains("sk-live-"), "{stored}");
        assert!(stored.contains("<redacted>"), "{stored}");
        assert!(ring.lines()[0].contains("<redacted>"));
    }

    #[test]
    fn lines_name_the_level_and_the_span() {
        let mut ring = Ring::with_limits(10, 1_000);
        ring.record(Level::Error, Some("window"), "webview closed");
        let line = &ring.lines()[0];
        assert!(line.contains(" error "), "{line}");
        assert!(line.contains(" window "), "{line}");
        assert!(line.ends_with("webview closed"), "{line}");
    }

    #[test]
    fn the_default_ring_is_the_roadmaps_thousand_entries() {
        let ring = Ring::new();
        assert_eq!(ring.max_entries(), DEFAULT_ENTRIES);
        assert_eq!(ring.max_bytes(), DEFAULT_BYTES);
        assert!(ring.is_empty());
        assert_eq!(ring.bytes(), 0);
    }

    #[test]
    fn a_shared_record_lands_in_the_process_ring() {
        shared_record(Level::Info, Some("test"), "shared ring test event");
        let ring = shared().lock().expect("the shared ring is not poisoned");
        assert!(
            ring.events()
                .any(|event| event.message == "shared ring test event")
        );
    }
}

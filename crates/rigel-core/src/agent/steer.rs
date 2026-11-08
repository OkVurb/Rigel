//! User steering for a running turn (roadmap 39).
//!
//! A note is queued by turn id and drained exactly once at the next step boundary. It is not
//! injected into a provider stream: the current step finishes, then the note is stored as a
//! user-intervention row and becomes the next user message.

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct SteerQueue {
    notes: Mutex<HashMap<String, Vec<String>>>,
}

impl SteerQueue {
    pub fn push(&self, turn_id: &str, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() || turn_id.is_empty() {
            return false;
        }
        match self.notes.lock() {
            Ok(mut notes) => {
                notes
                    .entry(turn_id.to_string())
                    .or_default()
                    .push(text.to_string());
                true
            }
            Err(_) => false,
        }
    }

    /// Remove every note for this turn. A second drain returns nothing, which is what makes the
    /// intervention land exactly once even if the coordinator reaches several boundaries.
    pub fn take(&self, turn_id: &str) -> Vec<String> {
        self.notes
            .lock()
            .ok()
            .and_then(|mut notes| notes.remove(turn_id))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_drained_exactly_once() {
        let queue = SteerQueue::default();
        assert!(queue.push("turn-1", "  also check the lockfile  "));
        assert_eq!(queue.take("turn-1"), vec!["also check the lockfile"]);
        assert!(queue.take("turn-1").is_empty());
        assert!(!queue.push("turn-1", "   "));
    }
}

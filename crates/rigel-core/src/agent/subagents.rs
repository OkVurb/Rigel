//! Background subagents: one registry per top-level turn.
//!
//! `spawn_agents` starts its children in threads and returns at once, so the coordinator can
//! keep talking to them in the room while they work. The registry is what turns that back into
//! a result: each thread claims a slot when it starts and writes its final answer into the same
//! slot when it finishes. `subagent_results` reads those slots, and the turn loop refuses to
//! finish while any slot is still running.
//!
//! The registry lives in [`crate::agent::tools::Tools`] because that is already the per-turn
//! object the coordinator, and only the coordinator, owns. A child gets a fresh empty registry:
//! it cannot spawn, and it must not inherit its parent's pending children.

use std::sync::Mutex;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// One child the parent asked for. `result` is exactly the JSON the old synchronous fan-out
/// used to return inside `spawn_agents`, so callers only have to learn the new tool name.
#[derive(Debug, Clone)]
pub struct ChildSlot {
    pub agent_id: String,
    pub role: String,
    pub files: Vec<String>,
    pub chat_name: String,
    /// `running`, `done` or `failed`.
    pub status: String,
    /// Whether the parent has been handed this terminal result. A turn may not finish with a
    /// finished child it never read: `spawn_agents` no longer returns the answers itself.
    pub collected: bool,
    pub result: Option<Value>,
    /// Cancels this child's turn and everything below it. The parent's token is its parent.
    pub cancel: CancellationToken,
}

pub struct SubagentHub {
    slots: Mutex<Vec<ChildSlot>>,
}

impl Default for SubagentHub {
    fn default() -> Self {
        Self::new()
    }
}

impl SubagentHub {
    pub fn new() -> Self {
        Self {
            slots: Mutex::new(Vec::new()),
        }
    }

    /// Claim a slot before the thread starts, so a result can never arrive for an unknown child
    /// and the parent's barrier sees the child the moment it exists.
    pub fn start(
        &self,
        agent_id: &str,
        role: &str,
        files: &[String],
        chat_name: &str,
        cancel: CancellationToken,
    ) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.push(ChildSlot {
                agent_id: agent_id.to_string(),
                role: role.to_string(),
                files: files.to_vec(),
                chat_name: chat_name.to_string(),
                status: "running".to_string(),
                collected: false,
                result: None,
                cancel,
            });
        }
    }

    /// Publish a child's final JSON. Safe to call twice: the first terminal answer wins.
    pub fn finish(&self, agent_id: &str, result: Value) {
        if let Ok(mut slots) = self.slots.lock() {
            let Some(slot) = slots.iter_mut().find(|slot| slot.agent_id == agent_id) else {
                return;
            };
            if slot.status != "running" {
                return;
            }
            let ok = result.get("ok").and_then(Value::as_bool).unwrap_or(false);
            slot.status = if ok { "done" } else { "failed" }.to_string();
            slot.result = Some(result);
        }
    }

    /// A thread that panics or exits without reporting must not leave the parent waiting for a
    /// slot that will never be filled.
    pub fn fail(&self, agent_id: &str, message: impl Into<String>) {
        self.finish(
            agent_id,
            json!({
                "ok": false,
                "agent_id": agent_id,
                "error": message.into(),
            }),
        );
    }

    pub fn snapshot(&self) -> Vec<ChildSlot> {
        self.slots
            .lock()
            .map(|slots| slots.clone())
            .unwrap_or_default()
    }

    pub fn outstanding(&self) -> usize {
        self.slots
            .lock()
            .map(|slots| slots.iter().filter(|slot| slot.status == "running").count())
            .unwrap_or(0)
    }

    /// Finished children whose answer the parent has not collected yet.
    pub fn uncollected(&self) -> usize {
        self.slots
            .lock()
            .map(|slots| {
                slots
                    .iter()
                    .filter(|slot| slot.status != "running" && !slot.collected)
                    .count()
            })
            .unwrap_or(0)
    }

    /// Mark terminal slots as read. Running slots stay uncollected until their answer exists.
    pub fn mark_collected(&self, agent_ids: &[String]) {
        if let Ok(mut slots) = self.slots.lock() {
            for slot in slots.iter_mut() {
                if slot.status != "running" && agent_ids.contains(&slot.agent_id) {
                    slot.collected = true;
                }
            }
        }
    }

    /// Cancel every child still running. Used when the parent stops or hits its step limit, so
    /// a detached child cannot keep spending after the turn that owns it is over.
    pub fn cancel_all(&self) {
        if let Ok(slots) = self.slots.lock() {
            for slot in slots.iter().filter(|slot| slot.status == "running") {
                slot.cancel.cancel();
            }
        }
    }
}

/// Marks a slot failed if its thread unwinds before publishing a result.
pub struct SlotGuard {
    pub hub: std::sync::Arc<SubagentHub>,
    pub agent_id: String,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.hub.fail(
            &self.agent_id,
            "the subagent thread exited without a result",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_is_running_until_it_publishes_a_terminal_result() {
        let hub = SubagentHub::new();
        hub.start(
            "child-1",
            "reader",
            &["README.md".to_string()],
            "reader-9b1c",
            CancellationToken::new(),
        );
        assert_eq!(hub.outstanding(), 1);
        assert_eq!(hub.snapshot()[0].status, "running");

        hub.finish("child-1", json!({ "ok": true, "text": "found it" }));
        let slot = &hub.snapshot()[0];
        assert_eq!(hub.outstanding(), 0);
        assert_eq!(slot.status, "done");
        assert_eq!(slot.result.as_ref().unwrap()["text"], "found it");
        assert_eq!(
            hub.uncollected(),
            1,
            "a finished answer is not read until the parent collects it"
        );
        hub.mark_collected(&["child-1".to_string()]);
        assert_eq!(hub.uncollected(), 0);
    }

    #[test]
    fn an_unreported_thread_is_marked_failed() {
        let hub = std::sync::Arc::new(SubagentHub::new());
        hub.start(
            "child-1",
            "reader",
            &[],
            "reader-9b1c",
            CancellationToken::new(),
        );
        {
            let _guard = SlotGuard {
                hub: hub.clone(),
                agent_id: "child-1".to_string(),
            };
        }
        assert_eq!(hub.outstanding(), 0);
        assert_eq!(hub.snapshot()[0].status, "failed");
    }

    #[test]
    fn cancelling_the_hub_reaches_every_running_child() {
        let hub = SubagentHub::new();
        let first = CancellationToken::new();
        let second = CancellationToken::new();
        hub.start("a", "reader", &[], "a-1111", first.clone());
        hub.start("b", "reader", &[], "b-2222", second.clone());
        hub.cancel_all();
        assert!(first.is_cancelled());
        assert!(second.is_cancelled());
    }
}

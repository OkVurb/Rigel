//! Consent for actions that leave the patch journal.
//!
//! A file edit is reversible by construction: it is journalled and can be reverted. A shell
//! command is not, so it is shown first — exact text, working directory, timeout — and the
//! default answer is no. The CLI asks on stdin; the window gets a card in P4.10. A caller
//! with neither surface uses [`DenyCommands`], which is safe by default.

use std::path::PathBuf;

use async_trait::async_trait;

use super::deliberation::Deliberation;
use super::destructive::Finding;

/// What the caller (and the user) is asked to approve. Everything the command will do that
/// is not obvious from its text is visible here: the directory it runs in and how long it may
/// run before Rigel kills it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRequest {
    pub command: String,
    /// Absolute project root. `cwd` is always inside it.
    pub project_root: PathBuf,
    pub cwd: PathBuf,
    pub timeout_secs: u64,
    /// Policy rows the deterministic classifier matched (D32). Empty for an ordinary command;
    /// a non-empty list is what requires deliberation and a snapshot.
    pub destructive: Vec<Finding>,
    /// The three advisors' one round, when the command is destructive. Advisory: the gate does
    /// not read it, and `None` never means "allowed".
    pub deliberation: Option<Deliberation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandDecision {
    /// Run it once. Rigel does not remember the answer.
    AllowOnce,
    Deny,
}

/// Who answers a [`CommandRequest`]. Implementations must fail closed: an error, EOF or an
/// unreadable answer denies.
#[async_trait]
pub trait CommandApprover: Send + Sync {
    async fn approve(&self, request: &CommandRequest) -> CommandDecision;
}

/// Refuses everything. The default for a caller with no approval surface.
pub struct DenyCommands;

#[async_trait]
impl CommandApprover for DenyCommands {
    async fn approve(&self, _request: &CommandRequest) -> CommandDecision {
        CommandDecision::Deny
    }
}

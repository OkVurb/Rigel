//! Consent for actions that leave the patch journal.
//!
//! A file edit is reversible by construction: it is journalled and can be reverted. A shell
//! command is not, so it is shown first — exact text, working directory, timeout — and the
//! default answer is no. The CLI asks on stdin; the window gets a card (P4.10). A caller with
//! neither surface uses [`DenyCommands`], which is safe by default.
//!
//! **Scope (N2).** An answer is not only yes or no, it is "yes, for how long": once, for this
//! session, for this project, or always. The four scopes live here so every surface shows the
//! same words and the same stored keys. A grant is keyed by the exact text the user read, so
//! approving `cargo test` never approves `cargo test -- --nocapture` by accident.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::deliberation::Deliberation;
use super::destructive::Finding;

/// How long an approval lasts, as N2 lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    /// This call only. Nothing is remembered.
    Once,
    /// Until this process exits.
    Session,
    /// This project, across restarts.
    Project,
    /// Every project, across restarts.
    Always,
}

impl ApprovalScope {
    pub const ALL: [ApprovalScope; 4] = [Self::Once, Self::Session, Self::Project, Self::Always];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Session => "session",
            Self::Project => "project",
            Self::Always => "always",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "once" => Self::Once,
            "session" => Self::Session,
            "project" => Self::Project,
            "always" => Self::Always,
            _ => return None,
        })
    }
}

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
    /// Allowed, with the scope the user chose.
    Allow(ApprovalScope),
    /// Refused. A refusal is never remembered: the next ask asks again.
    Deny,
}

impl CommandDecision {
    pub const ALLOW_ONCE: Self = Self::Allow(ApprovalScope::Once);

    pub fn is_allowed(self) -> bool {
        matches!(self, Self::Allow(_))
    }
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

/// One writable child as the user sees it before it starts (D114).
///
/// The plan is the child's own numbered statement of what it will change; it is required
/// because a task line alone does not tell a reader which edits are coming.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanAgent {
    pub role: String,
    pub files: Vec<String>,
    pub task: String,
    pub plan: String,
}

/// A fan-out of writable children, waiting for one answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanRequest {
    pub session_id: String,
    pub agents: Vec<PlanAgent>,
}

/// Approve, skip the gate for this fan-out with a reason the ledger keeps, or deny. Denying
/// starts nothing at all: the plan gate is before registration, not after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanDecision {
    Approve,
    Skip { reason: String },
    Deny,
}

impl PlanDecision {
    pub fn is_allowed(&self) -> bool {
        !matches!(self, Self::Deny)
    }

    /// What `agents.plan_note` records, so a skipped gate reads as a waiver rather than an
    /// approval.
    pub fn note(&self) -> Option<String> {
        match self {
            Self::Approve => None,
            Self::Skip { reason } => Some(format!("skipped by the user: {reason}")),
            Self::Deny => None,
        }
    }
}

/// Who answers a [`PlanRequest`]. Implementations must fail closed: an error, a timeout or
/// an unreadable answer denies.
#[async_trait]
pub trait PlanApprover: Send + Sync {
    async fn approve_plan(&self, request: &PlanRequest) -> PlanDecision;
}

/// Refuses every writable fan-out. The default where no human surface is installed.
pub struct DenyPlans;

#[async_trait]
impl PlanApprover for DenyPlans {
    async fn approve_plan(&self, _request: &PlanRequest) -> PlanDecision {
        PlanDecision::Deny
    }
}

/// What has already been approved, keyed by the exact text the user saw.
///
/// Cloneable and cheap: a turn's tool set and the window share one of these, which is what
/// makes the `session` scope mean "this session" rather than "this turn". `project` and
/// `always` are files under the project root and the index directory, written only when the
/// user picks those scopes.
#[derive(Clone, Debug)]
pub struct Scopes {
    session: Arc<Mutex<HashSet<String>>>,
    project_file: PathBuf,
    always_file: PathBuf,
}

impl Scopes {
    pub fn new(project_root: impl AsRef<Path>, index_dir: impl AsRef<Path>) -> Self {
        Self {
            session: Arc::new(Mutex::new(HashSet::new())),
            project_file: project_root.as_ref().join(".rigel").join("approvals.json"),
            always_file: index_dir.as_ref().join("approvals.json"),
        }
    }

    /// The scope that already covers `key`, strongest first. `None` means the user has not
    /// approved this exact text at any scope.
    pub fn granted(&self, key: &str) -> Option<ApprovalScope> {
        if self
            .session
            .lock()
            .map(|session| session.contains(key))
            .unwrap_or(false)
        {
            return Some(ApprovalScope::Session);
        }
        if load(&self.project_file).contains(key) {
            return Some(ApprovalScope::Project);
        }
        if load(&self.always_file).contains(key) {
            return Some(ApprovalScope::Always);
        }
        None
    }

    /// Remember an approval at `scope`. `Once` is a no-op: nothing outlives the call.
    pub fn remember(&self, key: &str, scope: ApprovalScope) -> Result<(), String> {
        match scope {
            ApprovalScope::Once => Ok(()),
            ApprovalScope::Session => {
                if let Ok(mut session) = self.session.lock() {
                    session.insert(key.to_string());
                }
                Ok(())
            }
            ApprovalScope::Project => append(&self.project_file, key),
            ApprovalScope::Always => append(&self.always_file, key),
        }
    }

    /// The persisted grants, for a settings surface that wants to show or clear them.
    pub fn persisted(&self) -> (Vec<String>, Vec<String>) {
        let mut project = load(&self.project_file).into_iter().collect::<Vec<_>>();
        let mut always = load(&self.always_file).into_iter().collect::<Vec<_>>();
        project.sort();
        always.sort();
        (project, always)
    }
}

fn load(path: &Path) -> HashSet<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashSet::new();
    };
    serde_json::from_str::<Vec<String>>(&text)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn append(path: &Path, key: &str) -> Result<(), String> {
    let mut keys = load(path);
    if !keys.insert(key.to_string()) {
        return Ok(());
    }
    let mut sorted = keys.drain().collect::<Vec<_>>();
    sorted.sort();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(&sorted).map_err(|error| error.to_string())?;
    std::fs::write(path, text)
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

/// A question waiting for a human, written where any surface can find it.
///
/// The CLI asks on stdin, but an MCP permission tool runs in the harness's child process and a
/// Tauri window runs in another thread entirely. Neither can borrow the agent's stack, so the
/// question is a file: published under `approvals/pending`, answered under `approvals/answered`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub id: String,
    /// Stable key for [`Scopes`]: the kind plus the exact text the human reads.
    pub key: String,
    pub kind: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
    /// Milliseconds since the epoch, so a surface can show the oldest first.
    pub requested_at: i64,
}

/// What a surface wrote back. `None` is a rejection, which is also what a timeout means.
///
/// `skip` is only meaningful for a plan question: it accepts the fan-out without endorsing
/// the plan, which D114 allows when the surface records why in `note`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnsweredApproval {
    pub scope: Option<ApprovalScope>,
    #[serde(default)]
    pub skip: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The on-disk approval channel. Cheap to clone; every surface points at the same directory.
#[derive(Debug, Clone)]
pub struct ApprovalBus {
    root: PathBuf,
}

impl ApprovalBus {
    pub fn new(index_dir: impl AsRef<Path>) -> Self {
        Self {
            root: index_dir.as_ref().join("approvals"),
        }
    }

    fn pending_dir(&self) -> PathBuf {
        self.root.join("pending")
    }

    fn answered_dir(&self) -> PathBuf {
        self.root.join("answered")
    }

    /// Ask a human. Returns their scope, or an error when nobody answered in `wait`; a caller
    /// must treat an error as a refusal (N2).
    pub async fn ask(
        &self,
        pending: PendingApproval,
        wait: std::time::Duration,
    ) -> Result<ApprovalScope, String> {
        match self.ask_answered(pending, wait).await {
            Ok(answered) => match answered.scope {
                Some(scope) => Ok(scope),
                None => Err("the request was rejected".to_string()),
            },
            Err(error) => Err(error),
        }
    }

    /// Ask a plan question. A skip is only a skip when a surface said so; a timeout, a
    /// rejection and an unreadable answer all deny.
    pub async fn ask_plan(
        &self,
        pending: PendingApproval,
        wait: std::time::Duration,
    ) -> PlanDecision {
        match self.ask_answered(pending, wait).await {
            Ok(answered) if answered.skip => PlanDecision::Skip {
                reason: answered
                    .note
                    .filter(|note| !note.trim().is_empty())
                    .unwrap_or_else(|| "no reason given".to_string()),
            },
            Ok(answered) if answered.scope.is_some() => PlanDecision::Approve,
            _ => PlanDecision::Deny,
        }
    }

    async fn ask_answered(
        &self,
        pending: PendingApproval,
        wait: std::time::Duration,
    ) -> Result<AnsweredApproval, String> {
        self.publish(&pending)?;
        let deadline = std::time::Instant::now() + wait;
        loop {
            if let Some(answered) = self.take_answer(&pending.id) {
                let _ =
                    std::fs::remove_file(self.pending_dir().join(format!("{}.json", pending.id)));
                return Ok(answered);
            }
            if std::time::Instant::now() >= deadline {
                let _ =
                    std::fs::remove_file(self.pending_dir().join(format!("{}.json", pending.id)));
                return Err(format!(
                    "nobody answered within {}s",
                    wait.as_secs_f32().round()
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    pub fn publish(&self, pending: &PendingApproval) -> Result<(), String> {
        let dir = self.pending_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
        let text = serde_json::to_string_pretty(pending).map_err(|error| error.to_string())?;
        std::fs::write(dir.join(format!("{}.json", pending.id)), text)
            .map_err(|error| format!("could not publish the request: {error}"))
    }

    /// Every open question, oldest first. A file that cannot be read is skipped, not fatal.
    pub fn pending(&self) -> Vec<PendingApproval> {
        let Ok(entries) = std::fs::read_dir(self.pending_dir()) else {
            return Vec::new();
        };
        let mut open = entries
            .flatten()
            .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
            .filter_map(|text| serde_json::from_str::<PendingApproval>(&text).ok())
            .collect::<Vec<_>>();
        open.sort_by_key(|pending| pending.requested_at);
        open
    }

    /// Answer one question. An id that is no longer pending is not an error: a late click after
    /// a timeout must not fail the window that made it.
    pub fn answer(&self, id: &str, scope: Option<ApprovalScope>) -> Result<(), String> {
        self.answer_with(id, scope, false, None)
    }

    /// Answer a plan question: a scope approves, `skip` waives the gate, and a note is kept
    /// with the waiver so the intervention ledger says why.
    pub fn answer_with(
        &self,
        id: &str,
        scope: Option<ApprovalScope>,
        skip: bool,
        note: Option<String>,
    ) -> Result<(), String> {
        let dir = self.answered_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
        let answered = AnsweredApproval {
            // A skip is an answer in its own right; letting a stale scope ride along would
            // make the two surfaces disagree about what was approved.
            scope: if skip { None } else { scope },
            skip,
            note: note.filter(|note| !note.trim().is_empty()),
        };
        let text = serde_json::to_string(&answered).map_err(|error| error.to_string())?;
        std::fs::write(dir.join(format!("{id}.json")), text)
            .map_err(|error| format!("could not write the answer: {error}"))?;
        let _ = std::fs::remove_file(self.pending_dir().join(format!("{id}.json")));
        Ok(())
    }

    fn take_answer(&self, id: &str) -> Option<AnsweredApproval> {
        let path = self.answered_dir().join(format!("{id}.json"));
        let text = std::fs::read_to_string(&path).ok()?;
        let _ = std::fs::remove_file(path);
        serde_json::from_str(&text).ok()
    }
}

/// A fresh id that two agents asking at the same moment cannot share.
fn new_request_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!(
        "{}-{}-{nanos}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

impl PendingApproval {
    pub fn now(kind: impl Into<String>, key: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: new_request_id(),
            key: key.into(),
            kind: kind.into(),
            title: title.into(),
            detail: None,
            requested_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis() as i64)
                .unwrap_or_default(),
        }
    }

    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_parse_and_round_trip() {
        for scope in ApprovalScope::ALL {
            assert_eq!(ApprovalScope::parse(scope.as_str()), Some(scope));
        }
        assert_eq!(ApprovalScope::parse("Always"), Some(ApprovalScope::Always));
        assert_eq!(ApprovalScope::parse("forever"), None);
    }

    #[test]
    fn a_session_grant_does_not_outlive_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let scopes = Scopes::new(dir.path(), dir.path().join("index"));
        assert_eq!(scopes.granted("cargo test"), None);
        scopes
            .remember("cargo test", ApprovalScope::Session)
            .unwrap();
        assert_eq!(scopes.granted("cargo test"), Some(ApprovalScope::Session));
        // A different command is not covered by the same grant.
        assert_eq!(scopes.granted("cargo test --all"), None);

        let restarted = Scopes::new(dir.path(), dir.path().join("index"));
        assert_eq!(restarted.granted("cargo test"), None);
    }

    #[test]
    fn project_and_always_grants_survive_a_restart_in_their_own_files() {
        let dir = tempfile::tempdir().unwrap();
        let index = dir.path().join("index");
        let scopes = Scopes::new(dir.path(), &index);
        scopes
            .remember("cargo fmt", ApprovalScope::Project)
            .unwrap();
        scopes
            .remember("git status", ApprovalScope::Always)
            .unwrap();

        let restarted = Scopes::new(dir.path(), &index);
        assert_eq!(restarted.granted("cargo fmt"), Some(ApprovalScope::Project));
        assert_eq!(restarted.granted("git status"), Some(ApprovalScope::Always));

        let (project, always) = restarted.persisted();
        assert_eq!(project, vec!["cargo fmt".to_string()]);
        assert_eq!(always, vec!["git status".to_string()]);
    }

    #[test]
    fn once_remembers_nothing_and_the_store_is_shared_by_clones() {
        let dir = tempfile::tempdir().unwrap();
        let scopes = Scopes::new(dir.path(), dir.path().join("index"));
        scopes.remember("ls", ApprovalScope::Once).unwrap();
        assert_eq!(scopes.granted("ls"), None);

        let clone = scopes.clone();
        scopes.remember("pwd", ApprovalScope::Session).unwrap();
        assert_eq!(clone.granted("pwd"), Some(ApprovalScope::Session));
    }

    #[tokio::test]
    async fn a_published_request_is_answered_with_the_scope_the_human_chose() {
        let dir = tempfile::tempdir().unwrap();
        let bus = ApprovalBus::new(dir.path());
        let asker = bus.clone();

        let handle = tokio::spawn(async move {
            asker
                .ask(
                    PendingApproval::now("command", "command:cargo test", "cargo test")
                        .with_detail(serde_json::json!({ "cwd": "/work" })),
                    std::time::Duration::from_secs(5),
                )
                .await
        });

        // The card sees the question before it answers it.
        let mut open = Vec::new();
        for _ in 0..50 {
            open = bus.pending();
            if !open.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].key, "command:cargo test");
        assert_eq!(open[0].title, "cargo test");
        assert_eq!(open[0].detail.as_ref().unwrap()["cwd"], "/work");

        bus.answer(&open[0].id, Some(ApprovalScope::Session))
            .unwrap();
        assert_eq!(handle.await.unwrap(), Ok(ApprovalScope::Session));
        // Answering consumes both files: the question is not shown twice.
        assert!(bus.pending().is_empty());
    }

    #[tokio::test]
    async fn an_unanswered_request_is_a_refusal_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let bus = ApprovalBus::new(dir.path());
        let outcome = bus
            .ask(
                PendingApproval::now("command", "command:rm -rf build", "rm -rf build"),
                std::time::Duration::from_millis(150),
            )
            .await;
        assert!(outcome.is_err());
        assert!(bus.pending().is_empty());
        assert!(!dir.path().join("approvals/pending").join("x.json").exists());
    }

    #[tokio::test]
    async fn a_rejection_comes_back_as_an_error_not_a_scope() {
        let dir = tempfile::tempdir().unwrap();
        let bus = ApprovalBus::new(dir.path());
        let asker = bus.clone();
        let handle = tokio::spawn(async move {
            asker
                .ask(
                    PendingApproval::now("command", "command:git reset --hard", "git reset --hard"),
                    std::time::Duration::from_secs(5),
                )
                .await
        });
        let mut open = Vec::new();
        for _ in 0..50 {
            open = bus.pending();
            if !open.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        bus.answer(&open[0].id, None).unwrap();
        assert!(handle.await.unwrap().is_err());
    }

    #[test]
    fn request_ids_are_unique_within_one_process() {
        let first = PendingApproval::now("command", "a", "a");
        let second = PendingApproval::now("command", "a", "a");
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn a_deny_is_not_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let scopes = Scopes::new(dir.path(), dir.path().join("index"));
        assert!(!CommandDecision::Deny.is_allowed());
        assert!(CommandDecision::ALLOW_ONCE.is_allowed());
        assert_eq!(scopes.granted("rm -rf build"), None);
    }
}

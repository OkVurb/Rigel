//! Rigel's core: storage, sessions, the bill, patches, memory and the native agent.
//!
//! The core owns all application state. Nothing in the renderer or in an adapter is
//! allowed to hold a session, a claim or a budget as authoritative; see
//! `docs/handoff.md` §3.2 (S2).

pub mod agent;
pub mod attribute;
pub mod budget;
pub mod claims;
pub mod comments;
pub mod credentials;
pub mod diagnostics;
pub mod domain;
pub mod forge;
pub mod memory;
pub mod merge;
pub mod migrate;
pub mod patches;
pub mod plan;
pub mod resume;
pub mod rules;
pub mod safety;
pub mod session;
pub mod skills;
pub mod snapshot;
pub mod spend;
pub mod storage;
pub mod toolchain;
pub mod tree;
pub mod update;
pub mod worktree;
pub use agent::{
    AgentError, ApprovalScope, Cancels, ChatMessage, ChatRequest, CommandApprover, CommandDecision,
    CommandRequest, Coordinator, DEFAULT_MAX_STEPS, Deliberation, Deliberator, Delta, Finding,
    Model, ModelDeliberator, ModelError, NoDeliberation, OpenRouterModel, OriginApprover,
    OriginDecision, OriginRequest, POLICIES, PlanAgent, PlanApprover, PlanDecision, PlanRequest,
    Policy, Position, Scopes, ScriptedModel, ServerStatus, StopReason, SubagentPolicy, ToolCall,
    ToolOutcome, ToolSpec, Tools, TurnRun, classify,
};
pub use attribute::{Attributed, ConflictWatcher, Observed, POLL_INTERVAL, Watcher};
pub use budget::{
    BudgetError, BudgetOutcome, BudgetService, BudgetSnapshot, DailyStatus, InterruptSink,
    ReconcileReport, SafetySettings, TurnQuote, DEFAULT_DAILY_CAP_MICROS, DEFAULT_WRITABLE_AGENTS,
    quote_turn, utc_day,
};
pub use claims::{
    CLAIM_TTL, Claim, ClaimConflict, ClaimError, ClaimResult, ClaimService, ConflictSides,
    ConflictText, MERGE_AUTHOR, MergeResolution, MergedConflict, STALE_AFTER, UnverifiedChange,
};
pub use comments::{CommentError, HunkComment, HunkComments};
pub use domain::{AgentEvent, CostSource, EventSink, ItemKind, NullSink, Usage};
pub use memory::{MemoryError, MemoryFile, MemoryService, WriteMode};
pub use merge::{MergeError, MergeResult, three_way};
pub use migrate::{INDEX_MIGRATIONS, MigrateError, Migration, PROJECT_MIGRATIONS, apply, now_ms};
pub use patches::{
    AttributedHunk, HunkHeader, Patch, PatchError, PatchPipeline, Recovery, RecoveryState, StopAt,
    apply_patch_text, parse_diff_hunks, reverse_patch,
};
pub use resume::{ResumeError, ResumeOutcome};
pub use safety::{
    AppliedReview, DecisionError, MergeReview, MergeReviews, Verdict, apply_review, diff_sha,
};
pub use session::{
    Agent, Checkpoint, Item, Session, SessionError, SessionService, SessionStatus, TeamAgent, Turn,
    TurnTranscript, WritableAgent,
};
pub use skills::{
    Capability, Evidence, EvidenceSummary, GitHubSource, IndexStats, IngestReport, SkillDetail,
    SkillIndex, SkillsError, Source, SourceState, SourceStatus, SyncPolicy, VariantRow,
};
pub use snapshot::{
    FileEntry, MAX_SNAPSHOT_BYTES, MAX_SNAPSHOT_FILES, Manifest, RestoreReport, Snapshot,
    SnapshotError, SnapshotKind, SnapshotService,
};
pub use storage::{StorageError, Store, default_index_dir};
pub use toolchain::{
    Decision, Environment, Host, InstallPlan, InstallRecord, Manager, Pin, Requirement,
    RequirementSource, RunOutcome, Runner, SourceKind, SystemEnvironment, SystemRunner, TOOLS,
    ToolError, ToolService, ToolStatus, TrustFactor, TrustReport, requirements, tool_by_alias,
    tool_by_id, trust_for,
};
pub use update::{
    PUBKEY_FILE as UPDATE_PUBKEY_FILE, UpdateCheck, UpdateError, UpdateManifest, UpdateSettings,
};
pub use worktree::{
    MergeLine, MergeReport, MergeStatus, WORKTREES_DIR, Worktree, WorktreeError, WorktreeService,
};

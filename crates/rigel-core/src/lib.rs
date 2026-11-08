//! Rigel's core: storage, sessions, the bill, patches, memory and the native agent.
//!
//! The core owns all application state. Nothing in the renderer or in an adapter is
//! allowed to hold a session, a claim or a budget as authoritative; see
//! `docs/handoff.md` §3.2 (S2).

pub mod agent;
pub mod agentmail;
pub mod attention;
pub mod attribute;
pub mod budget;
pub mod claims;
pub mod cloud;
pub mod comments;
pub mod credentials;
pub mod diagnostics;
pub mod domain;
pub mod forge;
pub mod gravity;
pub mod mcp;
pub mod memory;
pub mod merge;
pub mod migrate;
pub mod patches;
pub mod pins;
pub mod plan;
pub mod process;
pub mod provenance;
pub mod redaction;
pub mod resume;
pub mod rules;
pub mod safety;
pub mod search;
pub mod session;
pub mod skills;
pub mod snapshot;
pub mod spend;
pub mod storage;
pub mod store;
pub mod swarm;
pub mod templates;
pub mod testrun;
pub mod toolchain;
pub mod tree;
pub mod update;
pub mod worktree;
pub use agent::{
    AgentError, ApprovalScope, Cancels, ChatMessage, ChatRequest, CommandApprover, CommandDecision,
    CommandRequest, Coordinator, DEFAULT_MAX_STEPS, Deliberation, Deliberator, Delta, Finding,
    Model, ModelDeliberator, ModelError, NetworkPolicy, NoDeliberation, OpenRouterModel,
    OriginApprover, OriginDecision, OriginRequest, POLICIES, PlanAgent, PlanApprover, PlanDecision,
    PlanRequest, Policy, Position, Scopes, ScriptedModel, ServerStatus, SkillScript, StopReason,
    SubagentPolicy, ToolCall, ToolOutcome, ToolSpec, Tools, TurnRun, classify,
};
pub use agentmail::{
    Mail, MailConfig, MailError, MailStatus, MessageBody, MessageSummary, SentMail, ThreadSummary,
    ThreadView,
};
pub use attention::{Attention, AttentionGroup, AttentionItem};
pub use attribute::{Attributed, ConflictWatcher, Observed, POLL_INTERVAL, Watcher};
pub use budget::{
    BudgetError, BudgetOutcome, BudgetService, BudgetSnapshot, DEFAULT_DAILY_CAP_MICROS,
    DEFAULT_WRITABLE_AGENTS, DailyStatus, InterruptSink, ReconcileReport, SafetySettings,
    TurnQuote, quote_turn, utc_day,
};
pub use claims::{
    CLAIM_TTL, Claim, ClaimConflict, ClaimError, ClaimResult, ClaimService, ConflictSides,
    ConflictText, MERGE_AUTHOR, MergeResolution, MergedConflict, STALE_AFTER, UnverifiedChange,
};
pub use cloud::{
    Cloud, CloudConfig, CloudError, CloudStatus, Endpoints, MemorySessionStore, PullReport,
    PushReport, SessionStore,
};
pub use comments::{CommentError, HunkComment, HunkComments};
pub use domain::{AgentEvent, CostSource, EventSink, ItemKind, NullSink, Usage};
pub use memory::{
    DenyMemory, MemoryApprover, MemoryDecision, MemoryError, MemoryFile, MemoryRequest,
    MemoryService, WriteMode,
};
pub use merge::{MergeError, MergeResult, three_way};
pub use migrate::{INDEX_MIGRATIONS, MigrateError, Migration, PROJECT_MIGRATIONS, apply, now_ms};
pub use patches::{
    AttributedHunk, HunkHeader, Patch, PatchError, PatchPipeline, Recovery, RecoveryState, StopAt,
    apply_patch_text, parse_diff_hunks, reverse_patch,
};
pub use pins::{PinError, SkillPinReview, SkillPinView};
pub use provenance::{Provenance, ProvenanceError};
pub use resume::{ResumeError, ResumeOutcome};
pub use safety::{
    AppliedReview, DecisionError, MergeReview, MergeReviews, Verdict, apply_review, diff_sha,
};
pub use search::{MAX_HITS, PatchHit, SessionHit, TurnHit, WorkspaceHits};
pub use session::{
    Agent, Bookmark, Checkpoint, Item, RecoveryOffer, Session, SessionError, SessionService,
    SessionStatus, SkillPin, SubagentGraph, TeamAgent, Tombstone, Turn, TurnTranscript,
    WritableAgent,
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
pub use templates::{
    MAX_TEMPLATES, SessionTemplate, SessionTemplates, TemplateAgent, TemplateError, TemplateSkill,
};
pub use testrun::{TestPlan, TestRun, TestRunError};
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

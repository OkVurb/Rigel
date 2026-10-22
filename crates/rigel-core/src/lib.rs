//! Rigel's core: storage, sessions, the bill, patches, memory and the native agent.
//!
//! The core owns all application state. Nothing in the renderer or in an adapter is
//! allowed to hold a session, a claim or a budget as authoritative; see
//! `docs/handoff.md` §3.2 (S2).

pub mod agent;
pub mod budget;
pub mod credentials;
pub mod domain;
pub mod memory;
pub mod migrate;
pub mod patches;
pub mod resume;
pub mod session;
pub mod skills;
pub mod snapshot;
pub mod storage;
pub mod tree;
pub use agent::{
    AgentError, ApprovalScope, Cancels, ChatMessage, ChatRequest, CommandApprover, CommandDecision,
    CommandRequest, Coordinator, DEFAULT_MAX_STEPS, Deliberation, Deliberator, Delta, Finding,
    Model, ModelDeliberator, ModelError, NoDeliberation, OpenRouterModel, OriginApprover,
    OriginDecision, OriginRequest, POLICIES, Policy, Position, Scopes, ScriptedModel, ServerStatus,
    StopReason, SubagentPolicy, ToolCall, ToolOutcome, ToolSpec, Tools, TurnRun, classify,
};
pub use budget::{
    BudgetError, BudgetOutcome, BudgetService, BudgetSnapshot, InterruptSink, ReconcileReport,
    TurnQuote, quote_turn,
};
pub use domain::{AgentEvent, CostSource, EventSink, ItemKind, NullSink, Usage};
pub use memory::{MemoryError, MemoryFile, MemoryService, WriteMode};
pub use migrate::{INDEX_MIGRATIONS, MigrateError, Migration, PROJECT_MIGRATIONS, apply, now_ms};
pub use patches::{Patch, PatchError, PatchPipeline, Recovery, RecoveryState, StopAt};
pub use resume::{ResumeError, ResumeOutcome};
pub use session::{
    Agent, Checkpoint, Item, Session, SessionError, SessionService, SessionStatus, Turn,
    TurnTranscript,
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

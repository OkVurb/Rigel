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
pub mod storage;
pub mod tree;

pub use agent::{
    AgentError, Cancels, ChatMessage, ChatRequest, Coordinator, DEFAULT_MAX_STEPS, Delta, Model,
    ModelError, OpenRouterModel, ScriptedModel, StopReason, SubagentPolicy, ToolCall, ToolOutcome,
    ToolSpec, Tools, TurnRun,
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
pub use storage::{StorageError, Store, default_index_dir};

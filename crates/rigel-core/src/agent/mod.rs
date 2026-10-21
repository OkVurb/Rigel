//! The native agent: a provider-agnostic model loop, the tools it may call, and the bill.
//!
//! | file | owns |
//! |---|---|
//! | [`model`] | the `Model` trait, streamed deltas, tool-call assembly, SSE decoding |
//! | [`openrouter`] | one real provider, retries and `Retry-After` |
//! | [`omniroute`] | the local router, its health probe and autostart |
//! | [`scripted`] | a deterministic model for tests and the offline QA run |
//! | [`pricing`] | the local price fallback, marked `estimated` |
//! | [`tools`] | read/list/edit/write, skills, budget — all path-checked |
//! | [`coordinator`] | the turn loop, cancellation, checkpointing |
//!
//! No adapter will speak this trait. Phase 4's harnesses have their own protocols and are
//! normalised into the same `AgentEvent`s; the coordinator is for the agent Rigel runs
//! itself (D19).

pub mod coordinator;
pub mod model;
pub mod omniroute;
pub mod openrouter;
pub mod pricing;
pub mod scripted;
pub mod tools;

pub use coordinator::{
    AgentError, Cancels, Coordinator, DEFAULT_MAX_STEPS, DEFAULT_SUBAGENT_MODEL, SUBAGENT_MODELS,
    StopReason, SubagentPolicy, TurnRun, fixed_context_chars,
};
pub use model::{
    ChatMessage, ChatRequest, Delta, DeltaSink, Model, ModelError, RawUsage, SseDecoder, ToolCall,
    ToolCallBuilder, ToolSpec,
};
pub use omniroute::ServerStatus;
pub use openrouter::{DEFAULT_BASE_URL, DEFAULT_MODEL, OpenRouterModel};
pub use scripted::{Script, ScriptedModel};
pub use tools::{ToolOutcome, Tools};

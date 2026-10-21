//! The stdio MCP server agents connect to.
//!
//! Phase 1 fixes the tool names so the adapters and the UI can reference them before
//! the handlers exist. Everything here returns data owned by `rigel-core`; this crate
//! never touches SQLite directly (handoff §14.2).

pub const SERVER_NAME: &str = "rigel";

/// Tools exposed to every spawned agent. Order is the order the docs list them in.
pub const TOOL_NAMES: &[&str] = &[
    "rigel_claim",
    "rigel_release",
    "rigel_who",
    "rigel_skills_search",
    "rigel_skill_load",
    "rigel_patch_propose",
    "rigel_budget",
    "rigel_memory_read",
    "rigel_memory_write",
];

pub fn tool_names() -> &'static [&'static str] {
    TOOL_NAMES
}

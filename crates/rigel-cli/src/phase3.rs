//! Phase 3's CLI surface: the native agent, patches, memory, keys and the MCP server.
//!
//! The command definitions live here; the handlers live in `agent` and `ops` so no single
//! file has to hold all of it.

use std::path::PathBuf;

use clap::Subcommand;

mod agent;
mod ops;

pub use agent::run_agent;
pub use ops::{run_keys, run_mcp, run_memory, run_patch};

#[derive(Subcommand)]
pub enum AgentCommand {
    /// Run one turn and print what it did.
    Run {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        /// Session id. Defaults to the newest active session, starting one if there is none.
        #[arg(long)]
        session: Option<String>,
        /// Role for the native agent. One is created on first use.
        #[arg(long, default_value = "native")]
        agent: String,
        #[arg(long)]
        prompt: String,
        /// Deterministic model script instead of OpenRouter (see docs/qa/p3.md).
        #[arg(long, value_name = "FILE")]
        script: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
        /// Stop after the patch journal is written, before the rename (crash test).
        #[arg(long)]
        crash_after_journal: bool,
        #[arg(long, default_value_t = rigel_core::DEFAULT_MAX_STEPS)]
        max_steps: usize,
        #[arg(long)]
        json: bool,
    },

    /// Agents in a session, with their spend.
    List {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum PatchCommand {
    List {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Apply {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        json: bool,
    },
    Revert {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum MemoryCommand {
    Read {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        agent: String,
        #[arg(long, default_value = "memory")]
        topic: String,
        #[arg(long)]
        json: bool,
    },
    Write {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        agent: String,
        #[arg(long, default_value = "memory")]
        topic: String,
        /// Content. Read from stdin when omitted.
        #[arg(long)]
        text: Option<String>,
        #[arg(long, default_value = "append")]
        mode: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum KeyCommand {
    /// Which providers have a key. Never prints the key.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Store a key, read from stdin.
    Set {
        provider: String,
    },
    Delete {
        provider: String,
    },
}

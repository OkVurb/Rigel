//! `rigel` — the CLI half of the app.
//!
//! Phase 1 ships only what the migration work needs: `doctor`, which will grow one check
//! per phase, and `migrate`, which is how a project database gets created without
//! launching the window.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rigel_core::Store;
use serde_json::json;

#[derive(Parser)]
#[command(name = "rigel", version, about = "Rigel — an agent workspace")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Report local state. Empty in Phase 1; one check lands per phase.
    Doctor {
        #[arg(long)]
        json: bool,
    },

    /// Create or migrate a project's session database.
    Migrate {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Command::Doctor { json: as_json } => {
            if as_json {
                let report = json!({
                    "ok": true,
                    "version": env!("CARGO_PKG_VERSION"),
                    "server": rigel_mcp_name(),
                    "checks": [],
                });
                println!("{report}");
            } else {
                println!("rigel {}", env!("CARGO_PKG_VERSION"));
                println!("  checks: none yet (Phase 1)");
            }
            ExitCode::SUCCESS
        }

        Command::Migrate { project } => match Store::open_project(&project).await {
            Ok(store) => {
                println!("{}", store.path().display());
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("rigel: {error}");
                ExitCode::from(2)
            }
        },
    }
}

/// Kept as a function so `doctor` does not need the MCP crate as a dependency yet.
fn rigel_mcp_name() -> &'static str {
    "rigel"
}

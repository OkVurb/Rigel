//! `rigel audit`: verify the local approval chain and read command provenance.
//!
//! Both commands are read-only. `verify` deliberately does not need the database: a ledger
//! copied out of the project can be checked on its own.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use rigel_core::audit::{self, Link};
use serde::Serialize;

#[derive(Subcommand)]
pub enum AuditCommand {
    /// Verify the hash chain of `.rigel/audit/approvals.jsonl`.
    Verify {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        json: bool,
    },

    /// Show one command item's turn, model step, tool call, approval and snapshot.
    Provenance {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        /// The command item id from the transcript.
        #[arg(long)]
        item: String,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(command: AuditCommand) -> ExitCode {
    match command {
        AuditCommand::Verify { project, json } => match audit::verify(&project) {
            Ok(report) => {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
                    );
                } else if report.valid {
                    println!(
                        "audit chain: {} approval(s), intact ({})",
                        report.records,
                        report.path.display()
                    );
                } else if let Some(bad) = &report.first_bad {
                    println!("audit chain: tampered at line {}: {}", bad.line, bad.reason);
                    println!("  {}", report.path.display());
                }
                if report.valid {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(2)
                }
            }
            Err(error) => {
                eprintln!("rigel: could not verify the audit chain: {error}");
                ExitCode::from(2)
            }
        },

        AuditCommand::Provenance {
            project,
            item,
            json,
        } => {
            let service = match rigel_core::SessionService::open(&project).await {
                Ok(service) => service,
                Err(error) => {
                    eprintln!("rigel: {error}");
                    return ExitCode::from(2);
                }
            };
            match audit::provenance::command(service.pool(), &item).await {
                Ok(chain) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&chain)
                                .unwrap_or_else(|_| "{}".to_string())
                        );
                    } else {
                        println!("item {}", chain.item_id);
                        link("command", &chain.command);
                        link("turn", &chain.turn);
                        link("agent", &chain.agent);
                        link("model", &chain.model);
                        link("step", &chain.step);
                        link("tool call", &chain.tool_call);
                        link("approval", &chain.approval);
                        link("snapshot", &chain.snapshot);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("rigel: {error}");
                    ExitCode::from(2)
                }
            }
        }
    }
}

fn link<T: Serialize>(label: &str, link: &Link<T>) {
    match &link.value {
        Some(value) => println!(
            "  {label}: {}",
            serde_json::to_string(value).unwrap_or_else(|_| "?".to_string())
        ),
        None => println!(
            "  {label}: unknown — {}",
            link.reason.as_deref().unwrap_or("no reason recorded")
        ),
    }
}

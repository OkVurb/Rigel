//! `rigel` — the CLI half of the app.
//!
//! Everything here goes through the same core the window uses, which is what makes the
//! Phase 1 exit test meaningful: two separate processes, one database, one session.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rigel_core::{SessionService, Store};
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

    /// Session operations for one project.
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
}

#[derive(Subcommand)]
enum SessionCommand {
    /// Start a session and print its id.
    Start {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        budget_cents: Option<i64>,
    },

    /// List sessions, newest first.
    List {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        json: bool,
    },

    /// End a session.
    End {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        id: String,
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
                    "server": "rigel",
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

        Command::Session { command } => run_session(command).await,
    }
}

async fn run_session(command: SessionCommand) -> ExitCode {
    match command {
        SessionCommand::Start {
            project,
            title,
            budget_cents,
        } => match SessionService::open(&project).await {
            Ok(service) => match service.start(title.as_deref(), budget_cents).await {
                Ok(session) => {
                    println!("{}", session.id);
                    ExitCode::SUCCESS
                }
                Err(error) => fail(error),
            },
            Err(error) => fail(error),
        },

        SessionCommand::List {
            project,
            json: as_json,
        } => match SessionService::open(&project).await {
            Ok(service) => match service.list().await {
                Ok(sessions) => {
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&sessions).unwrap_or_default()
                        );
                    } else if sessions.is_empty() {
                        println!("no sessions");
                    } else {
                        for session in sessions {
                            println!(
                                "{}  {:<11}  {:<13}  {}",
                                &session.id[..8.min(session.id.len())],
                                format!("{:?}", session.status).to_lowercase(),
                                session.created_at,
                                session.title.unwrap_or_default()
                            );
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => fail(error),
            },
            Err(error) => fail(error),
        },

        SessionCommand::End { project, id } => match SessionService::open(&project).await {
            Ok(service) => match service.end(&id).await {
                Ok(session) => {
                    println!("{}", session.id);
                    ExitCode::SUCCESS
                }
                Err(error) => fail(error),
            },
            Err(error) => fail(error),
        },
    }
}

fn fail(error: impl std::fmt::Display) -> ExitCode {
    eprintln!("rigel: {error}");
    ExitCode::from(2)
}

//! `rigel worktree` — a private checkout per agent, and the merge back (P5.5).
//!
//! The CLI is the surface the app and a harness both use. It deliberately has no `--force`
//! merge: a conflict is reported, and the resolution is a person choosing which file to keep.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use rigel_core::{SessionService, WorktreeService};
use serde_json::json;

#[derive(Subcommand)]
pub enum WorktreeCommand {
    /// Cut a detached worktree at HEAD for one agent.
    Create {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        json: bool,
    },

    /// Every file the agent changed, without touching the project.
    Preview {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        json: bool,
    },

    /// Move the agent's changes into the project through the patch pipeline.
    Merge {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        json: bool,
    },

    /// Remove the checkout. A dirty one needs `--force`.
    Remove {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },

    /// The checkouts under the data directory.
    List {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run(dir: &std::path::Path, command: WorktreeCommand) -> ExitCode {
    match command {
        WorktreeCommand::Create {
            project,
            agent,
            json: as_json,
        } => {
            let (trees, _service) = match open(&project, dir).await {
                Ok(pair) => pair,
                Err(code) => return code,
            };
            match trees.create(&agent).await {
                Ok(worktree) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": true,
                                "agent": worktree.agent_id,
                                "path": worktree.path,
                                "base": worktree.base,
                            })
                        );
                    } else {
                        println!("{}", worktree.path.display());
                        println!("  base    {}", worktree.base);
                        println!("  agent   {}", worktree.agent_id);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        WorktreeCommand::Preview {
            project,
            agent,
            json: as_json,
        } => {
            let (trees, _service) = match open(&project, dir).await {
                Ok(pair) => pair,
                Err(code) => return code,
            };
            match trees.preview(&agent).await {
                Ok(changes) => {
                    if as_json {
                        let rows: Vec<_> = changes
                            .iter()
                            .map(|(path, status)| json!({ "path": path, "status": status }))
                            .collect();
                        println!("{}", json!({ "ok": true, "agent": agent, "changes": rows }));
                    } else if changes.is_empty() {
                        println!("no changes in {agent}'s worktree");
                    } else {
                        for (path, status) in &changes {
                            println!("{status:<12} {path}");
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        WorktreeCommand::Merge {
            project,
            agent,
            json: as_json,
        } => {
            let (trees, service) = match open(&project, dir).await {
                Ok(pair) => pair,
                Err(code) => return code,
            };
            let session = match service.agent(&agent).await {
                Ok(row) => row.session_id,
                Err(error) => {
                    eprintln!("rigel: {error}");
                    return ExitCode::from(1);
                }
            };
            match trees.merge_back(&session, &agent).await {
                Ok(report) => {
                    if as_json {
                        println!("{}", json!({ "ok": report.clean(), "report": report }));
                    } else if report.lines.is_empty() {
                        println!("nothing to merge from {agent}");
                    } else {
                        for line in &report.lines {
                            print!(
                                "{:<12} {}",
                                format!("{:?}", line.status).to_lowercase(),
                                line.path
                            );
                            if let Some(detail) = &line.detail {
                                print!("  — {detail}");
                            }
                            println!();
                        }
                        println!(
                            "\n  {} applied · {} conflicted · {} skipped",
                            report.applied, report.conflicted, report.skipped
                        );
                    }
                    if report.clean() {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::from(1)
                    }
                }
                Err(error) => failure(error, as_json),
            }
        }

        WorktreeCommand::Remove {
            project,
            agent,
            force,
            json: as_json,
        } => {
            let (trees, _service) = match open(&project, dir).await {
                Ok(pair) => pair,
                Err(code) => return code,
            };
            match trees.remove(&agent, force).await {
                Ok(()) => {
                    if as_json {
                        println!("{}", json!({ "ok": true, "removed": agent }));
                    } else {
                        println!("removed {agent}'s worktree");
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    if !force {
                        eprintln!(
                            "hint: the checkout has unmerged changes; `rigel worktree merge` first, \
                             or `--force` to discard them"
                        );
                    }
                    failure(error, as_json)
                }
            }
        }

        WorktreeCommand::List {
            project,
            json: as_json,
        } => {
            let (trees, _service) = match open(&project, dir).await {
                Ok(pair) => pair,
                Err(code) => return code,
            };
            match trees.list() {
                Ok(paths) => {
                    if as_json {
                        println!("{}", json!({ "ok": true, "worktrees": paths }));
                    } else if paths.is_empty() {
                        println!("no worktrees");
                    } else {
                        for path in &paths {
                            println!("{}", path.display());
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }
    }
}

async fn open(
    project: &std::path::Path,
    dir: &std::path::Path,
) -> Result<(WorktreeService, SessionService), ExitCode> {
    let service = match SessionService::open(project).await {
        Ok(service) => service,
        Err(error) => {
            eprintln!("rigel: {error}");
            return Err(ExitCode::from(2));
        }
    };
    let trees = WorktreeService::new(project, dir, service.pool().clone());
    Ok((trees, service))
}

fn failure(error: rigel_core::WorktreeError, as_json: bool) -> ExitCode {
    if as_json {
        println!("{}", json!({ "ok": false, "error": error.to_string() }));
    }
    eprintln!("rigel: {error}");
    ExitCode::from(1)
}

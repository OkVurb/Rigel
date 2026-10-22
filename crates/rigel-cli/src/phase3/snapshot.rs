//! `rigel snapshot` — the pre-destructive copy of uncommitted work (D38).
//!
//! The snapshots live under the app data dir, not the project: they are not part of the work
//! tree and must not be indexed, patched or committed. `restore` refuses a different project,
//! because the manifest records absolute roots.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use rigel_core::SnapshotService;
use serde_json::json;

#[derive(Subcommand)]
pub enum SnapshotCommand {
    /// Take a snapshot now, as the agent does before a destructive command.
    Capture {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        json: bool,
    },

    /// List snapshots, newest first.
    List {
        #[arg(long)]
        json: bool,
    },

    /// Put a snapshot back into the project it was taken from.
    Restore {
        id: String,
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

pub fn run_snapshot(command: SnapshotCommand, data_dir: PathBuf) -> ExitCode {
    match command {
        SnapshotCommand::Capture {
            project,
            json: as_json,
        } => {
            let root = canonical(&project, as_json);
            let Some(root) = root else {
                return ExitCode::from(1);
            };
            let snapshots = SnapshotService::new(root, data_dir);
            match snapshots.capture("manual") {
                Ok(snapshot) => {
                    if as_json {
                        println!("{}", json!({ "ok": true, "snapshot": snapshot }));
                    } else {
                        println!(
                            "snapshot {}  {:?}  {} files, {} bytes\n  {}",
                            snapshot.id,
                            snapshot.kind,
                            snapshot.file_count,
                            snapshot.total_bytes,
                            snapshot.directory.display()
                        );
                        println!("  {}", snapshot.note);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => fail(error, as_json),
            }
        }

        SnapshotCommand::List { json: as_json } => {
            let snapshots = SnapshotService::new(".", data_dir);
            match snapshots.list() {
                Ok(manifests) => {
                    if as_json {
                        println!("{}", json!({ "ok": true, "snapshots": manifests }));
                    } else if manifests.is_empty() {
                        println!("no snapshots yet");
                    } else {
                        for manifest in manifests {
                            let stash = manifest
                                .stash
                                .as_deref()
                                .map(|sha| &sha[..8.min(sha.len())])
                                .unwrap_or("-");
                            println!(
                                "{}  {:?}  {} files  {} bytes  stash {stash}  {}",
                                manifest.id,
                                manifest.kind,
                                manifest.files.len(),
                                manifest.total_bytes,
                                manifest.root
                            );
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => fail(error, as_json),
            }
        }

        SnapshotCommand::Restore {
            id,
            project,
            json: as_json,
        } => {
            let root = canonical(&project, as_json);
            let Some(root) = root else {
                return ExitCode::from(1);
            };
            let snapshots = SnapshotService::new(root, data_dir);
            match snapshots.restore(&id, &project) {
                Ok(report) => {
                    if as_json {
                        println!("{}", json!({ "ok": true, "restore": report }));
                    } else {
                        println!(
                            "restored {}  {:?}  {} copied files{}",
                            report.id,
                            report.kind,
                            report.files_restored,
                            if report.git_restored {
                                "  (tracked changes from the stash)"
                            } else {
                                ""
                            }
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => fail(error, as_json),
            }
        }
    }
}

fn canonical(project: &std::path::Path, as_json: bool) -> Option<PathBuf> {
    match project.canonicalize() {
        Ok(root) => Some(root),
        Err(error) => {
            if as_json {
                println!("{}", json!({ "ok": false, "error": error.to_string() }));
            }
            eprintln!("rigel: {}: {error}", project.display());
            None
        }
    }
}

fn fail(error: rigel_core::SnapshotError, as_json: bool) -> ExitCode {
    if as_json {
        println!("{}", json!({ "ok": false, "error": error.to_string() }));
    }
    eprintln!("rigel: {error}");
    ExitCode::from(1)
}

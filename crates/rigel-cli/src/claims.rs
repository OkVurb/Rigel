//! `rigel claim` — the read side of the ledger, and a way to release a claim by hand (P5.7).
//!
//! Claiming is something agents do through MCP. A human uses this to see who said they would
//! edit what, and to release a claim a crashed agent left behind. The language is deliberate:
//! "claimed", "stale", "unverified" — never "locked", because nothing is (N1, D5).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use rigel_core::{Claim, ClaimService, SessionService};
use serde_json::json;

#[derive(Subcommand)]
pub enum ClaimCommand {
    /// Who has claimed what, and which claimed files changed without their author.
    List {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        /// One path or a directory prefix.
        #[arg(long)]
        path: Option<String>,
        /// Include released claims: the audit trail, not just the live picture.
        #[arg(long)]
        history: bool,
        #[arg(long)]
        json: bool,
    },

    /// Release a claim. The row stays as history, with the reason attached.
    Release {
        path: String,
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        /// Only release this agent's claim; by default the current holder is released.
        #[arg(long)]
        agent: Option<String>,
        #[arg(long, default_value = "released by hand")]
        reason: String,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run_claim(command: ClaimCommand) -> ExitCode {
    match command {
        ClaimCommand::List {
            project,
            path,
            history,
            json: as_json,
        } => list(&project, path.as_deref(), history, as_json).await,
        ClaimCommand::Release {
            path,
            project,
            agent,
            reason,
            json: as_json,
        } => release(&project, &path, agent.as_deref(), &reason, as_json).await,
    }
}

async fn open(project: &std::path::Path) -> Result<(ClaimService, SessionService), String> {
    let root = std::fs::canonicalize(project)
        .map_err(|error| format!("{}: {error}", project.display()))?;
    let service = SessionService::open(&root)
        .await
        .map_err(|error| error.to_string())?;
    let claims = ClaimService::new(service.pool().clone(), &root);
    Ok((claims, service))
}

async fn list(
    project: &std::path::Path,
    path: Option<&str>,
    history: bool,
    as_json: bool,
) -> ExitCode {
    let (claims, _service) = match open(project).await {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("rigel: {error}");
            return ExitCode::from(2);
        }
    };
    let rows = if history {
        claims.history(path).await
    } else {
        claims.held(path).await
    };
    let unverified = claims.unverified().await.unwrap_or_default();
    let rows = match rows {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("rigel: {error}");
            return ExitCode::from(2);
        }
    };

    if as_json {
        println!(
            "{}",
            json!({
                "ok": true,
                "claims": rows,
                "unverified_changes": unverified,
            })
        );
        return ExitCode::SUCCESS;
    }
    if rows.is_empty() {
        println!("no claims");
    }
    for claim in &rows {
        print_claim(claim);
    }
    if !unverified.is_empty() {
        println!();
        for change in &unverified {
            println!(
                "  changed without the claimant  {}  (claimed by {}){}",
                change.path,
                change.claimant.as_deref().unwrap_or("nobody"),
                if change.ambiguous {
                    "  — more than one agent could have written it"
                } else {
                    ""
                }
            );
        }
    }
    ExitCode::SUCCESS
}

fn print_claim(claim: &Claim) {
    let state = match (claim.released_at, claim.stale) {
        (Some(_), _) => "released",
        (None, true) => "stale",
        (None, false) => "claimed",
    };
    let name = if claim.agent.is_empty() {
        claim.agent_id.as_str()
    } else {
        claim.agent.as_str()
    };
    println!(
        "  {state:<8} {:<28} {name} ({}){}",
        claim.path,
        claim.harness,
        match &claim.release_reason {
            Some(reason) => format!("  — {reason}"),
            None => String::new(),
        }
    );
}

async fn release(
    project: &std::path::Path,
    path: &str,
    agent: Option<&str>,
    reason: &str,
    as_json: bool,
) -> ExitCode {
    let (claims, _service) = match open(project).await {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("rigel: {error}");
            return ExitCode::from(2);
        }
    };
    // A human naming a path means "whoever has it": find the holder when no agent was named.
    let holder = match agent {
        Some(agent) => Some(agent.to_string()),
        None => match claims.held(Some(path)).await {
            Ok(held) => held.first().map(|claim| claim.agent_id.clone()),
            Err(error) => {
                eprintln!("rigel: {error}");
                return ExitCode::from(2);
            }
        },
    };
    let Some(holder) = holder else {
        if as_json {
            println!(
                "{}",
                json!({ "ok": false, "error": "no claim on that path" })
            );
        } else {
            eprintln!("rigel: no claim on {path}");
        }
        return ExitCode::from(1);
    };
    let canonical = match claims.normalize(path) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("rigel: {error}");
            return ExitCode::from(1);
        }
    };
    match claims.release(&holder, &[canonical.clone()], reason).await {
        Ok(released) if released.is_empty() => {
            if as_json {
                println!(
                    "{}",
                    json!({ "ok": false, "error": "no claim on that path" })
                );
            } else {
                eprintln!("rigel: no claim on {path}");
            }
            ExitCode::from(1)
        }
        Ok(released) => {
            if as_json {
                println!(
                    "{}",
                    json!({ "ok": true, "released": released, "agent": holder, "reason": reason })
                );
            } else {
                println!("released {canonical} (held by {holder}): {reason}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("rigel: {error}");
            ExitCode::from(2)
        }
    }
}

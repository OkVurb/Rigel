//! `rigel tools` and `rigel trust` — the Phase 6 surface.
//!
//! Two rules shape every command here:
//!
//! * A requirement always prints the line in the skill that asked for it. A list of packages
//!   with no provenance is a suggestion, and suggestions do not install anything.
//! * An install prints the exact command before it is approved, and the trust arithmetic
//!   before the question. `--yes` is the only way to skip the question, and it is not the
//!   default anywhere. `--unattended` never runs: it queues (D13).

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Subcommand;
use rigel_core::{
    Host, Pin, SkillIndex, SystemRunner, ToolService, TrustReport, requirements, tool_by_alias,
    trust_for,
};
use serde_json::json;

#[derive(Subcommand)]
pub enum ToolCommand {
    /// Probe every known tool on one host and refresh the cache.
    List {
        #[arg(long)]
        host: Option<String>,
        /// Ignore a still-fresh cache entry.
        #[arg(long)]
        refresh: bool,
        #[arg(long)]
        json: bool,
    },

    /// Show one tool: whether it is present, its version, its path and its hash.
    Which {
        tool: String,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// What a skill needs, and the line that said so.
    Requirements {
        /// A skill id from the index. Omit when using `--file`.
        id: Option<String>,
        #[arg(long, value_name = "FILE")]
        file: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },

    /// Compare a tool's binary with the hash Rigel recorded for it.
    Verify {
        tool: String,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// Re-probe after an install; the agent gets this to confirm a new binary.
    Restart {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// Installs that ran, were refused, or are waiting for someone.
    Ledger {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },

    /// Installs queued because nobody was there to approve them.
    Queued {
        #[arg(long)]
        json: bool,
    },

    /// Install a tool after showing the exact command.
    Install {
        tool: String,
        #[arg(long)]
        host: Option<String>,
        /// Approve without a prompt. The command is still printed first.
        #[arg(long)]
        yes: bool,
        /// Queue instead of running. Never executes a network install (D13).
        #[arg(long)]
        unattended: bool,
        /// Run a package the trust arithmetic blocked. Recorded as user-approved.
        #[arg(long)]
        allow_low_trust: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum TrustCommand {
    /// Print the arithmetic behind a package's trust score.
    Explain {
        package: String,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run_tools(dir: &std::path::Path, command: ToolCommand) -> ExitCode {
    let service = match ToolService::open(dir).await {
        Ok(service) => service,
        Err(error) => {
            eprintln!("rigel: {error}");
            return ExitCode::from(2);
        }
    };

    match command {
        ToolCommand::List {
            host,
            refresh,
            json: as_json,
        } => {
            let host = host
                .map(|value| Host::parse(&value))
                .unwrap_or_else(Host::current);
            match service.detect(&host, refresh).await {
                Ok(statuses) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({ "ok": true, "host": host.key(), "tools": statuses })
                        );
                    } else {
                        let width = statuses.iter().map(|row| row.id.len()).max().unwrap_or(4);
                        for row in &statuses {
                            if row.present {
                                println!(
                                    "{:<width$}  {:<28}  {}",
                                    row.id,
                                    row.version.clone().unwrap_or_default(),
                                    row.path
                                        .as_ref()
                                        .map(|path| path.display().to_string())
                                        .unwrap_or_default(),
                                    width = width
                                );
                            } else {
                                println!("{:<width$}  missing", row.id, width = width);
                            }
                        }
                        let missing = statuses.iter().filter(|row| !row.present).count();
                        println!(
                            "\n  host {}: {} present, {} missing",
                            host.key(),
                            statuses.len() - missing,
                            missing
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        ToolCommand::Which {
            tool,
            host,
            json: as_json,
        } => {
            let host = host
                .map(|value| Host::parse(&value))
                .unwrap_or_else(Host::current);
            let Some(spec) = tool_by_alias(&tool) else {
                eprintln!("rigel: {tool} is not in the curated tool registry");
                return ExitCode::from(1);
            };
            match service.detect(&host, false).await {
                Ok(statuses) => {
                    let row = statuses.into_iter().find(|row| row.id == spec.id);
                    let present = row.as_ref().is_some_and(|row| row.present);
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": true,
                                "host": host.key(),
                                "tool": spec.id,
                                "present": present,
                                "status": row,
                            })
                        );
                    } else if let Some(row) = row.filter(|row| row.present) {
                        println!("{:<10} {}", spec.id, row.version.unwrap_or_default());
                        println!(
                            "  path      {}",
                            row.path
                                .as_ref()
                                .map(|path| path.display().to_string())
                                .unwrap_or_default()
                        );
                        println!(
                            "  sha256    {}",
                            row.sha256.unwrap_or_else(|| "not hashed".to_string())
                        );
                    } else {
                        println!("{} is not installed on {}", spec.id, host.key());
                        println!("  install   rigel tools install {}", spec.id);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        ToolCommand::Requirements {
            id,
            file,
            json: as_json,
        } => {
            let (source_name, body) = match (id, file) {
                (_, Some(file)) => match std::fs::read_to_string(&file) {
                    Ok(body) => (file.display().to_string(), body),
                    Err(error) => {
                        eprintln!("rigel: could not read {}: {error}", file.display());
                        return ExitCode::from(2);
                    }
                },
                (Some(id), None) => {
                    let index = match SkillIndex::open(dir).await {
                        Ok(index) => index,
                        Err(error) => {
                            eprintln!("rigel: {error}");
                            return ExitCode::from(2);
                        }
                    };
                    match index.body(&id).await {
                        Ok(Some((body, _))) => (id.clone(), body),
                        Ok(None) => {
                            eprintln!("rigel: {id} has no body in the index");
                            return ExitCode::from(1);
                        }
                        Err(error) => {
                            eprintln!("rigel: {error}");
                            return ExitCode::from(2);
                        }
                    }
                }
                (None, None) => {
                    eprintln!("rigel: give a skill id or --file <PATH>");
                    return ExitCode::from(1);
                }
            };

            let found = requirements(&body);
            let host = Host::current();
            let statuses = service.detect(&host, false).await.unwrap_or_default();
            let rows: Vec<_> = found
                .iter()
                .map(|requirement| {
                    let present = statuses
                        .iter()
                        .find(|row| row.id == requirement.tool)
                        .is_some_and(|row| row.present);
                    json!({
                        "tool": requirement.tool,
                        "name": requirement.name,
                        "binary": requirement.binary,
                        "present": present,
                        "source": requirement.source,
                        "docs": requirement.docs,
                    })
                })
                .collect();

            if as_json {
                println!(
                    "{}",
                    json!({ "ok": true, "from": source_name, "host": host.key(), "requirements": rows })
                );
            } else if rows.is_empty() {
                println!("{source_name}: no tool requirements found");
            } else {
                for requirement in &found {
                    let present = statuses
                        .iter()
                        .find(|row| row.id == requirement.tool)
                        .is_some_and(|row| row.present);
                    println!(
                        "{:<10} {:<16} {}:{}  {}",
                        requirement.tool,
                        if present { "present" } else { "missing" },
                        requirement.source.kind.label(),
                        requirement.source.line,
                        requirement.source.text
                    );
                }
                let missing = found
                    .iter()
                    .filter(|requirement| {
                        !statuses
                            .iter()
                            .find(|row| row.id == requirement.tool)
                            .is_some_and(|row| row.present)
                    })
                    .count();
                if missing > 0 {
                    println!(
                        "\n  {missing} missing — `rigel tools install <tool>` shows the command"
                    );
                }
            }
            ExitCode::SUCCESS
        }

        ToolCommand::Verify {
            tool,
            host,
            json: as_json,
        } => {
            let host = host
                .map(|value| Host::parse(&value))
                .unwrap_or_else(Host::current);
            let Some(spec) = tool_by_alias(&tool) else {
                eprintln!("rigel: {tool} is not in the curated tool registry");
                return ExitCode::from(1);
            };
            match service.verify(spec.id, &host).await {
                Ok(pin) => {
                    let changed = matches!(pin, Pin::Changed { .. });
                    if as_json {
                        println!(
                            "{}",
                            json!({ "ok": !changed, "host": host.key(), "tool": spec.id, "pin": pin })
                        );
                    } else {
                        match &pin {
                            Pin::Pinned { sha256 } => {
                                println!("{:<10} pinned  {sha256}", spec.id)
                            }
                            Pin::Changed { from, to, path } => {
                                println!(
                                    "{:<10} changed  {} → {}  ({})",
                                    spec.id,
                                    short(from),
                                    short(to),
                                    path.display()
                                );
                                println!(
                                    "  the binary is not the one Rigel recorded; reinstall from a source you trust"
                                );
                            }
                            Pin::Unpinned => {
                                println!("{:<10} no recorded hash yet", spec.id)
                            }
                            Pin::Missing => println!("{:<10} not installed", spec.id),
                        }
                    }
                    if changed {
                        ExitCode::from(1)
                    } else {
                        ExitCode::SUCCESS
                    }
                }
                Err(error) => failure(error, as_json),
            }
        }

        ToolCommand::Restart {
            host,
            json: as_json,
        } => {
            let host = host
                .map(|value| Host::parse(&value))
                .unwrap_or_else(Host::current);
            match service.restart(&host).await {
                Ok(statuses) => {
                    let present = statuses.iter().filter(|row| row.present).count();
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": true,
                                "host": host.key(),
                                "present": present,
                                "tools": statuses,
                                "note": "A new shell reads its PATH from the OS. Run `rigel tools which <tool>` there before using the tool.",
                            })
                        );
                    } else {
                        println!("{} present on {} after re-probing", present, host.key());
                        println!(
                            "  new shells see it; this one may not — re-check with `rigel tools which <tool>`"
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => failure(error, as_json),
            }
        }

        ToolCommand::Ledger {
            limit,
            json: as_json,
        } => match service.ledger(limit).await {
            Ok(rows) => {
                if as_json {
                    println!("{}", json!({ "ok": true, "installs": rows }));
                } else if rows.is_empty() {
                    println!("no installs recorded");
                } else {
                    for row in &rows {
                        println!(
                            "{:<14} {:<16} {:<13} exit {}  {} ms",
                            row.name,
                            row.package,
                            row.decision,
                            row.exit_code
                                .map(|code| code.to_string())
                                .unwrap_or_else(|| "-".to_string()),
                            row.duration_ms.unwrap_or(0)
                        );
                        if let Some(error) = &row.error {
                            println!("  {error}");
                        }
                    }
                }
                ExitCode::SUCCESS
            }
            Err(error) => failure(error, as_json),
        },

        ToolCommand::Queued { json: as_json } => match service.queued().await {
            Ok(rows) => {
                if as_json {
                    println!("{}", json!({ "ok": true, "queued": rows }));
                } else if rows.is_empty() {
                    println!("nothing queued");
                } else {
                    for row in &rows {
                        println!("{:<12} {}", row.name, row.command);
                    }
                    println!("\n  approve one with `rigel tools install <tool> --yes`");
                }
                ExitCode::SUCCESS
            }
            Err(error) => failure(error, as_json),
        },

        ToolCommand::Install {
            tool,
            host,
            yes,
            unattended,
            allow_low_trust,
            json: as_json,
        } => {
            let host = host
                .map(|value| Host::parse(&value))
                .unwrap_or_else(Host::current);
            let plan = match service.plan(&tool, &host).await {
                Ok(plan) => plan,
                Err(error) => return failure(error, as_json),
            };

            if !as_json {
                println!("install {}", plan.name);
                println!("  command   {}", plan.command);
                println!("  manager   {}", plan.manager.label());
                println!("  host      {}", plan.host);
                println!("  verify    {}", plan.verification);
                println!(
                    "  trust     {:.0}/100 ({})",
                    plan.trust.score, plan.trust.band
                );
                if let Some(version) = &plan.already_present {
                    println!("  note      already present: {version}");
                }
            }

            // The plan is printed before the question in both modes, so `--json` still shows
            // the command that ran.
            // `--json` changes the output, never the decision: a machine-readable run still
            // needs `--yes` or an answer on stdin.
            let approved = if unattended {
                false
            } else if yes {
                true
            } else {
                ask(&plan.command).unwrap_or(false)
            };

            if !approved && !unattended {
                let record = match service.deny(&plan, "not approved").await {
                    Ok(record) => record,
                    Err(error) => return failure(error, as_json),
                };
                if as_json {
                    println!(
                        "{}",
                        json!({ "ok": false, "approved": false, "plan": plan, "install": record })
                    );
                }
                eprintln!("rigel: nothing ran");
                return ExitCode::from(1);
            }

            match service
                .install(
                    &plan,
                    Arc::new(SystemRunner),
                    &host,
                    unattended,
                    allow_low_trust,
                )
                .await
            {
                Ok(record) => {
                    let ok = record.exit_code == Some(0) || record.decision == "queued";
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": ok,
                                "approved": approved,
                                "plan": plan,
                                "install": record,
                                "recheck": record.recheck(),
                            })
                        );
                    } else {
                        println!("\n  {}", record.recheck());
                        if let Some(duration) = record.duration_ms {
                            println!("  {} ms · exit {:?}", duration, record.exit_code);
                        }
                    }
                    if ok {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::from(1)
                    }
                }
                Err(error) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": false,
                                "approved": approved,
                                "plan": plan,
                                "error": error.to_string(),
                            })
                        );
                    }
                    eprintln!("rigel: {error}");
                    if matches!(error, rigel_core::ToolError::Blocked { .. }) {
                        eprintln!(
                            "      run `rigel trust explain {}` to see the arithmetic, or pass \
                             --allow-low-trust if you know the package",
                            plan.package
                        );
                    }
                    ExitCode::from(1)
                }
            }
        }
    }
}

pub fn run_trust(command: TrustCommand) -> ExitCode {
    match command {
        TrustCommand::Explain {
            package,
            host,
            json: as_json,
        } => {
            let host = host
                .map(|value| Host::parse(&value))
                .unwrap_or_else(Host::current);
            let report: TrustReport = trust_for(&package, &host);
            if as_json {
                println!(
                    "{}",
                    json!({ "ok": true, "host": host.key(), "trust": report })
                );
                return ExitCode::SUCCESS;
            }

            println!(
                "{}  {:.0}/100  {}",
                report.package, report.score, report.band
            );
            for factor in &report.factors {
                println!(
                    "  {:<12} {:>4.0} × {:.2} = {:>5.1}   {}",
                    factor.key, factor.weight, factor.ratio, factor.points, factor.evidence
                );
            }
            println!("\n  total: {}", report.arithmetic.join("; "));
            if report.score < 50.0 {
                println!("  below 50, an install is refused unless --allow-low-trust is passed.");
            } else if report.score < 75.0 {
                println!("  between 50 and 75, an install asks and shows this page first.");
            } else {
                println!(
                    "  at or above 75, the score is trusted — and an install still asks first."
                );
            }
            ExitCode::SUCCESS
        }
    }
}

fn ask(command: &str) -> std::io::Result<bool> {
    print!("\n  run `{command}`? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer)? == 0 {
        return Ok(false);
    }
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

fn failure(error: rigel_core::ToolError, as_json: bool) -> ExitCode {
    if as_json {
        println!("{}", json!({ "ok": false, "error": error.to_string() }));
    }
    eprintln!("rigel: {error}");
    ExitCode::from(1)
}

//! `rigel` — the CLI half of the app.
//!
//! Everything here goes through the same core the window uses, which is what makes the
//! Phase 1 exit test meaningful: two separate processes, one database, one session. Phase 2
//! adds the skills surface, and with it the rule from §7.2 that the UI never parses human
//! output — every command the renderer shells out to has `--json`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use rigel_core::{
    SessionService, SkillIndex, SkillsError, Store, default_index_dir, skills::Source,
};
use serde_json::json;

mod claims;
mod doctor;
mod harness;
mod phase3;
mod tools;
mod update;
mod worktree;

use harness::{CodexCommand, HarnessCommand};
use phase3::{AgentCommand, KeyCommand, MemoryCommand, PatchCommand, SnapshotCommand};

#[derive(Parser)]
#[command(name = "rigel", version, about = "Rigel — an agent workspace")]
struct Cli {
    /// Where `index.db` and `blobs/` live. Defaults to the per-user app data directory.
    #[arg(long, global = true, env = "RIGEL_INDEX_DIR", value_name = "DIR")]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Report local state. Exit 0 is all clear, 1 has warnings, 2 has failures.
    Doctor {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
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

    /// The skill index: search it offline, extend it from the network.
    Skills {
        #[command(subcommand)]
        command: SkillCommand,
    },

    /// The native agent: one turn, billed to the session.
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },

    /// Proposed and applied file patches.
    Patch {
        #[command(subcommand)]
        command: PatchCommand,
    },

    /// Committed memory for one agent.
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },

    /// Provider keys in the OS keychain.
    Keys {
        #[command(subcommand)]
        command: KeyCommand,
    },

    /// Pre-destructive snapshots of uncommitted work.
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },

    /// Advisory file claims: who holds what, and release one by hand.
    Claim {
        #[command(subcommand)]
        command: claims::ClaimCommand,
    },

    /// Tools on this machine: what is present, what a skill needs, what to install.
    Tools {
        #[command(subcommand)]
        command: tools::ToolCommand,
    },

    /// The arithmetic behind an install decision.
    Trust {
        #[command(subcommand)]
        command: tools::TrustCommand,
    },

    /// Signed updates; GitHub is checked at launch.
    Update {
        #[command(subcommand)]
        command: update::UpdateCommand,
    },

    /// One private git checkout per agent, merged back through the patch pipeline.
    Worktree {
        #[command(subcommand)]
        command: worktree::WorktreeCommand,
    },

    /// The Codex harness: probe it, or run one turn through it.
    Codex {
        #[command(subcommand)]
        command: CodexCommand,
    },

    /// The Claude Code harness: probe it, or run one turn through it.
    Claude {
        #[command(subcommand)]
        command: harness::ClaudeCommand,
    },

    /// Every harness on this machine, cached in `adapters.json`.
    Harness {
        #[command(subcommand)]
        command: HarnessCommand,
    },

    /// Spend across every session in a project (D92).
    Spend {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        json: bool,
    },

    /// Run the stdio MCP server an adapter injects into an agent.
    Mcp {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        session: String,
        #[arg(long)]
        agent: String,
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

    /// Reconcile a session with the tree and the patch journal after a kill.
    Resume {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        id: String,
        /// Continue on a tree that no longer matches the checkpoint (recorded as an intervention).
        #[arg(long)]
        force: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum SkillCommand {
    /// Full-text search over the local index. Never touches the network.
    Search {
        query: Vec<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },

    /// Show one skill: metadata, static review, evidence, variants.
    Get {
        id: String,
        /// Write the body to `<DIR>/<name>/SKILL.md`.
        #[arg(long, value_name = "DIR")]
        to: Option<PathBuf>,
        /// Print the body hash a session will pin to (binding lands in Phase 3).
        #[arg(long)]
        pin: bool,
        #[arg(long)]
        json: bool,
    },

    /// Walk a GitHub repository (or URL) into the index.
    Index {
        target: String,
        /// Revalidate every cached URL instead of trusting the freshness window.
        #[arg(long)]
        force: bool,
        /// Cap the number of skills taken from one repository.
        #[arg(long, default_value_t = rigel_core::skills::github::DEFAULT_REPO_LIMIT)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },

    /// Current health of every source.
    Status {
        #[arg(long)]
        json: bool,
    },

    /// Pull rows from a marketplace into the index.
    Sync {
        /// `skillsmp` or `skillhub`.
        source: String,
        #[arg(long, default_value = "agent")]
        query: String,
        #[arg(long, default_value_t = 30)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },

    /// Sweep both marketplaces across many pages into the index (metadata only, no bodies).
    Sweep {
        /// SkillHub list pages to walk, 100 cards each. 0 skips the marketplace.
        #[arg(long, default_value_t = 0)]
        skillhub_pages: u32,
        /// SkillsMP search queries to walk, comma-separated, each up to `--skillsmp-pages` pages of 50.
        #[arg(
            long,
            default_value = "skill,claude,code,test,commit,review,deploy,automation,agent,data,docs,security,api,design,pdf,summarize,write,translate,debug,git"
        )]
        skillsmp_queries: String,
        /// SkillsMP pages per query, 50 rows each.
        #[arg(long, default_value_t = 0)]
        skillsmp_pages: u32,
        #[arg(long)]
        json: bool,
    },

    /// Show index size, blob count and source health.
    Stats {
        #[arg(long)]
        json: bool,
    },

    /// Import the prototype's export, or one skills.sh page with its dated audits.
    Import {
        /// The prototype's `skilldb:entries:v1` JSON export.
        #[arg(long, value_name = "FILE", conflicts_with = "skills_sh")]
        from: Option<PathBuf>,
        /// A skills.sh skill page, e.g. https://www.skills.sh/owner/repo/skill.
        #[arg(long = "skills-sh", value_name = "URL")]
        skills_sh: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// Delete blobs no row references.
    Gc {
        #[arg(long)]
        json: bool,
    },
}

fn data_dir(cli: &Cli) -> PathBuf {
    cli.data_dir.clone().unwrap_or_else(default_index_dir)
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let dir = data_dir(&cli);

    match cli.command {
        Command::Doctor {
            project,
            json: as_json,
        } => doctor::run(&project, &dir, as_json).await,

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

        Command::Skills { command } => run_skills(&dir, command).await,

        Command::Agent { command } => phase3::run_agent(&dir, command).await,

        Command::Patch { command } => phase3::run_patch(command).await,

        Command::Memory { command } => phase3::run_memory(command).await,

        Command::Keys { command } => phase3::run_keys(command),

        Command::Snapshot { command } => phase3::run_snapshot(command, dir),

        Command::Claim { command } => claims::run_claim(command).await,

        Command::Codex { command } => harness::run_codex(command).await,

        Command::Claude { command } => harness::run_claude(command, &dir).await,

        Command::Harness { command } => harness::run_harness(command, &dir).await,

        Command::Spend {
            project,
            json: as_json,
        } => match Store::open_project(&project).await {
            Ok(store) => match rigel_core::spend::report(store.pool()).await {
                Ok(report) => {
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&report).unwrap_or_default()
                        );
                    } else {
                        println!("total  {}", dollars(report.total_micros));
                        println!("turns  {}", report.turns);
                        println!("\nby session");
                        for row in &report.by_session {
                            println!(
                                "  {:<40} {:>10}  ({} turns)",
                                truncate(&row.label, 40),
                                dollars(row.micros),
                                row.turns
                            );
                        }
                        println!("\nby day");
                        for row in &report.by_day {
                            println!(
                                "  {:<12} {:>10}  ({} turns)",
                                row.label,
                                dollars(row.micros),
                                row.turns
                            );
                        }
                        println!("\nby agent");
                        for row in &report.by_agent {
                            println!(
                                "  {:<40} {:>10}  ({} turns)",
                                truncate(&row.label, 40),
                                dollars(row.micros),
                                row.turns
                            );
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("rigel: {error}");
                    ExitCode::from(2)
                }
            },
            Err(error) => {
                eprintln!("rigel: {error}");
                ExitCode::from(2)
            }
        },

        Command::Tools { command } => tools::run_tools(&dir, command).await,

        Command::Trust { command } => tools::run_trust(command),

        Command::Update { command } => update::run(&dir, command).await,

        Command::Worktree { command } => worktree::run(&dir, command).await,

        Command::Mcp {
            project,
            session,
            agent,
        } => phase3::run_mcp(&dir, project, session, agent).await,
    }
}

async fn run_skills(dir: &Path, command: SkillCommand) -> ExitCode {
    let index = match SkillIndex::open(dir).await {
        Ok(index) => index,
        Err(error) => return skills_failure(error, false),
    };

    match command {
        SkillCommand::Search {
            query,
            limit,
            json: as_json,
        } => {
            let query = query.join(" ");
            match index.search(&query, limit).await {
                Ok(hits) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "query": query,
                                "count": hits.len(),
                                "order": "grade -> stars -> recency",
                                "results": hits,
                            })
                        );
                    } else if hits.is_empty() {
                        println!(
                            "no skills match {query:?} — index a repo first: rigel skills index <repo>"
                        );
                    } else {
                        for hit in &hits {
                            println!(
                                "{}  {:<10}  {:<28}  {}",
                                hit.grade.clone().unwrap_or_else(|| "?".to_string()),
                                hit.source,
                                truncate(&hit.name, 28),
                                truncate(&hit.description, 80)
                            );
                            let mut notes = Vec::new();
                            if hit.variants > 1 {
                                notes.push(format!("{} variants", hit.variants));
                            }
                            if hit.unmeasured {
                                notes.push("unmeasured claim".to_string());
                            }
                            if !hit.has_body {
                                notes.push("no body".to_string());
                            }
                            if !notes.is_empty() {
                                println!("            {}", notes.join(" · "));
                            }
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => skills_failure(error, as_json),
            }
        }

        SkillCommand::Get {
            id,
            to,
            pin,
            json: as_json,
        } => match index.get(&id).await {
            Ok(Some(detail)) => {
                if let Some(dir) = &to {
                    let (body, sha) = match index.body(&id).await {
                        Ok(Some(body)) => body,
                        Ok(None) => {
                            return skills_failure(SkillsError::NoBody(id.clone()), as_json);
                        }
                        Err(error) => return skills_failure(error, as_json),
                    };
                    let folder = slugify(&detail.skill.name);
                    let destination = dir.join(&folder);
                    if let Err(error) = std::fs::create_dir_all(&destination) {
                        return skills_failure(SkillsError::Io(error), as_json);
                    }
                    let file = destination.join("SKILL.md");
                    if let Err(error) = std::fs::write(&file, body) {
                        return skills_failure(SkillsError::Io(error), as_json);
                    }
                    if as_json {
                        println!(
                            "{}",
                            json!({ "detail": detail, "written": file, "body_sha": sha, "pinned": pin })
                        );
                    } else {
                        println!("{}", file.display());
                        println!("  sha {sha}{}", if pin { " (pinned)" } else { "" });
                    }
                    return ExitCode::SUCCESS;
                }

                if as_json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&detail).unwrap_or_default()
                    );
                    return ExitCode::SUCCESS;
                }

                if pin {
                    match index.body(&id).await {
                        Ok(Some((_, sha))) => println!(
                            "pin       {sha} — a session will bind to this hash in Phase 3\n"
                        ),
                        _ => println!("pin       no body to pin yet\n"),
                    }
                }
                print_detail(&detail);
                ExitCode::SUCCESS
            }
            Ok(None) => skills_failure(SkillsError::NotIndexed(id), as_json),
            Err(error) => skills_failure(error, as_json),
        },

        SkillCommand::Index {
            target,
            force,
            limit,
            json: as_json,
        } => {
            let policy = if force {
                rigel_core::SyncPolicy::forced()
            } else {
                rigel_core::SyncPolicy::normal()
            };
            let source = match index.github().await {
                Ok(source) => source.with_limit(limit),
                Err(error) => return skills_failure(error, as_json),
            };

            match source.fetch_repo(&target, policy).await {
                Ok(snapshot) => match index.ingest(&snapshot).await {
                    Ok(report) => {
                        if as_json {
                            println!("{}", serde_json::to_string(&report).unwrap_or_default());
                        } else {
                            println!(
                                "{} skills from {}@{}  (+{} added, {} updated, {} unchanged)",
                                report.with_body,
                                report.repo.clone().unwrap_or_default(),
                                report.branch.clone().unwrap_or_default(),
                                report.added,
                                report.updated,
                                report.unchanged
                            );
                            println!(
                                "  {} of {} found · {} requests · {} clusters",
                                report.with_body,
                                report.found,
                                report.http_requests,
                                report.clusters
                            );
                            if report.truncated {
                                println!(
                                    "  note: GitHub truncated the tree; some skills were not seen"
                                );
                            }
                        }
                        ExitCode::SUCCESS
                    }
                    Err(error) => skills_failure(error, as_json),
                },
                Err(error) => skills_failure(error, as_json),
            }
        }

        SkillCommand::Status { json: as_json } => match index.status().await {
            Ok(statuses) => {
                if as_json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&statuses).unwrap_or_default()
                    );
                } else if statuses.is_empty() {
                    println!("no sources yet — rigel skills index <repo>");
                } else {
                    for status in &statuses {
                        println!(
                            "{:<10} {:<8} last sync {}",
                            status.id,
                            format!("{:?}", status.state).to_lowercase(),
                            status
                                .last_sync_at
                                .map(rigel_core::skills::fmt_time)
                                .unwrap_or_else(|| "never".to_string())
                        );
                        if let Some(error) = &status.error {
                            println!("           {error}");
                        }
                    }
                }
                ExitCode::SUCCESS
            }
            Err(error) => skills_failure(error, as_json),
        },

        SkillCommand::Sync {
            source,
            query,
            limit,
            json: as_json,
        } => match index.sync_marketplace(&source, &query, limit).await {
            Ok(count) => {
                if as_json {
                    println!("{}", json!({ "source": source, "rows": count }));
                } else {
                    println!("{count} rows from {source}");
                }
                ExitCode::SUCCESS
            }
            Err(error) => skills_failure(error, as_json),
        },

        SkillCommand::Sweep {
            skillhub_pages,
            skillsmp_queries,
            skillsmp_pages,
            json: as_json,
        } => {
            let queries: Vec<String> = skillsmp_queries.split(',').map(str::to_owned).collect();
            match index.sweep(skillhub_pages, &queries, skillsmp_pages).await {
                Ok(report) => {
                    if as_json {
                        println!("{}", serde_json::to_string(&report).unwrap_or_default());
                    } else {
                        println!(
                            "skillhub    {} rows over {} page(s){}",
                            report.skillhub_rows,
                            report.skillhub_pages,
                            report
                                .skillhub_error
                                .as_deref()
                                .map(|error| format!(" — stopped: {error}"))
                                .unwrap_or_default()
                        );
                        println!(
                            "skillsmp    {} rows over {} query/ies × {} page(s){}",
                            report.skillsmp_rows,
                            report.skillsmp_queries,
                            report.skillsmp_pages,
                            report
                                .skillsmp_error
                                .as_deref()
                                .map(|error| format!(" — stopped: {error}"))
                                .unwrap_or_default()
                        );
                        println!("duplicates dropped {}", report.duplicates_dropped);
                        println!(
                            "index       {} → {} skills",
                            report.skills_before, report.skills_after
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => skills_failure(error, as_json),
            }
        }

        SkillCommand::Stats { json: as_json } => match index.stats().await {
            Ok(stats) => {
                if as_json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&stats).unwrap_or_default()
                    );
                } else {
                    println!("skills      {}", stats.skills);
                    println!("with body   {}", stats.with_body);
                    println!("clusters    {}", stats.clusters);
                    println!(
                        "blobs       {} ({} KB)",
                        stats.blobs,
                        stats.blob_bytes / 1024
                    );
                }
                ExitCode::SUCCESS
            }
            Err(error) => skills_failure(error, as_json),
        },

        SkillCommand::Import {
            from,
            skills_sh,
            json: as_json,
        } => match (from, skills_sh) {
            (Some(from), None) => {
                let text = match std::fs::read_to_string(&from) {
                    Ok(text) => text,
                    Err(error) => return skills_failure(SkillsError::Io(error), as_json),
                };
                match index.import_prototype_entries(&text).await {
                    Ok(report) => {
                        if as_json {
                            println!("{}", serde_json::to_string(&report).unwrap_or_default());
                        } else {
                            println!(
                                "imported {} · skipped {} (already present) · invalid {}",
                                report.imported, report.skipped, report.invalid
                            );
                        }
                        ExitCode::SUCCESS
                    }
                    Err(error) => skills_failure(error, as_json),
                }
            }
            (None, Some(url)) => {
                let policy = rigel_core::SyncPolicy::normal();
                match rigel_core::skills::skillssh::import(&index, &url, policy).await {
                    Ok(report) => {
                        if as_json {
                            println!("{}", serde_json::to_string(&report).unwrap_or_default());
                        } else {
                            println!(
                                "{}  {}",
                                report.repo,
                                report
                                    .skill_id
                                    .as_deref()
                                    .unwrap_or("(the repository no longer carries this skill)")
                            );
                            for audit in &report.audits {
                                println!(
                                    "  {:<22} {:<6} {}",
                                    audit.vendor,
                                    audit.verdict,
                                    audit.audited_on.as_deref().unwrap_or("date unavailable")
                                );
                            }
                            println!(
                                "  indexed {} skills ({} added, {} updated)",
                                report.indexed.found, report.indexed.added, report.indexed.updated
                            );
                        }
                        ExitCode::SUCCESS
                    }
                    Err(error) => skills_failure(error, as_json),
                }
            }
            (None, None) => {
                eprintln!(
                    "rigel: pass --from <file> for the prototype export, or --skills-sh <url> for a skills.sh page"
                );
                ExitCode::from(1)
            }
            (Some(_), Some(_)) => {
                eprintln!("rigel: pass one of --from or --skills-sh, not both");
                ExitCode::from(1)
            }
        },

        SkillCommand::Gc { json: as_json } => {
            let live = match index.live_blobs().await {
                Ok(live) => live,
                Err(error) => return skills_failure(error, as_json),
            };
            match index.blobs().gc(&live) {
                Ok(removed) => {
                    if as_json {
                        println!("{}", json!({ "removed": removed, "live": live.len() }));
                    } else {
                        println!("removed {removed} orphan blobs, kept {}", live.len());
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => skills_failure(SkillsError::Io(error), as_json),
            }
        }
    }
}

fn print_detail(detail: &rigel_core::SkillDetail) {
    let skill = &detail.skill;
    println!("{}", skill.name);
    println!("  {}", skill.description);
    println!();
    println!("  id        {}", skill.id);
    println!("  source    {}", skill.source);
    if let Some(repo) = &skill.repo {
        println!("  repo      {repo}");
    }
    if let Some(url) = &skill.url {
        println!("  url       {url}");
    }
    println!(
        "  grade     {} ({})",
        skill
            .grade
            .clone()
            .unwrap_or_else(|| "not scanned".to_string()),
        skill.grade_scope.clone().unwrap_or_else(|| "—".to_string())
    );
    println!(
        "  body      {}",
        if skill.has_body {
            "indexed"
        } else {
            "not fetched"
        }
    );
    if let Some(seen) = skill.last_seen_at {
        println!("  last seen {}", rigel_core::skills::fmt_time(seen));
    }

    if !detail.capabilities.is_empty() {
        println!();
        println!("static review (does not prove a skill is harmless)");
        for capability in &detail.capabilities {
            println!("  [{}] {}", capability.severity.as_str(), capability.label);
            for line in &capability.evidence {
                println!("        {}:{}  {}", line.file, line.line, line.code);
            }
        }
    }

    if !detail.evidence.is_empty() {
        println!();
        println!("evidence");
        for row in &detail.evidence {
            if row.label == "audited" {
                // A directory's verdict is a date and a link, not a number: print it that way
                // so nobody reads the empty measured/advertised columns as a zero.
                println!(
                    "  {:<24} [audited] {}  {}",
                    row.metric,
                    rigel_core::skills::fmt_time(row.observed_at),
                    row.source
                );
                continue;
            }
            println!(
                "  {:<18} measured {:<6} advertised {:<6} [{}] {}",
                row.metric,
                row.measured
                    .map(|v| format!("{v}"))
                    .unwrap_or_else(|| "—".to_string()),
                row.advertised
                    .map(|v| format!("{v}"))
                    .unwrap_or_else(|| "—".to_string()),
                row.label,
                row.source
            );
        }
    }

    if !detail.variants.is_empty() {
        println!();
        println!("{} near-identical variant(s)", detail.variants.len());
        for variant in &detail.variants {
            println!("  {}  {}", variant.id, variant.delta);
        }
    }
}

/// Micro-dollars as `$0.1234`; the canonical unit is µ$ everywhere else.
fn dollars(micros: i64) -> String {
    format!(
        "${}.{:04}",
        micros / 1_000_000,
        (micros % 1_000_000).abs() / 100
    )
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let cut: String = value.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

fn slugify(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let collapsed = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if collapsed.is_empty() {
        "skill".to_string()
    } else {
        collapsed
    }
}

/// §7.2: `1` user error, `2` precondition failed. The message goes to stderr in both cases;
/// `--json` still gets a machine-readable object so the UI never parses prose.
fn skills_failure(error: SkillsError, as_json: bool) -> ExitCode {
    if as_json {
        println!("{}", json!({ "ok": false, "error": error.to_string() }));
    }
    eprintln!("rigel: {error}");
    ExitCode::from(error.exit_code())
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

        SessionCommand::Resume {
            project,
            id,
            force,
            json: as_json,
        } => match SessionService::open(&project).await {
            Ok(service) => match rigel_core::resume::resume(&service, &project, &id, force).await {
                Ok(outcome) => {
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&outcome).unwrap_or_default()
                        );
                    } else {
                        println!(
                            "{}  {}  tree {}",
                            &outcome.session.id[..8.min(outcome.session.id.len())],
                            if outcome.clean {
                                "clean"
                            } else {
                                "interrupted"
                            },
                            &outcome.tree_hash[..12.min(outcome.tree_hash.len())]
                        );
                        for turn in &outcome.closed_turns {
                            println!(
                                "  closed turn {} ({})",
                                turn.seq,
                                turn.stop_reason.clone().unwrap_or_default()
                            );
                        }
                        for patch in &outcome.patches {
                            println!(
                                "  patch {} {} {}",
                                patch.patch_id,
                                patch.path,
                                format!("{:?}", patch.state).to_lowercase()
                            );
                        }
                        if !outcome.reconcile.ok {
                            println!(
                                "  budget mismatch: recorded {}µ$ summed {}µ$",
                                outcome.reconcile.session_recorded_micros,
                                outcome.reconcile.session_summed_micros
                            );
                        }
                    }
                    if !outcome.clean && !force {
                        // §7.2: a resumed session that cannot continue cleanly is a failed
                        // precondition, not an error.
                        ExitCode::from(2)
                    } else {
                        ExitCode::SUCCESS
                    }
                }
                Err(error) => {
                    eprintln!("rigel: {error}");
                    ExitCode::from(2)
                }
            },
            Err(error) => fail(error),
        },
    }
}

fn fail(error: impl std::fmt::Display) -> ExitCode {
    eprintln!("rigel: {error}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::{AgentCommand, Cli, Command};

    #[test]
    fn agent_run_takes_the_effort_separately_from_the_model() {
        let cli = <Cli as clap::Parser>::try_parse_from([
            "rigel",
            "agent",
            "run",
            "--prompt",
            "say pong",
            "--model",
            "gpt-5.6-sol",
            "--effort",
            "high",
        ])
        .expect("--effort is part of agent run");
        match cli.command {
            Command::Agent {
                command:
                    AgentCommand::Run {
                        effort,
                        model,
                        prompt,
                        ..
                    },
            } => {
                assert_eq!(effort.as_deref(), Some("high"));
                assert_eq!(model.as_deref(), Some("gpt-5.6-sol"));
                assert_eq!(prompt, "say pong");
            }
            _ => panic!("expected `rigel agent run`"),
        }
    }

    #[test]
    fn agent_run_without_an_effort_leaves_the_provider_default() {
        let cli = <Cli as clap::Parser>::try_parse_from(["rigel", "agent", "run", "--prompt", "x"])
            .expect("effort is optional");
        match cli.command {
            Command::Agent {
                command: AgentCommand::Run { effort, .. },
            } => assert_eq!(effort, None),
            _ => panic!("expected `rigel agent run`"),
        }
    }
}

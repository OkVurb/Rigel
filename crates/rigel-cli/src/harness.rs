//! The `rigel codex` and `rigel claude` surfaces: probe a harness, or run one turn through it.
//!
//! They are the first external adapters wired to the CLI (P4.1, P4.5, P4.6). A turn is a real
//! session turn: it opens the newest session (or starts one), records the prompt, normalised
//! items, approvals, answer and a priced `Usage` row, and closes the turn. Codex's estimate is
//! printed before the request (P4.19), and a turn left running by a killed process is marked
//! `interrupted` on the next run for that agent only.
//!
//! Claude asks before it acts through `rigel_approve`, so the CLI writes the MCP config naming
//! this binary's own server and answers each question on stdin while the turn runs. A harness
//! turn therefore asks the same four scopes, in the same words, as the native agent.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use clap::Subcommand;
use rigel_adapters::registry::{self, CACHE_FILE};
use rigel_adapters::{
    Adapter, AdapterError, ApprovalDecision, ApprovalRequest, Approver, ClaudeAdapter,
    ClaudeConfig, ClaudeEvent, CodexAdapter, CodexConfig, CodexEvent, PermissionMode, Recorded,
    Transport, write_mcp_config,
};
use rigel_core::agent::approval::{ApprovalBus, PendingApproval};
use rigel_core::credentials::KeychainCredentialStore;
use rigel_core::{ApprovalScope, Scopes};
use serde_json::json;

use crate::phase3::parse_scope_answer;

#[derive(Subcommand)]
pub enum CodexCommand {
    /// Report the installed binary, its version and how it authenticates.
    Probe {
        /// The binary to probe. Defaults to `codex` on `PATH`.
        #[arg(long)]
        exe: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },

    /// Run one prompt through Codex and print the answer and the bill.
    Run {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        /// Session id. Defaults to the newest open session, starting one if there is none.
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        prompt: String,
        /// Provider base URL. Defaults to the local router.
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        model: Option<String>,
        /// `auto`, `app-server` or `exec`.
        #[arg(long, default_value = "auto")]
        transport: String,
        #[arg(long)]
        exe: Option<PathBuf>,
        /// Tokens allowed for Codex's available-skills catalogue (P4.19). Defaults to 1000;
        /// the CLI's own default is 2% of the model window, up to 10 000.
        #[arg(long, value_name = "TOKENS")]
        skills_budget: Option<u32>,
        /// Skip host skill discovery entirely for this turn; shorthand for `--skills-budget 0`.
        #[arg(long)]
        no_skills: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum HarnessCommand {
    /// Probe all four harnesses, refresh `adapters.json`, and print what was found.
    Probe {
        #[arg(long)]
        json: bool,
        /// Ignore a still-accurate cache and probe again.
        #[arg(long)]
        refresh: bool,
    },
}

pub async fn run_harness(command: HarnessCommand, dir: &std::path::Path) -> ExitCode {
    match command {
        HarnessCommand::Probe { json, refresh } => probe_harnesses(dir, json, refresh).await,
    }
}

async fn probe_harnesses(dir: &std::path::Path, as_json: bool, refresh: bool) -> ExitCode {
    let path = dir.join(CACHE_FILE);
    let cache = match rigel_adapters::probe_cached(&path, &KeychainCredentialStore, refresh).await {
        Ok(cache) => cache,
        Err(error) => {
            eprintln!("rigel: could not write {}: {error}", path.display());
            return ExitCode::from(2);
        }
    };

    if as_json {
        println!(
            "{}",
            json!({
                "ok": true,
                "path": path,
                "probed_at": cache.probed_at,
                "harnesses": cache.infos(),
            })
        );
        return ExitCode::SUCCESS;
    }

    for info in cache.infos() {
        let name = registry::display_name(info.harness);
        match (info.installed, info.version.as_deref()) {
            (true, Some(version)) => println!(
                "{name:<12} {version:<10} {:<13} {}",
                auth_label(info.auth_mode),
                info.path
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            ),
            (true, None) => println!(
                "{name:<12} {:<10} {:<13} {}",
                "unknown",
                auth_label(info.auth_mode),
                info.path
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            ),
            (false, _) => println!(
                "{name:<12} {:<10} {:<13} install: {}",
                "missing",
                auth_label(info.auth_mode),
                info.install_hint.unwrap_or_default()
            ),
        }
    }
    println!("\n  cached in {}", path.display());
    ExitCode::SUCCESS
}

fn auth_label(mode: rigel_adapters::AuthMode) -> &'static str {
    match mode {
        rigel_adapters::AuthMode::ApiKey => "api key",
        rigel_adapters::AuthMode::Subscription => "subscription",
        rigel_adapters::AuthMode::None => "no key",
    }
}

/// The permission tool `rigel mcp` advertises to Claude Code (P4.10).
const APPROVAL_TOOL: &str = "mcp__rigel__rigel_approve";

#[derive(Subcommand)]
pub enum ClaudeCommand {
    /// Report the installed binary, its version and how it authenticates.
    Probe {
        /// The binary to probe. Defaults to `claude` on `PATH`.
        #[arg(long)]
        exe: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },

    /// Run one prompt through Claude Code and print the answer and the bill.
    Run {
        #[arg(long, value_name = "PATH", default_value = ".")]
        project: PathBuf,
        /// Session id. Defaults to the newest open session, starting one if there is none.
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        model: Option<String>,
        /// `default`, `plan`, `acceptEdits` or `dontAsk`. Approvals are asked either way.
        #[arg(long, default_value = "default")]
        permission_mode: String,
        #[arg(long)]
        exe: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

pub async fn run_claude(command: ClaudeCommand, dir: &Path) -> ExitCode {
    match command {
        ClaudeCommand::Probe { exe, json } => probe_claude(exe, json).await,
        ClaudeCommand::Run {
            project,
            session,
            prompt,
            model,
            permission_mode,
            exe,
            json: as_json,
        } => {
            run_claude_turn(ClaudeRunOptions {
                project,
                session,
                prompt,
                model,
                permission_mode,
                exe,
                as_json,
                data_dir: dir.to_path_buf(),
            })
            .await
        }
    }
}

pub async fn run_codex(command: CodexCommand) -> ExitCode {
    match command {
        CodexCommand::Probe { exe, json: as_json } => probe(exe, as_json).await,
        CodexCommand::Run {
            project,
            session,
            prompt,
            base,
            model,
            transport,
            exe,
            skills_budget,
            no_skills,
            json: as_json,
        } => {
            let skills_budget = if no_skills {
                0
            } else {
                skills_budget.unwrap_or(rigel_adapters::codex::DEFAULT_SKILLS_BUDGET)
            };
            run(RunOptions {
                project,
                session,
                prompt,
                base,
                model,
                transport,
                exe,
                skills_budget,
                as_json,
            })
            .await
        }
    }
}

async fn probe(exe: Option<PathBuf>, as_json: bool) -> ExitCode {
    let mut config = CodexConfig::router(".");
    if let Some(exe) = exe {
        config.exe = exe;
    }
    config.inject_credential(&KeychainCredentialStore);
    let adapter = CodexAdapter::new(config);

    match adapter.probe().await {
        Ok(info) => {
            if as_json {
                println!("{}", json!({ "ok": true, "harness": info }));
            } else {
                println!(
                    "codex {}  {}",
                    info.version
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    info.path
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default()
                );
                println!("  auth        {:?}", info.auth_mode);
                println!("  json stream {}", info.capabilities.json_stream);
                println!("  approvals   {}", info.capabilities.programmatic_approval);
                println!("  worktree    {}", info.capabilities.worktree);
            }
            ExitCode::SUCCESS
        }
        Err(AdapterError::NotInstalled { .. }) => {
            if as_json {
                println!(
                    "{}",
                    json!({ "ok": true, "harness": { "installed": false } })
                );
            } else {
                println!("codex not installed");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if as_json {
                println!("{}", json!({ "ok": false, "error": error.to_string() }));
            }
            eprintln!("rigel: {error}");
            ExitCode::from(2)
        }
    }
}

struct RunOptions {
    project: PathBuf,
    session: Option<String>,
    prompt: String,
    base: Option<String>,
    model: Option<String>,
    transport: String,
    exe: Option<PathBuf>,
    skills_budget: u32,
    as_json: bool,
}

struct ClaudeRunOptions {
    project: PathBuf,
    session: Option<String>,
    prompt: String,
    model: Option<String>,
    permission_mode: String,
    exe: Option<PathBuf>,
    as_json: bool,
    /// The same directory the MCP child is told to use, so the approval channel agrees.
    data_dir: PathBuf,
}

async fn run(options: RunOptions) -> ExitCode {
    let RunOptions {
        project,
        session,
        prompt,
        base,
        model,
        transport,
        exe,
        skills_budget,
        as_json,
    } = options;

    let transport = match transport.as_str() {
        "auto" => Transport::Auto,
        "app-server" => Transport::AppServer,
        "exec" => Transport::Exec,
        other => {
            eprintln!("rigel: unknown transport {other:?}; expected auto, app-server or exec");
            return ExitCode::from(1);
        }
    };

    let mut config = CodexConfig::router(project);
    config.transport = transport;
    if let Some(base) = base {
        config.base_url = base;
    }
    if let Some(model) = model {
        config.model = model;
    }
    if let Some(exe) = exe {
        config.exe = exe;
    }
    config.skills_budget = skills_budget;

    let credential_provider = config.credential_provider().to_string();
    let project = config.cwd.clone();
    let model_name = config.model.clone();
    config.inject_credential(&KeychainCredentialStore);

    let recorded =
        match Recorded::open(&project, session.as_deref(), "codex", &model_name, &prompt).await {
            Ok(recorded) => recorded,
            Err(error) => {
                eprintln!("rigel: could not open the session: {error}");
                return ExitCode::from(2);
            }
        };

    // Codex asks before it writes; the same four scopes as the native agent answer here (N2).
    let adapter = CodexAdapter::new(config).with_approver(std::sync::Arc::new(HarnessApprover {
        scopes: Scopes::new(&project, rigel_core::default_index_dir()),
    }));

    // P4.19: the estimate goes out before the request. JSON mode keeps stdout to one document,
    // so the line goes to stderr where a script can still read it.
    let estimate = adapter.estimate_input(&prompt, recorded.last_input);
    let estimate_line = match estimate.last_turn_input {
        Some(previous) => format!(
            "context  ~{} in (last turn {previous} + prompt {}) · skills ceiling {}",
            estimate.input_tokens, estimate.prompt_tokens, estimate.skills_tokens
        ),
        None => format!(
            "context  ~{} in (prompt {} + fixed {} + skills {})",
            estimate.input_tokens,
            estimate.prompt_tokens,
            estimate.fixed_tokens,
            estimate.skills_tokens
        ),
    };
    if as_json {
        eprintln!("{estimate_line}");
    } else {
        println!("  {estimate_line}");
    }

    let mut notices = Vec::new();
    let mut items = Vec::new();
    let mut approvals = Vec::new();
    let mut streamed = false;

    let outcome = adapter
        .run_turn(&prompt, |event| match event {
            CodexEvent::Delta { text } => {
                if !as_json {
                    streamed = true;
                    print!("{text}");
                    let _ = std::io::stdout().flush();
                }
            }
            CodexEvent::Notice { message } => {
                if !notices.contains(&message) {
                    notices.push(message);
                }
            }
            // P4.8: every normalised item is reported, so `rigel codex run --json` shows the
            // commands and file changes a turn actually made.
            CodexEvent::Item(item) => items.push(item),
            CodexEvent::Approval { request, decision } => {
                if !as_json {
                    println!(
                        "  approval  {} ({}) -> {}",
                        request.title,
                        request.kind.as_deref().unwrap_or("tool"),
                        decision_name(decision)
                    );
                }
                approvals.push(json!({
                    "id": request.tool_call_id,
                    "title": request.title,
                    "kind": request.kind,
                    "decision": decision_name(decision),
                }));
            }
            CodexEvent::Started { .. } | CodexEvent::Message { .. } => {}
        })
        .await;

    if streamed && !as_json {
        println!();
    }

    recorded.record_items(&items, &approvals).await;
    let recorded_usage = recorded.finish_codex(&outcome, &model_name).await;

    match outcome {
        Ok(result) => {
            if as_json {
                println!(
                    "{}",
                    json!({
                        "ok": true,
                        "text": result.text,
                        "transport": result.transport.as_str(),
                        "fell_back": result.fell_back,
                        "thread_id": result.thread_id,
                        "usage": result.usage,
                        "recorded_usage": recorded_usage,
                        "estimate": estimate,
                        "estimate_before_send": estimate_line,
                        "session": recorded.session_id,
                        "agent": recorded.agent.id,
                        "turn": recorded.turn.id,
                        "duration_ms": result.duration.as_millis() as u64,
                        "notices": notices,
                        "approvals": approvals,
                        "items": items.iter().map(|item| json!({
                            "kind": item.kind.as_str(),
                            "body": item.body,
                        })).collect::<Vec<_>>(),
                    })
                );
            } else {
                if !streamed {
                    println!("{}", result.text);
                }
                println!();
                println!(
                    "  transport {}  {} ms{}",
                    result.transport.as_str(),
                    result.duration.as_millis(),
                    if result.fell_back {
                        "  (fell back)"
                    } else {
                        ""
                    }
                );
                if let Some(usage) = result.usage {
                    println!(
                        "  usage     {} in ({} cached) · {} out · turn {}",
                        usage.input_tokens,
                        usage.cached_input_tokens,
                        usage.output_tokens,
                        &recorded.turn.id[..8.min(recorded.turn.id.len())]
                    );
                }
                for item in &items {
                    println!("  item      {} {}", item.kind.as_str(), item.body);
                }
                for notice in &notices {
                    println!("  notice    {notice}");
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if as_json {
                println!(
                    "{}",
                    json!({
                        "ok": false,
                        "error": error.to_string(),
                        "provider": credential_provider,
                        "fix": format!("rigel keys set {credential_provider}"),
                        "session": recorded.session_id,
                        "turn": recorded.turn.id,
                    })
                );
            }
            eprintln!("rigel: {error}");
            if matches!(error, AdapterError::RequiresApiKey { .. }) {
                eprintln!(
                    "fix: store the {credential_provider} key with `rigel keys set {credential_provider}`"
                );
            }
            ExitCode::from(2)
        }
    }
}

async fn probe_claude(exe: Option<PathBuf>, as_json: bool) -> ExitCode {
    let mut config = ClaudeConfig::new(".");
    if let Some(exe) = exe {
        config.exe = exe;
    }
    config.inject_credential(&KeychainCredentialStore);
    let adapter = ClaudeAdapter::new(config);

    match adapter.probe().await {
        Ok(info) => {
            if as_json {
                println!("{}", json!({ "ok": true, "harness": info }));
            } else {
                println!(
                    "claude {}  {}",
                    info.version
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    info.path
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default()
                );
                println!("  auth        {:?}", info.auth_mode);
                println!("  json stream {}", info.capabilities.json_stream);
                println!("  approvals   {}", info.capabilities.programmatic_approval);
            }
            ExitCode::SUCCESS
        }
        Err(AdapterError::NotInstalled { .. }) => {
            if as_json {
                println!(
                    "{}",
                    json!({ "ok": true, "harness": { "installed": false } })
                );
            } else {
                println!("claude not installed");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if as_json {
                println!("{}", json!({ "ok": false, "error": error.to_string() }));
            }
            eprintln!("rigel: {error}");
            ExitCode::from(2)
        }
    }
}

/// Answer the harness's approval questions while the turn runs.
///
/// `rigel_approve` publishes each question to the channel and waits, so this loop is the
/// terminal surface that answers it: the same four scopes and the same words as the native
/// agent. EOF or an unreadable answer is a refusal, never a grant (N2).
async fn pump_approvals(bus: ApprovalBus, stop: Arc<AtomicBool>, as_json: bool) {
    let mut asked: HashSet<String> = HashSet::new();
    while !stop.load(Ordering::Relaxed) {
        for pending in bus.pending() {
            if !asked.insert(pending.id.clone()) {
                continue;
            }
            let scope = ask_pending(&pending, as_json).await;
            let _ = bus.answer(&pending.id, scope);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

async fn ask_pending(pending: &PendingApproval, as_json: bool) -> Option<ApprovalScope> {
    let question = format!(
        "
  approval  {}
  once / session / project / always, or reject [once]: ",
        pending.title.trim()
    );
    if as_json {
        eprint!("{question}");
        let _ = std::io::stderr().flush();
    } else {
        print!("{question}");
        let _ = std::io::stdout().flush();
    }
    let line = tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line),
        }
    })
    .await
    .ok()
    .flatten();
    line.as_deref().and_then(parse_scope_answer)
}

async fn run_claude_turn(options: ClaudeRunOptions) -> ExitCode {
    let ClaudeRunOptions {
        project,
        session,
        prompt,
        model,
        permission_mode,
        exe,
        as_json,
        data_dir,
    } = options;

    let mode = match permission_mode.as_str() {
        "default" => PermissionMode::Default,
        "plan" => PermissionMode::Plan,
        "acceptEdits" => PermissionMode::AcceptEdits,
        "dontAsk" => PermissionMode::DontAsk,
        other => {
            eprintln!(
                "rigel: unknown permission mode {other:?}; expected default, plan, acceptEdits or dontAsk"
            );
            return ExitCode::from(1);
        }
    };

    let mut config = ClaudeConfig::new(project);
    config.permission_mode = mode;
    if let Some(model) = model {
        config.model = Some(model);
    }
    if let Some(exe) = exe {
        config.exe = exe;
    }

    let credential_provider = config.credential_provider().to_string();
    let project = config.cwd.clone();
    let model_name = config
        .model
        .clone()
        .unwrap_or_else(|| "claude-sonnet".to_string());
    config.inject_credential(&KeychainCredentialStore);

    let recorded =
        match Recorded::open(&project, session.as_deref(), "claude", &model_name, &prompt).await {
            Ok(recorded) => recorded,
            Err(error) => {
                eprintln!("rigel: could not open the session: {error}");
                return ExitCode::from(2);
            }
        };

    // `rigel_approve` is answered by this process, so the child is pointed at this binary's own
    // server and at the same data directory the pump watches (P4.10).
    let mcp_config = match write_mcp_config(
        &data_dir,
        &project,
        &recorded.session_id,
        &recorded.agent.id,
    ) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("rigel: could not write the MCP config: {error}");
            return ExitCode::from(2);
        }
    };
    let config = config.with_permission_tool(mcp_config, APPROVAL_TOOL);

    let bus = ApprovalBus::new(&data_dir);
    let stop = Arc::new(AtomicBool::new(false));
    let pump = tokio::spawn(pump_approvals(bus, stop.clone(), as_json));

    let adapter = ClaudeAdapter::new(config);
    let mut notices = Vec::new();
    let mut items = Vec::new();
    let mut streamed = false;

    let outcome = adapter
        .run_turn(&prompt, |event| match event {
            ClaudeEvent::Delta { text } => {
                if !as_json {
                    streamed = true;
                    print!("{text}");
                    let _ = std::io::stdout().flush();
                }
            }
            ClaudeEvent::Notice { message } => {
                if !notices.contains(&message) {
                    notices.push(message);
                }
            }
            ClaudeEvent::Item(item) => items.push(item),
            ClaudeEvent::Started { .. } | ClaudeEvent::Message { .. } => {}
        })
        .await;

    stop.store(true, Ordering::Relaxed);
    let _ = pump.await;

    if streamed && !as_json {
        println!();
    }

    recorded.record_items(&items, &[]).await;
    let recorded_usage = recorded.finish_claude(&outcome, &model_name).await;

    match outcome {
        Ok(result) => {
            if as_json {
                println!(
                    "{}",
                    json!({
                        "ok": true,
                        "text": result.text,
                        "claude_session": result.session_id,
                        "usage": result.usage,
                        "recorded_usage": recorded_usage,
                        "session": recorded.session_id,
                        "agent": recorded.agent.id,
                        "turn": recorded.turn.id,
                        "duration_ms": result.duration.as_millis() as u64,
                        "notices": notices,
                        "items": items.iter().map(|item| json!({
                            "kind": item.kind.as_str(),
                            "body": item.body,
                        })).collect::<Vec<_>>(),
                    })
                );
            } else {
                if !streamed {
                    println!("{}", result.text);
                }
                println!();
                if let Some(usage) = result.usage {
                    println!(
                        "  usage     {} in ({} cached) · {} out · turn {}",
                        usage.input_tokens,
                        usage.cached_input_tokens,
                        usage.output_tokens,
                        &recorded.turn.id[..8.min(recorded.turn.id.len())]
                    );
                }
                for item in &items {
                    println!("  item      {} {}", item.kind.as_str(), item.body);
                }
                for notice in &notices {
                    println!("  notice    {notice}");
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if as_json {
                println!(
                    "{}",
                    json!({
                        "ok": false,
                        "error": error.to_string(),
                        "provider": credential_provider,
                        "fix": format!("rigel keys set {credential_provider}"),
                        "session": recorded.session_id,
                        "turn": recorded.turn.id,
                    })
                );
            }
            eprintln!("rigel: {error}");
            if matches!(error, AdapterError::RequiresApiKey { .. }) {
                eprintln!(
                    "fix: store the {credential_provider} key with `rigel keys set {credential_provider}`"
                );
            }
            ExitCode::from(2)
        }
    }
}

fn decision_name(decision: ApprovalDecision) -> &'static str {
    match decision {
        ApprovalDecision::AllowOnce => "allow_once",
        ApprovalDecision::AllowAlways => "allow_session",
        ApprovalDecision::RejectOnce => "reject_once",
        ApprovalDecision::RejectAlways => "reject_always",
        ApprovalDecision::Cancel => "cancelled",
    }
}

/// Answers a harness's own approval request with one of Rigel's four scopes.
///
/// Codex can only express "once" and "for this session", so a project or always grant is kept
/// in Rigel's store and replayed as `acceptForSession`: the next request for the same text is
/// answered without a prompt, and the harness is never told a scope it does not have.
struct HarnessApprover {
    scopes: Scopes,
}

impl HarnessApprover {
    /// The key is the kind plus the exact text the user read, so approving a command never
    /// approves a file change that happens to share its title.
    fn key(request: &ApprovalRequest) -> String {
        format!(
            "{}:{}",
            request.kind.as_deref().unwrap_or("tool"),
            request.title
        )
    }
}

/// Rigel's scope as the harness's own vocabulary. Codex's session grant is the strongest answer
/// its protocol carries, so project and always are sent as that; Rigel's store is what makes
/// them outlive the Codex process.
fn decision_for(scope: ApprovalScope) -> ApprovalDecision {
    match scope {
        ApprovalScope::Once => ApprovalDecision::AllowOnce,
        ApprovalScope::Session | ApprovalScope::Project | ApprovalScope::Always => {
            ApprovalDecision::AllowAlways
        }
    }
}

#[async_trait]
impl Approver for HarnessApprover {
    async fn approve(&self, request: &ApprovalRequest) -> ApprovalDecision {
        let key = Self::key(request);
        if let Some(scope) = self.scopes.granted(&key) {
            return decision_for(scope);
        }
        let asked = request.clone();
        let Some(scope) = tokio::task::spawn_blocking(move || ask_harness(&asked))
            .await
            .ok()
            .flatten()
        else {
            // End of input, an unreadable line or anything else: reject. Never allow by accident.
            return ApprovalDecision::RejectOnce;
        };
        if scope != ApprovalScope::Once {
            // A store that cannot write is not permission to widen: the answer still holds for
            // this one request.
            let _ = self.scopes.remember(&key, scope);
        }
        decision_for(scope)
    }
}

/// Print the request exactly as the harness sent it and read one answer from stdin. Anything
/// other than an explicit scope letter is a rejection, which is the same rule the native prompt
/// follows.
fn ask_harness(request: &ApprovalRequest) -> Option<ApprovalScope> {
    eprintln!(
        "\nthe harness wants to {}:\n  {}{}\n  [o]nce  [s]ession  [p]roject  [a]lways  [r]eject  (anything else rejects):",
        request.kind.as_deref().unwrap_or("do something"),
        request.title,
        match &request.raw_input {
            Some(input) if !input.is_null() => format!(
                "\n  input: {}",
                input.to_string().chars().take(600).collect::<String>()
            ),
            _ => String::new(),
        }
    );
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => parse_scope_answer(&line),
    }
}

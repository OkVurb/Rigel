//! The `rigel codex` surface: probe the harness, or run one turn through it.
//!
//! This is the first external adapter wired to the CLI (P4.1, P4.5). It deliberately does not
//! touch the session store yet: the point today is that the protocol works through Rigel's own
//! binary, with the same `-c` overrides the probe recorded (see handoff §4.2).

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use async_trait::async_trait;
use clap::Subcommand;
use rigel_adapters::registry::{self, CACHE_FILE};
use rigel_adapters::{
    Adapter, AdapterError, ApprovalDecision, ApprovalRequest, Approver, CodexAdapter, CodexConfig,
    CodexEvent, Transport,
};
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

pub async fn run_codex(command: CodexCommand) -> ExitCode {
    match command {
        CodexCommand::Probe { exe, json: as_json } => probe(exe, as_json).await,
        CodexCommand::Run {
            project,
            prompt,
            base,
            model,
            transport,
            exe,
            json: as_json,
        } => {
            run(RunOptions {
                project,
                prompt,
                base,
                model,
                transport,
                exe,
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
    prompt: String,
    base: Option<String>,
    model: Option<String>,
    transport: String,
    exe: Option<PathBuf>,
    as_json: bool,
}

async fn run(options: RunOptions) -> ExitCode {
    let RunOptions {
        project,
        prompt,
        base,
        model,
        transport,
        exe,
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

    let credential_provider = config.credential_provider().to_string();
    let project = config.cwd.clone();
    config.inject_credential(&KeychainCredentialStore);

    // Codex asks before it writes; the same four scopes as the native agent answer here (N2).
    let adapter = CodexAdapter::new(config).with_approver(std::sync::Arc::new(HarnessApprover {
        scopes: Scopes::new(&project, rigel_core::default_index_dir()),
    }));
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
                        "  usage     {} in ({} cached) · {} out",
                        usage.input_tokens, usage.cached_input_tokens, usage.output_tokens
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
        "\n{} wants to {}:\n  {}{}\n  [o]nce  [s]ession  [p]roject  [a]lways  [r]eject  (anything else rejects):",
        "the harness",
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

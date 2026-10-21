//! The `rigel codex` surface: probe the harness, or run one turn through it.
//!
//! This is the first external adapter wired to the CLI (P4.1, P4.5). It deliberately does not
//! touch the session store yet: the point today is that the protocol works through Rigel's own
//! binary, with the same `-c` overrides the probe recorded (see handoff §4.2).

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use rigel_adapters::{Adapter, AdapterError, CodexAdapter, CodexConfig, CodexEvent, Transport};
use rigel_core::credentials::KeychainCredentialStore;
use serde_json::json;

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
    config.inject_credential(&KeychainCredentialStore);

    let adapter = CodexAdapter::new(config);
    let mut notices = Vec::new();
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

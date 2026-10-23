//! The window's Claude path (P4.6): the same adapter and recorder as `rigel claude run`, with
//! the approval question going to the window's card instead of a terminal.
//!
//! Two halves:
//! * [`start`] creates nothing. `turn_send` has already written the turn row and returned it to
//!   the renderer; this adopts that row, runs one turn and closes it, so the window never shows
//!   a turn it cannot account for.
//! * [`mcp_mode`] is the stdio server Claude calls back into. The window binary hosts it so a
//!   packaged app does not need the CLI on `PATH`; the config names `current_exe()`.

use std::path::PathBuf;
use std::time::Duration;

use rigel_adapters::{ClaudeAdapter, ClaudeConfig, ClaudeEvent, Recorded, write_mcp_config};
use rigel_core::{Agent, AgentEvent, ItemKind, SessionService, Turn, credentials};
use tauri::Emitter;
use tokio::io::BufReader;

/// The permission tool `rigel mcp` advertises to Claude Code (P4.10).
const APPROVAL_TOOL: &str = "mcp__rigel__rigel_approve";

/// What the harness gets before an unanswered question is refused. The card has ten minutes; a
/// question nobody sees must not hold the child open forever.
const APPROVAL_WAIT: Duration = Duration::from_secs(600);

/// Run one Claude turn on its own thread. Returns once the thread is spawned; the answer streams
/// on `rigel://events` as `Delta` and closes with `TurnFinished`, exactly like a native turn.
pub struct ClaudeTurn {
    pub service: SessionService,
    pub root: PathBuf,
    pub session_id: String,
    pub agent: Agent,
    pub turn: Turn,
    pub prompt: String,
    pub model: String,
}

pub fn start(app: tauri::AppHandle, request: ClaudeTurn) -> Result<(), String> {
    let ClaudeTurn {
        service,
        root,
        session_id,
        agent,
        turn,
        prompt,
        model,
    } = request;
    let data_dir = super::index_dir();

    // Settings keep one model across providers. An OmniRoute or Gemini id is not a Claude model,
    // so only a Claude id is offered to the harness; otherwise its own default applies.
    let requested = model.starts_with("claude").then(|| model.clone());
    let model_name = requested
        .clone()
        .unwrap_or_else(|| "claude-sonnet".to_string());
    let mut config = ClaudeConfig::new(&root);
    config.model = requested;
    if !config.inject_credential(&credentials::KeychainCredentialStore) {
        return Err(
            "Claude needs an Anthropic key: add one in Settings, then send again.".to_string(),
        );
    }
    // The child is this binary's own `mcp` mode, pointed at the data directory the approval card
    // reads, so a question asked here is a question the window can answer.
    let mcp_config = write_mcp_config(&data_dir, &root, &session_id, &agent.id)?;
    let config = config.with_permission_tool(mcp_config, APPROVAL_TOOL);

    let cap = service_cap(&service, &session_id).unwrap_or(None);

    std::thread::Builder::new()
        .name("rigel-claude".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = app.emit(
                        "rigel://events",
                        AgentEvent::Error {
                            turn_id: Some(turn.id.clone()),
                            message: format!("could not start the turn runtime: {error}"),
                        },
                    );
                    return;
                }
            };

            let recorded = match runtime.block_on(Recorded::adopt(
                service,
                session_id.clone(),
                agent,
                turn.clone(),
                "claude",
                &prompt,
            )) {
                Ok(recorded) => recorded,
                Err(message) => {
                    let _ = app.emit(
                        "rigel://events",
                        AgentEvent::Error {
                            turn_id: Some(turn.id.clone()),
                            message,
                        },
                    );
                    return;
                }
            };

            let adapter = ClaudeAdapter::new(config);
            let mut items = Vec::new();
            let sink = app.clone();
            let delta_turn = recorded.turn.id.clone();
            let outcome = runtime.block_on(adapter.run_turn(&prompt, |event| match event {
                ClaudeEvent::Delta { text } => {
                    let _ = sink.emit(
                        "rigel://events",
                        AgentEvent::Delta {
                            turn_id: delta_turn.clone(),
                            kind: ItemKind::Message,
                            text,
                        },
                    );
                }
                ClaudeEvent::Item(item) => items.push(item),
                ClaudeEvent::Notice { .. }
                | ClaudeEvent::Started { .. }
                | ClaudeEvent::Message { .. } => {}
            }));

            runtime.block_on(recorded.record_items(&items, &[]));
            let usage = runtime.block_on(recorded.finish_claude(&outcome, &model_name));

            let (status, stop) = if outcome.is_ok() {
                ("completed", "completed")
            } else {
                ("failed", "harness_error")
            };
            if let Err(error) = &outcome {
                let _ = app.emit(
                    "rigel://events",
                    AgentEvent::Error {
                        turn_id: Some(recorded.turn.id.clone()),
                        message: error.to_string(),
                    },
                );
            }

            let spend = service_spend(&recorded).unwrap_or(0);
            let _ = app.emit(
                "rigel://events",
                AgentEvent::TurnFinished {
                    turn_id: recorded.turn.id.clone(),
                    status: status.to_string(),
                    stop: stop.to_string(),
                    usage: usage.unwrap_or_default(),
                    session_spend_micros: spend,
                    cap_micros: cap,
                },
            );
        })
        .map_err(|error| format!("could not start the Claude thread: {error}"))?;

    Ok(())
}

fn service_cap(service: &SessionService, session: &str) -> Result<Option<i64>, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    Ok(runtime
        .block_on(service.get(session))
        .ok()
        .and_then(|stored| stored.budget_cents))
}

/// The session's spend after the turn closed, so the window's meter moves with the bill.
fn service_spend(recorded: &Recorded) -> Result<i64, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    Ok(runtime
        .block_on(recorded.service.get(&recorded.session_id))
        .map(|session| session.spend_micros)
        .unwrap_or(0))
}

/// `rigel-app mcp --project P --session S --agent A [--data-dir D]`, the stdio server Claude
/// calls. Returns `None` when the arguments are not a subcommand, so a plain launch still opens
/// the window.
pub fn mcp_mode<I: IntoIterator<Item = String>>(args: I) -> Option<i32> {
    let args: Vec<String> = args.into_iter().collect();
    if args.iter().all(|arg| arg != "mcp") {
        return None;
    }

    let mut project = None;
    let mut session = None;
    let mut agent = None;
    let mut data_dir = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let mut take = |slot: &mut Option<String>| *slot = iter.next();
        match arg.as_str() {
            "--project" => take(&mut project),
            "--session" => take(&mut session),
            "--agent" => take(&mut agent),
            "--data-dir" => take(&mut data_dir),
            _ => {}
        }
    }

    let (Some(project), Some(session), Some(agent)) = (project, session, agent) else {
        eprintln!("rigel-app: mcp needs --project, --session and --agent");
        return Some(2);
    };

    Some(serve(
        PathBuf::from(project),
        session,
        agent,
        data_dir.map(PathBuf::from).unwrap_or_else(super::index_dir),
    ))
}

fn serve(project: PathBuf, session: String, agent: String, index_dir: PathBuf) -> i32 {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("rigel-app: mcp: {error}");
            return 2;
        }
    };

    runtime.block_on(async move {
        // A session or agent that does not exist is a config mistake, not a tool error: refuse
        // before reading a line, so Claude reports a dead server rather than ten failed calls.
        let service = match SessionService::open(&project).await {
            Ok(service) => service,
            Err(error) => {
                eprintln!("rigel-app: mcp: {error}");
                return 2;
            }
        };
        if let Err(error) = service.get(&session).await {
            eprintln!("rigel-app: mcp: {error}");
            return 1;
        }
        if let Err(error) = service.agent(&agent).await {
            eprintln!("rigel-app: mcp: {error}");
            return 1;
        }

        let ctx = rigel_mcp::McpContext {
            project_root: project,
            session_id: session,
            agent_id: agent,
            index_dir,
            approval_wait: APPROVAL_WAIT,
        };
        let stdin = BufReader::new(tokio::io::stdin());
        match rigel_mcp::serve(ctx, stdin, tokio::io::stdout()).await {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("rigel-app: mcp: {error}");
                2
            }
        }
    })
}

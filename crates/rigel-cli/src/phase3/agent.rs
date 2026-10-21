use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use rigel_core::{
    AgentEvent, BudgetService, Cancels, Coordinator, EventSink, Model, OpenRouterModel,
    ScriptedModel, SessionService, StopAt, StopReason, SubagentPolicy, Tools, credentials,
};
use serde_json::json;

use super::AgentCommand;

/// Human output streams; `--json` prints one event per line, which is what a test parses.
struct PrintSink {
    json: bool,
}

impl EventSink for PrintSink {
    fn emit(&self, event: AgentEvent) {
        if self.json {
            if let Ok(line) = serde_json::to_string(&event) {
                println!("{line}");
            }
            return;
        }
        match event {
            AgentEvent::TurnStarted { seq, .. } => println!("turn {seq}"),
            AgentEvent::Delta { text, .. } => {
                print!("{text}");
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            AgentEvent::Item { item } => match item.kind.as_str() {
                "mcp_call" => {
                    let tool = item
                        .body
                        .get("tool")
                        .and_then(|value| value.as_str())
                        .unwrap_or("?");
                    let ok = item
                        .body
                        .get("result")
                        .and_then(|value| value.get("ok"))
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false);
                    println!("\n  [{tool}] {}", if ok { "ok" } else { "error" });
                }
                "file_change" => {
                    let path = item
                        .body
                        .get("path")
                        .and_then(|value| value.as_str())
                        .unwrap_or("?");
                    let patch = item
                        .body
                        .get("patch_id")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    println!("\n  patched {path} ({})", &patch[..8.min(patch.len())]);
                }
                "error" => {
                    let message = item
                        .body
                        .get("message")
                        .and_then(|value| value.as_str())
                        .unwrap_or("error");
                    println!("\n  error: {message}");
                }
                _ => {}
            },
            AgentEvent::TurnFinished {
                status,
                stop,
                usage,
                session_spend_micros,
                cap_micros,
                ..
            } => {
                println!(
                    "\n{status} ({stop}) · {} tok · ${:.6}{}",
                    usage.total_tokens(),
                    session_spend_micros as f64 / 1_000_000.0,
                    cap_micros
                        .map(|cap| format!(" of ${:.2}", cap as f64 / 1_000_000.0))
                        .unwrap_or_default()
                );
            }
            AgentEvent::BudgetWarning {
                spend_micros,
                cap_micros,
                ..
            } => {
                println!(
                    "\nbudget: ${:.4} of ${:.2} — raise the cap or stop",
                    spend_micros as f64 / 1_000_000.0,
                    cap_micros as f64 / 1_000_000.0
                );
            }
            AgentEvent::BudgetExceeded {
                spend_micros,
                cap_micros,
                ..
            } => {
                println!(
                    "\nbudget exceeded: ${:.4} of ${:.2}; every running agent was interrupted",
                    spend_micros as f64 / 1_000_000.0,
                    cap_micros as f64 / 1_000_000.0
                );
            }
            AgentEvent::Error { message, .. } => println!("\nerror: {message}"),
            _ => {}
        }
    }
}

/// `omniroute` unless the caller or `RIGEL_PROVIDER` says otherwise. OmniRoute is Rigel's
/// default because it is the local router: one key, any model behind it.
fn provider_name(requested: Option<String>) -> String {
    requested
        .or_else(|| std::env::var("RIGEL_PROVIDER").ok())
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "omniroute".to_string())
}

fn default_model(provider: &str) -> String {
    match provider {
        "openrouter" => rigel_core::agent::openrouter::DEFAULT_MODEL.to_string(),
        _ => rigel_core::agent::omniroute::DEFAULT_MODEL.to_string(),
    }
}

/// The provider's model, with its key resolved. OmniRoute is started on demand: a local
/// router that is down is not a configuration error, it is a process to launch.
async fn provider_model(provider: &str) -> Result<Arc<dyn Model>, String> {
    match provider {
        "openrouter" => {
            let key = credentials::get(credentials::OPENROUTER).map_err(|error| {
                format!("{error}\n  set one with: rigel keys set openrouter (reads stdin)")
            })?;
            Ok(Arc::new(OpenRouterModel::new(key)))
        }
        "omniroute" => {
            let key = credentials::get(credentials::OMNIROUTE).map_err(|error| {
                format!(
                    "{error}\n  set one with: rigel keys set omniroute (reads stdin)\n  or run a deterministic script with --script"
                )
            })?;
            rigel_core::agent::omniroute::ensure_server().await?;
            Ok(Arc::new(rigel_core::agent::omniroute::model(key)))
        }
        other => Err(format!(
            "unknown provider `{other}`; use `omniroute` or `openrouter`"
        )),
    }
}

/// The newest active session, or none. Unlike [`resolve_session`] this never creates one:
/// listing agents and quoting a turn must not write to the database.
async fn find_session(
    service: &SessionService,
    requested: Option<&str>,
) -> Result<Option<rigel_core::Session>, String> {
    if let Some(id) = requested {
        return service
            .get(id)
            .await
            .map(Some)
            .map_err(|error| error.to_string());
    }
    let sessions = service.list().await.map_err(|error| error.to_string())?;
    Ok(sessions
        .into_iter()
        .find(|session| session.status != rigel_core::SessionStatus::Ended))
}

/// Price the first step of a turn without sending anything: no model call, no agent row, no
/// turn row and no chat history. A session is read when one exists so the quote uses its
/// spend and cap; when none exists the quote still works, with the cap reported as unset.
async fn quote_run(
    service: &SessionService,
    prompt: &str,
    requested: Option<&str>,
    provider: &str,
    model: Option<String>,
    script: Option<&std::path::Path>,
    as_json: bool,
) -> ExitCode {
    let session = match find_session(service, requested).await {
        Ok(session) => session,
        Err(error) => {
            eprintln!("rigel: {error}");
            return ExitCode::from(1);
        }
    };

    // The quote prices the exact request the first step would send: the same system prompt
    // and tool schemas the coordinator measures, plus the typed prompt.
    let model_name = model.unwrap_or_else(|| default_model(provider));
    let context_chars = rigel_core::agent::fixed_context_chars(&model_name);
    let quote = match &session {
        Some(session) => {
            let budget = BudgetService::with_null_interrupt(service.pool().clone());
            match budget
                .quote_session(&session.id, &model_name, context_chars, prompt)
                .await
            {
                Ok(quote) => quote,
                Err(error) => {
                    eprintln!("rigel: {error}");
                    return ExitCode::from(1);
                }
            }
        }
        None => rigel_core::quote_turn(&model_name, context_chars, prompt, 0, None),
    };

    if as_json {
        println!(
            "{}",
            json!({
                "ok": true,
                "dry_run": true,
                "session": session.as_ref().map(|session| session.id.clone()),
                "model": model_name,
                "script": script.is_some(),
                "quote": quote,
            })
        );
        return ExitCode::SUCCESS;
    }

    let remaining = match (quote.remaining_micros, quote.cap_micros) {
        (Some(left), Some(cap)) => format!(
            "${:.6} left of ${:.2}",
            left as f64 / 1_000_000.0,
            cap as f64 / 1_000_000.0
        ),
        _ => "no cap set".to_string(),
    };
    println!(
        "≈ ${:.6} first step · {remaining}",
        quote.first_step_micros as f64 / 1_000_000.0
    );
    println!(
        "  {} tok in · {} tok assumed out · {}",
        quote.input_tokens, quote.output_tokens, quote.model
    );
    if quote.over_remaining {
        println!(
            "  over the remaining budget; the cap would stop the turn after the step that crosses"
        );
    }
    if script.is_some() {
        println!("  scripted model: the real bill comes from the script's reported usage");
    }
    println!("dry run: nothing was sent, no agent or turn was created");
    ExitCode::SUCCESS
}

async fn resolve_session(
    service: &SessionService,
    requested: Option<&str>,
) -> Result<rigel_core::Session, String> {
    if let Some(id) = requested {
        return service.get(id).await.map_err(|error| error.to_string());
    }
    let sessions = service.list().await.map_err(|error| error.to_string())?;
    if let Some(found) = sessions
        .into_iter()
        .find(|session| session.status != rigel_core::SessionStatus::Ended)
    {
        return Ok(found);
    }
    service
        .start(Some("cli"), Some(500))
        .await
        .map_err(|error| error.to_string())
}

pub async fn run_agent(dir: &Path, command: AgentCommand) -> ExitCode {
    match command {
        AgentCommand::List {
            project,
            session,
            json: as_json,
        } => {
            let service = match SessionService::open(&project).await {
                Ok(service) => service,
                Err(error) => return crate::fail(error),
            };
            // Listing must not start a session as a side effect.
            let session = match find_session(&service, session.as_deref()).await {
                Ok(session) => session,
                Err(error) => {
                    eprintln!("rigel: {error}");
                    return ExitCode::from(1);
                }
            };
            let Some(session) = session else {
                if as_json {
                    println!("[]");
                } else {
                    println!("no session in this project yet");
                }
                return ExitCode::SUCCESS;
            };
            match service.agents(&session.id).await {
                Ok(agents) => {
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&agents).unwrap_or_default()
                        );
                    } else if agents.is_empty() {
                        println!("no agents in session {}", &session.id[..8]);
                    } else {
                        // A spawned child's role is its label (up to 40), so a fixed width would
                        // misalign every column after it.
                        let role_width = agents
                            .iter()
                            .map(|agent| agent.role.as_deref().map(str::len).unwrap_or(0))
                            .max()
                            .unwrap_or(0)
                            .max(10);
                        for agent in agents {
                            println!(
                                "{}  {:<8}  {:<role_width$}  ${:.4}  {}",
                                &agent.id[..8.min(agent.id.len())],
                                agent.harness,
                                agent.role.clone().unwrap_or_default(),
                                agent.spend_micros as f64 / 1_000_000.0,
                                agent.status
                            );
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => crate::fail(error),
            }
        }

        AgentCommand::Run {
            project,
            session,
            agent,
            prompt,
            dry_run,
            script,
            provider,
            model,
            crash_after_journal,
            max_steps,
            json: as_json,
        } => {
            let provider = provider_name(provider);
            let service = match SessionService::open(&project).await {
                Ok(service) => service,
                Err(error) => return crate::fail(error),
            };
            if dry_run {
                return quote_run(
                    &service,
                    &prompt,
                    session.as_deref(),
                    &provider,
                    model,
                    script.as_deref(),
                    as_json,
                )
                .await;
            }
            let session = match resolve_session(&service, session.as_deref()).await {
                Ok(session) => session,
                Err(error) => {
                    eprintln!("rigel: {error}");
                    return ExitCode::from(1);
                }
            };

            let model_name = model.clone().unwrap_or_else(|| {
                if script.is_some() {
                    "script".to_string()
                } else {
                    default_model(&provider)
                }
            });

            let worker = match service.agents(&session.id).await.ok().and_then(|agents| {
                agents.into_iter().find(|candidate| {
                    candidate.harness == "native"
                        && candidate.role.as_deref() == Some(agent.as_str())
                })
            }) {
                Some(worker) => worker,
                None => match service
                    .register_agent_with(&session.id, "native", Some(&agent), Some(&model_name))
                    .await
                {
                    Ok(worker) => worker,
                    Err(error) => return crate::fail(error),
                },
            };

            let (model, label): (Arc<dyn Model>, String) = match script {
                Some(path) => match ScriptedModel::from_path(&path) {
                    Ok(scripted) => {
                        let label = format!("script:{}", path.display());
                        (Arc::new(scripted.with_id(label.clone())), label)
                    }
                    Err(error) => {
                        eprintln!("rigel: {error}");
                        return ExitCode::from(1);
                    }
                },
                None => match provider_model(&provider).await {
                    Ok(model) => (model, model_name.clone()),
                    Err(error) => {
                        eprintln!("rigel: {error}");
                        return ExitCode::from(1);
                    }
                },
            };

            let cancels = Arc::new(Cancels::new());
            let budget = Arc::new(BudgetService::new(
                service.pool().clone(),
                cancels.clone() as Arc<dyn rigel_core::InterruptSink>,
            ));
            let stop_at = if crash_after_journal {
                StopAt::AfterJournal
            } else {
                StopAt::Never
            };
            let tools = Tools::new(
                project.clone(),
                service.clone(),
                budget.clone(),
                dir.to_path_buf(),
                stop_at,
            );
            let coordinator = Coordinator {
                session: service.clone(),
                budget,
                tools,
                model,
                sink: Arc::new(PrintSink { json: as_json }),
                cancels,
                project_root: project.clone(),
                model_name: label,
                max_steps,
                subagents: SubagentPolicy::default(),
                depth: 0,
                parent_cancel: None,
            };

            match coordinator.run(&session.id, &worker.id, &prompt).await {
                Ok(run) => {
                    let spend = service
                        .get(&session.id)
                        .await
                        .map(|stored| stored.spend_micros)
                        .unwrap_or(0);
                    if as_json {
                        println!(
                            "{}",
                            json!({
                                "ok": run.stop == StopReason::Completed,
                                "session": session.id,
                                "agent": worker.id,
                                "turn": run.turn.id,
                                "status": run.status,
                                "stop": run.stop.as_str(),
                                "steps": run.steps,
                                "tool_calls": run.tool_calls,
                                "usage": run.usage,
                                "session_spend_micros": spend,
                            })
                        );
                    }
                    match run.stop {
                        StopReason::Completed | StopReason::Cancelled => ExitCode::SUCCESS,
                        StopReason::BudgetExceeded => ExitCode::from(3),
                        StopReason::CrashSimulated => ExitCode::from(70),
                        StopReason::ModelError | StopReason::StepLimit => ExitCode::from(2),
                    }
                }
                Err(error) => {
                    if as_json {
                        println!("{}", json!({ "ok": false, "error": error.to_string() }));
                    }
                    eprintln!("rigel: {error}");
                    ExitCode::from(error.exit_code())
                }
            }
        }
    }
}

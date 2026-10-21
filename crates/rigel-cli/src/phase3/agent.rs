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
            let session = match resolve_session(&service, session.as_deref()).await {
                Ok(session) => session,
                Err(error) => {
                    eprintln!("rigel: {error}");
                    return ExitCode::from(1);
                }
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
            script,
            model,
            crash_after_journal,
            max_steps,
            json: as_json,
        } => {
            let service = match SessionService::open(&project).await {
                Ok(service) => service,
                Err(error) => return crate::fail(error),
            };
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
                    rigel_core::agent::openrouter::DEFAULT_MODEL.to_string()
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
                None => {
                    let key = match credentials::get(credentials::OPENROUTER) {
                        Ok(key) => key,
                        Err(error) => {
                            eprintln!(
                                "rigel: {error}\n  set one with: rigel keys set openrouter (reads stdin)\n  or run a deterministic script with --script"
                            );
                            return ExitCode::from(1);
                        }
                    };
                    (Arc::new(OpenRouterModel::new(key)), model_name.clone())
                }
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

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use rigel_core::{
    AgentEvent, BudgetService, Cancels, CommandApprover, CommandDecision, CommandRequest,
    Coordinator, Deliberator, EventSink, Model, ModelDeliberator, NoDeliberation, OpenRouterModel,
    OriginApprover, OriginDecision, OriginRequest, ScriptedModel, SessionService, StopAt,
    StopReason, SubagentPolicy, Tools, credentials,
};
use serde_json::json;

use super::AgentCommand;
use super::acp;

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
                    let result = tool_result(&item.body);
                    let path = result
                        .get("path")
                        .and_then(|value| value.as_str())
                        .unwrap_or("?");
                    let patch = result
                        .get("patch_id")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");
                    println!("\n  patched {path} ({})", &patch[..8.min(patch.len())]);
                }
                "command" => {
                    // A native tool item wraps the tool's own body under `result`.
                    let result = tool_result(&item.body);
                    let command = result
                        .get("command")
                        .and_then(|value| value.as_str())
                        .unwrap_or("?");
                    let code = result
                        .get("exit_code")
                        .and_then(|value| value.as_i64())
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| {
                            if result
                                .get("timed_out")
                                .and_then(|value| value.as_bool())
                                .unwrap_or(false)
                            {
                                "timed out".to_string()
                            } else if result.get("approved") == Some(&json!(false)) {
                                "denied".to_string()
                            } else {
                                "no exit code".to_string()
                            }
                        });
                    let destructive = result
                        .get("destructive")
                        .and_then(|value| value.as_array())
                        .map(|rows| !rows.is_empty())
                        .unwrap_or(false);
                    println!(
                        "\n  $ {command}  → {code}{}",
                        if destructive { "  · destructive" } else { "" }
                    );
                }
                "web_search" => {
                    let result = tool_result(&item.body);
                    let url = result
                        .get("url")
                        .and_then(|value| value.as_str())
                        .unwrap_or("?");
                    let status = result
                        .get("status")
                        .and_then(|value| value.as_u64())
                        .map(|status| status.to_string())
                        .unwrap_or_else(|| "denied".to_string());
                    println!("\n  fetched {url} ({status}, untrusted)");
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

/// The body of a native tool item, which the coordinator stores under `result`.
fn tool_result(body: &serde_json::Value) -> &serde_json::Value {
    body.get("result").unwrap_or(body)
}

/// Answers the two tools that leave the patch journal: `run_command` and `fetch_url`.
///
/// A command's exact text, directory and timeout are printed first, and only an explicit `a`,
/// `y` or `yes` lets it run. A fetch prints the URL and its origin, and asks for a scope: once,
/// this session, or always (persisted under the index dir). End of input, an unreadable line or
/// anything else denies — the same rule the ACP prompt follows.
struct CliApprover {
    mode: acp::ApprovalChoice,
}

#[async_trait::async_trait]
impl CommandApprover for CliApprover {
    async fn approve(&self, request: &CommandRequest) -> CommandDecision {
        match self.mode {
            acp::ApprovalChoice::Allow => CommandDecision::AllowOnce,
            acp::ApprovalChoice::Deny => CommandDecision::Deny,
            acp::ApprovalChoice::Ask => ask_command(request).await,
        }
    }
}

#[async_trait::async_trait]
impl OriginApprover for CliApprover {
    async fn approve(&self, request: &OriginRequest) -> OriginDecision {
        match self.mode {
            acp::ApprovalChoice::Allow => OriginDecision::AllowOnce,
            acp::ApprovalChoice::Deny => OriginDecision::Deny,
            acp::ApprovalChoice::Ask => ask_origin(request).await,
        }
    }
}

async fn ask_command(request: &CommandRequest) -> CommandDecision {
    let prompt = format_command(request);
    eprintln!("{prompt}");
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        match std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line) {
            Ok(0) | Err(_) => CommandDecision::Deny,
            Ok(_) => parse_command_answer(&line),
        }
    })
    .await
    .unwrap_or(CommandDecision::Deny)
}

/// The question, exactly as printed. Pure, so a test can assert that the exact text, the cwd
/// inside the project and the timeout are shown before consent (N2).
fn format_command(request: &CommandRequest) -> String {
    let destructive = if request.destructive.is_empty() {
        String::new()
    } else {
        let rows: String = request
            .destructive
            .iter()
            .map(|finding| {
                format!(
                    "\n  DESTRUCTIVE {}: {} ({})",
                    finding.id, finding.summary, finding.detail
                )
            })
            .collect();
        format!("\n  this command is classified destructive{rows}")
    };
    let deliberation = match &request.deliberation {
        Some(round) if round.available() => {
            let positions: String = round
                .positions
                .iter()
                .map(|position| {
                    let text: String = position.text.chars().take(400).collect();
                    format!("\n  [{}] {text}", position.stance)
                })
                .collect();
            format!(
                "\n  deliberation ({}):{positions}\n  {}",
                round.turn_id.as_deref().unwrap_or("no turn"),
                round.note
            )
        }
        Some(round) => format!("\n  deliberation: {}", round.note),
        None => String::new(),
    };
    format!(
        "\nThe agent wants to run a command:\n  {}\n  cwd: {} (inside {})\n  timeout: {}s{}{}\n  [a]llow once  [r]eject  (anything else rejects):",
        request.command,
        acp::plain_path(request.cwd.clone()).display(),
        acp::plain_path(request.project_root.clone()).display(),
        request.timeout_secs,
        destructive,
        deliberation
    )
}

/// One line to one decision. Only an explicit yes allows.
fn parse_command_answer(line: &str) -> CommandDecision {
    match line.trim() {
        "a" | "y" | "yes" => CommandDecision::AllowOnce,
        _ => CommandDecision::Deny,
    }
}

async fn ask_origin(request: &OriginRequest) -> OriginDecision {
    let prompt = format_origin(request);
    eprintln!("{prompt}");
    tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        match std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line) {
            Ok(0) | Err(_) => OriginDecision::Deny,
            Ok(_) => parse_origin_answer(&line),
        }
    })
    .await
    .unwrap_or(OriginDecision::Deny)
}

/// The question, exactly as printed. Pure, so a test can assert the URL and its origin are shown
/// before any request is sent.
fn format_origin(request: &OriginRequest) -> String {
    format!(
        "\nThe agent wants to fetch a URL:\n  {}\n  origin: {}\n  [o]nce  [s]ession  [a]lways  [r]eject  (anything else rejects):",
        request.url, request.origin
    )
}

/// One line to one scope. Only an explicit answer allows, and `a` here means **always** — the
/// network prompt has a scope the command prompt deliberately does not.
fn parse_origin_answer(line: &str) -> OriginDecision {
    match line.trim() {
        "o" | "once" => OriginDecision::AllowOnce,
        "s" | "session" => OriginDecision::AllowSession,
        "a" | "always" => OriginDecision::AllowAlways,
        _ => OriginDecision::Deny,
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
        "gemini" => rigel_core::agent::gemini::DEFAULT_MODEL.to_string(),
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
        "gemini" => {
            let key = credentials::get(credentials::GEMINI).map_err(|error| {
                format!(
                    "{error}\n  set one with: rigel keys set gemini (reads stdin), or GEMINI_API_KEY in the environment\n  the key comes from Google AI Studio: https://aistudio.google.com/apikey"
                )
            })?;
            Ok(Arc::new(rigel_core::agent::gemini::model(key)))
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
            "unknown provider `{other}`; use `omniroute`, `openrouter` or `gemini`"
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

        AgentCommand::Acp {
            project,
            session,
            agent,
            harness,
            prompt,
            approval,
            timeout_secs,
            exe,
            json: as_json,
        } => {
            acp::run(acp::AcpArgs {
                project,
                session,
                agent,
                harness,
                prompt,
                json: as_json,
                timeout_secs,
                approval,
                exe,
            })
            .await
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
            approval,
            crash_after_journal,
            max_steps,
            json: as_json,
        } => {
            let provider = provider_name(provider);
            // A machine-readable run has nobody to ask, so the default is no. An explicit
            // `--approval allow` is the only way a command runs unattended.
            let approval = approval.unwrap_or(if as_json {
                acp::ApprovalChoice::Deny
            } else {
                acp::ApprovalChoice::Ask
            });
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
            // A scripted model has a fixed number of steps; spending three on advice would
            // change the turn it was written to test. Scripted runs record that advice was not
            // gathered, and the approval gate is unchanged.
            let scripted = script.is_some();

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
            let deliberator: Arc<dyn Deliberator> = if scripted {
                Arc::new(NoDeliberation)
            } else {
                Arc::new(ModelDeliberator::new(
                    model.clone(),
                    label.clone(),
                    service.clone(),
                    budget.clone(),
                ))
            };
            let tools = Tools::new(
                project.clone(),
                service.clone(),
                budget.clone(),
                dir.to_path_buf(),
                stop_at,
            )
            .with_command_approval(Arc::new(CliApprover { mode: approval }))
            .with_origin_approval(Arc::new(CliApprover { mode: approval }))
            .with_deliberation(deliberator);
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

#[cfg(test)]
mod tests {
    use super::{
        CommandDecision, format_command, format_origin, parse_command_answer, parse_origin_answer,
    };
    use rigel_core::{CommandRequest, OriginDecision, OriginRequest};
    use std::path::PathBuf;

    fn request() -> CommandRequest {
        CommandRequest {
            command: "cargo test --workspace".to_string(),
            project_root: PathBuf::from("/work/demo"),
            cwd: PathBuf::from("/work/demo/crates/core"),
            timeout_secs: 120,
            destructive: Vec::new(),
            deliberation: None,
        }
    }

    #[test]
    fn the_prompt_shows_the_exact_text_and_scope_before_consent() {
        let prompt = format_command(&request());
        assert!(prompt.contains("cargo test --workspace"));
        assert!(prompt.contains("/work/demo/crates/core"));
        assert!(prompt.contains("/work/demo"));
        assert!(prompt.contains("120s"));
        assert!(prompt.contains("anything else rejects"));
    }

    #[test]
    fn a_destructive_finding_is_shown_before_the_question() {
        let mut request = request();
        request.command = "rm -rf /etc/secrets".to_string();
        request.destructive =
            rigel_core::classify(&request.command, &request.cwd, &request.project_root);
        let prompt = format_command(&request);
        assert!(prompt.contains("DESTRUCTIVE delete-outside-project"));
        assert!(prompt.contains("DESTRUCTIVE recursive-remove"));
        assert!(prompt.contains("rm -rf /etc/secrets"));
    }

    #[test]
    fn only_an_explicit_yes_allows_once() {
        for yes in ["a", "y", "yes", "  a  \n"] {
            assert_eq!(parse_command_answer(yes), CommandDecision::AllowOnce);
        }
        // `A` is the ACP prompt's "always allow"; this prompt has no such scope, so it is a
        // rejection rather than a silent upgrade.
        for no in ["", "n", "no", "A\n", "allow\n", "always", "r", "\n"] {
            assert_eq!(parse_command_answer(no), CommandDecision::Deny);
        }
    }

    #[test]
    fn the_network_prompt_names_the_url_and_the_origin_it_approves() {
        let prompt = format_origin(&OriginRequest {
            url: "https://example.com/docs/page".to_string(),
            origin: "https://example.com".to_string(),
        });
        assert!(prompt.contains("https://example.com/docs/page"));
        assert!(prompt.contains("origin: https://example.com"));
        assert!(prompt.contains("[a]lways"));
        assert!(prompt.contains("anything else rejects"));
    }

    #[test]
    fn the_network_answer_maps_to_the_three_scopes_or_denies() {
        assert_eq!(parse_origin_answer("o\n"), OriginDecision::AllowOnce);
        assert_eq!(parse_origin_answer("once"), OriginDecision::AllowOnce);
        assert_eq!(parse_origin_answer("s"), OriginDecision::AllowSession);
        assert_eq!(
            parse_origin_answer("session\n"),
            OriginDecision::AllowSession
        );
        assert_eq!(parse_origin_answer("a"), OriginDecision::AllowAlways);
        assert_eq!(parse_origin_answer("always\n"), OriginDecision::AllowAlways);
        for no in ["", "r", "reject", "y", "yes", "A", "once more"] {
            assert_eq!(parse_origin_answer(no), OriginDecision::Deny);
        }
    }
}

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rigel_core::{MemoryService, PatchPipeline, SessionService, WriteMode, credentials};
use serde_json::json;

use super::{KeyCommand, MemoryCommand, PatchCommand};

async fn newest_session(
    service: &SessionService,
    requested: Option<&str>,
) -> Result<rigel_core::Session, String> {
    if let Some(id) = requested {
        return service.get(id).await.map_err(|error| error.to_string());
    }
    let sessions = service.list().await.map_err(|error| error.to_string())?;
    sessions
        .into_iter()
        .find(|session| session.status != rigel_core::SessionStatus::Ended)
        .ok_or_else(|| "no session yet — `rigel session start` first".to_string())
}

pub async fn run_patch(command: PatchCommand) -> ExitCode {
    match command {
        PatchCommand::List {
            project,
            session,
            json: as_json,
        } => {
            let service = match SessionService::open(&project).await {
                Ok(service) => service,
                Err(error) => return crate::fail(error),
            };
            let session = match newest_session(&service, session.as_deref()).await {
                Ok(session) => session,
                Err(error) => {
                    eprintln!("rigel: {error}");
                    return ExitCode::from(1);
                }
            };
            let pipeline = PatchPipeline::new(&project, service.pool().clone());
            match pipeline.list(&session.id).await {
                Ok(patches) => {
                    if as_json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&patches).unwrap_or_default()
                        );
                    } else if patches.is_empty() {
                        println!("no patches");
                    } else {
                        for patch in patches {
                            println!(
                                "{}  {:<10}  {}",
                                &patch.id[..8.min(patch.id.len())],
                                patch.status,
                                patch.path
                            );
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => patch_failure(error, as_json),
            }
        }
        PatchCommand::Apply {
            project,
            id,
            json: as_json,
        } => {
            let service = match SessionService::open(&project).await {
                Ok(service) => service,
                Err(error) => return crate::fail(error),
            };
            let pipeline = PatchPipeline::new(&project, service.pool().clone());
            match pipeline.apply(&id).await {
                Ok(patch) => {
                    if as_json {
                        println!("{}", serde_json::to_string(&patch).unwrap_or_default());
                    } else {
                        println!("{}  {}", patch.status, patch.path);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => patch_failure(error, as_json),
            }
        }
        PatchCommand::Revert {
            project,
            id,
            json: as_json,
        } => {
            let service = match SessionService::open(&project).await {
                Ok(service) => service,
                Err(error) => return crate::fail(error),
            };
            let pipeline = PatchPipeline::new(&project, service.pool().clone());
            match pipeline.revert(&id).await {
                Ok(patch) => {
                    if as_json {
                        println!("{}", serde_json::to_string(&patch).unwrap_or_default());
                    } else {
                        println!("{}  {}", patch.status, patch.path);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => patch_failure(error, as_json),
            }
        }
    }
}

fn patch_failure(error: rigel_core::PatchError, as_json: bool) -> ExitCode {
    if as_json {
        println!("{}", json!({ "ok": false, "error": error.to_string() }));
    }
    eprintln!("rigel: {error}");
    let code = match error {
        rigel_core::PatchError::PatchNotFound(_)
        | rigel_core::PatchError::InvalidPath(_)
        | rigel_core::PatchError::OutsideRoot(_) => 1,
        _ => 2,
    };
    ExitCode::from(code)
}

pub async fn run_memory(command: MemoryCommand) -> ExitCode {
    match command {
        MemoryCommand::Read {
            project,
            agent,
            topic,
            json: as_json,
        } => {
            let service = match SessionService::open(&project).await {
                Ok(service) => service,
                Err(error) => return crate::fail(error),
            };
            let memory = MemoryService::new(&project, service.pool().clone());
            match memory.read(&agent, &topic) {
                Ok(Some(body)) => {
                    if as_json {
                        println!("{}", json!({ "ok": true, "topic": topic, "body": body }));
                    } else {
                        print!("{body}");
                    }
                    ExitCode::SUCCESS
                }
                Ok(None) => {
                    if as_json {
                        println!("{}", json!({ "ok": true, "topic": topic, "body": null }));
                    } else {
                        println!("no memory for {agent}/{topic}");
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => memory_failure(error, as_json),
            }
        }
        MemoryCommand::Write {
            project,
            agent,
            topic,
            text,
            mode,
            json: as_json,
        } => {
            let Some(mode) = WriteMode::parse(&mode) else {
                eprintln!("rigel: mode must be append or replace");
                return ExitCode::from(1);
            };
            let content = match text {
                Some(text) => text,
                None => {
                    let mut input = String::new();
                    if let Err(error) = std::io::stdin().read_to_string(&mut input) {
                        eprintln!("rigel: {error}");
                        return ExitCode::from(2);
                    }
                    input
                }
            };
            let service = match SessionService::open(&project).await {
                Ok(service) => service,
                Err(error) => return crate::fail(error),
            };
            let memory = MemoryService::new(&project, service.pool().clone());
            match memory.write(&agent, &topic, &content, mode).await {
                Ok(file) => {
                    if as_json {
                        println!("{}", serde_json::to_string(&file).unwrap_or_default());
                    } else {
                        println!("{}  {} words  {}", file.path, file.words, file.sha256);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => memory_failure(error, as_json),
            }
        }
    }
}

fn memory_failure(error: rigel_core::MemoryError, as_json: bool) -> ExitCode {
    if as_json {
        println!("{}", json!({ "ok": false, "error": error.to_string() }));
    }
    eprintln!("rigel: {error}");
    ExitCode::from(error.exit_code())
}

pub fn run_keys(command: KeyCommand) -> ExitCode {
    match command {
        KeyCommand::Status { json: as_json } => {
            let providers = [
                credentials::OMNIROUTE,
                credentials::OPENROUTER,
                "openai",
                "anthropic",
            ];
            let rows: Vec<_> = providers
                .iter()
                .map(|provider| {
                    json!({ "provider": provider, "present": credentials::present(provider) })
                })
                .collect();
            if as_json {
                println!("{}", json!({ "ok": true, "providers": rows }));
            } else {
                for row in rows {
                    println!(
                        "{:<12} {}",
                        row["provider"].as_str().unwrap_or("?"),
                        if row["present"].as_bool().unwrap_or(false) {
                            "key stored"
                        } else {
                            "no key"
                        }
                    );
                }
            }
            ExitCode::SUCCESS
        }
        KeyCommand::Set { provider } => {
            let mut value = String::new();
            if let Err(error) = std::io::stdin().read_to_string(&mut value) {
                eprintln!("rigel: {error}");
                return ExitCode::from(2);
            }
            let value = value.trim();
            if value.is_empty() {
                eprintln!("rigel: no key on stdin");
                return ExitCode::from(1);
            }
            match credentials::set(&provider, value) {
                Ok(()) => {
                    println!("stored {provider} in the OS keychain (value not printed)");
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("rigel: {error}");
                    ExitCode::from(2)
                }
            }
        }
        KeyCommand::Delete { provider } => match credentials::delete(&provider) {
            Ok(()) => {
                println!("deleted {provider}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("rigel: {error}");
                ExitCode::from(2)
            }
        },
    }
}

pub async fn run_mcp(dir: &Path, project: PathBuf, session: String, agent: String) -> ExitCode {
    let service = match SessionService::open(&project).await {
        Ok(service) => service,
        Err(error) => return crate::fail(error),
    };
    if let Err(error) = service.get(&session).await {
        eprintln!("rigel: {error}");
        return ExitCode::from(1);
    }
    if let Err(error) = service.agent(&agent).await {
        eprintln!("rigel: {error}");
        return ExitCode::from(1);
    }
    let ctx = rigel_mcp::McpContext {
        project_root: project,
        session_id: session,
        agent_id: agent,
        index_dir: dir.to_path_buf(),
    };
    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let stdout = tokio::io::stdout();
    match rigel_mcp::serve(ctx, stdin, stdout).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rigel: mcp: {error}");
            ExitCode::from(2)
        }
    }
}

//! One harness call recorded as a real session turn (P4.8, P4.19).
//!
//! The CLI and the window open, record and close a harness turn the same way: prompt, normalised
//! items, approvals, answer, a priced `Usage` row and the close. Keeping it here means a Codex turn
//! and a Claude turn cannot drift into two different bills, and the window's Claude path is not a
//! second implementation of the CLI's. A turn left running by a killed process is marked
//! `interrupted` on the next open for that agent only (P4.12).

use std::path::{Path, PathBuf};

use rigel_core::{Agent, BudgetService, CostSource, SessionService, Turn};
use serde_json::{Value, json};

use crate::{AdapterError, ClaudeTurn, NormalItem, TurnResult};

/// The session, agent and turn one harness call is billed to (P4.19).
pub struct Recorded {
    pub service: SessionService,
    pub session_id: String,
    pub agent: Agent,
    pub turn: Turn,
    /// Input tokens of this agent's last measured turn, for the next estimate.
    pub last_input: Option<u64>,
    pub harness: &'static str,
}

/// How a turn ended, so both adapters close it the same way.
pub enum TurnEnd {
    Answer(String),
    Failed(String),
}

/// The newest open session, or a new one. Mirrors `rigel agent run` so every harness shares a
/// session instead of inventing a second history.
pub async fn resolve_session(
    service: &SessionService,
    requested: Option<&str>,
    harness: &'static str,
) -> Result<rigel_core::Session, rigel_core::SessionError> {
    if let Some(id) = requested {
        return service.get(id).await;
    }
    match service
        .list()
        .await?
        .into_iter()
        .find(|session| session.status != rigel_core::SessionStatus::Ended)
    {
        Some(session) => Ok(session),
        None => service.start(Some(harness), Some(500)).await,
    }
}

impl Recorded {
    /// A previous run that was killed left a running turn behind. One process owns one turn at a
    /// time for an agent, so any running turn for it is dead, not live. Other agents in the
    /// session are untouched (P4.12).
    async fn close_stale(
        service: &SessionService,
        session_id: &str,
        agent_id: &str,
    ) -> Result<(), String> {
        for stale in service
            .running_turns(session_id)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|turn| turn.agent_id == agent_id)
        {
            let _ = service
                .finish_turn_with_reason(&stale.id, "failed", Some("interrupted"))
                .await;
            let _ = service.set_agent_status(agent_id, "interrupted").await;
        }
        Ok(())
    }

    pub async fn open(
        project: &Path,
        requested: Option<&str>,
        harness: &'static str,
        model: &str,
        prompt: &str,
    ) -> Result<Self, String> {
        let service = SessionService::open(project)
            .await
            .map_err(|error| error.to_string())?;
        let session = resolve_session(&service, requested, harness)
            .await
            .map_err(|error| error.to_string())?;

        let agent = match service
            .agents(&session.id)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|candidate| {
                candidate.harness == harness && candidate.role.as_deref() == Some(harness)
            }) {
            Some(agent) => agent,
            None => service
                .register_agent_with(&session.id, harness, Some(harness), Some(model))
                .await
                .map_err(|error| error.to_string())?,
        };

        // A previous `rigel codex` that was killed left a running turn behind. This command owns
        // one turn at a time, so any running turn for this agent is dead, not live. Other agents
        // in the session are untouched (P4.12).
        for stale in service
            .running_turns(&session.id)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|turn| turn.agent_id == agent.id)
        {
            let _ = service
                .finish_turn_with_reason(&stale.id, "failed", Some("interrupted"))
                .await;
            let _ = service.set_agent_status(&agent.id, "interrupted").await;
        }

        let last_input = service
            .transcript(&session.id)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|row| row.turn.agent_id == agent.id)
            .filter_map(|row| row.turn.usage)
            .next_back()
            .map(|usage| usage.input_tokens + usage.cached_input_tokens);

        let turn = service
            .append_turn(&agent.id, Some(prompt))
            .await
            .map_err(|error| error.to_string())?;
        let _ = service.set_agent_status(&agent.id, "running").await;
        let _ = service
            .append_item(
                &turn.id,
                "message",
                "complete",
                &json!({ "role": "user", "text": prompt }),
                &json!({ "source": "rigel-cli", "harness": harness }),
            )
            .await;

        Ok(Self {
            service,
            session_id: session.id,
            agent,
            turn,
            last_input,
            harness,
        })
    }

    /// Adopt a turn the caller already created and returned to its own UI. The window needs the
    /// row before the run starts, so this closes anything a killed process left running for this
    /// agent and records the prompt against that same turn instead of opening a second one.
    pub async fn adopt(
        service: SessionService,
        session_id: String,
        agent: Agent,
        turn: Turn,
        harness: &'static str,
        prompt: &str,
    ) -> Result<Self, String> {
        Self::close_stale(&service, &session_id, &agent.id).await?;

        let last_input = service
            .transcript(&session_id)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|row| row.turn.agent_id == agent.id)
            .filter_map(|row| row.turn.usage)
            .next_back()
            .map(|usage| usage.input_tokens + usage.cached_input_tokens);

        let _ = service.set_agent_status(&agent.id, "running").await;
        let _ = service
            .append_item(
                &turn.id,
                "message",
                "complete",
                &json!({ "role": "user", "text": prompt }),
                &json!({ "source": "rigel", "harness": harness }),
            )
            .await;

        Ok(Self {
            service,
            session_id,
            agent,
            turn,
            last_input,
            harness,
        })
    }

    /// Item metadata: the harness, plus whatever the adapter knows (a transport, a session id).
    fn meta(&self, extra: Value) -> Value {
        let mut map = match extra {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        map.insert("harness".to_string(), json!(self.harness));
        Value::Object(map)
    }

    /// Persist what the turn did before its answer is written, so a command appears above the
    /// message that reports it. This is what makes `rigel codex run` a transcript item rather
    /// than a printout (P4.8, P4.12).
    pub async fn record_items(&self, items: &[NormalItem], approvals: &[Value]) {
        for item in items {
            let _ = self
                .service
                .append_item(
                    &self.turn.id,
                    item.kind.as_str(),
                    "complete",
                    &item.body,
                    &self.meta(json!({})),
                )
                .await;
        }
        for approval in approvals {
            let _ = self
                .service
                .append_item(
                    &self.turn.id,
                    "approval",
                    "complete",
                    approval,
                    &self.meta(json!({})),
                )
                .await;
        }
    }

    /// Write the answer, the bill and the close of the turn. The usage row is what makes the
    /// next estimate use a measured number instead of the fixed allowance.
    pub async fn close_turn(
        &self,
        end: TurnEnd,
        usage: Option<rigel_core::Usage>,
        extra: Value,
    ) -> Option<rigel_core::Usage> {
        let meta = self.meta(extra);
        let text = match end {
            TurnEnd::Failed(message) => {
                let _ = self
                    .service
                    .append_item(
                        &self.turn.id,
                        "error",
                        "error",
                        &json!({ "message": message }),
                        &meta,
                    )
                    .await;
                let _ = self
                    .service
                    .finish_turn_with_reason(&self.turn.id, "failed", Some("harness_error"))
                    .await;
                let _ = self
                    .service
                    .set_agent_status(&self.agent.id, "failed")
                    .await;
                return None;
            }
            TurnEnd::Answer(text) => text,
        };

        let _ = self
            .service
            .append_item(
                &self.turn.id,
                "message",
                "complete",
                &json!({ "role": "assistant", "text": text }),
                &meta,
            )
            .await;

        if let Some(usage) = usage {
            let _ = self
                .service
                .append_item(
                    &self.turn.id,
                    "usage",
                    "complete",
                    &serde_json::to_value(usage).unwrap_or(Value::Null),
                    &meta,
                )
                .await;
            let budget = BudgetService::with_null_interrupt(self.service.pool().clone());
            if let Ok(rigel_core::BudgetOutcome::Exceeded) =
                budget.account(&self.agent.id, &self.turn.id, &usage).await
            {
                eprintln!("rigel: this turn crossed the session cap; no further turn will start");
            }
        }

        let _ = self
            .service
            .finish_turn_with_reason(&self.turn.id, "completed", Some("completed"))
            .await;
        let _ = self.service.set_agent_status(&self.agent.id, "idle").await;
        usage
    }

    /// Codex reports a transport and its own token counts.
    pub async fn finish_codex(
        &self,
        result: &Result<TurnResult, AdapterError>,
        model: &str,
    ) -> Option<rigel_core::Usage> {
        match result {
            Ok(turn) => {
                let usage = turn.usage.map(|reported| rigel_core::Usage {
                    input_tokens: reported.input_tokens,
                    cached_input_tokens: reported.cached_input_tokens,
                    output_tokens: reported.output_tokens,
                    reasoning_tokens: reported.reasoning_output_tokens,
                    cost_micros: rigel_core::agent::pricing::cost_micros(
                        model,
                        reported.input_tokens,
                        reported.cached_input_tokens,
                        reported.output_tokens + reported.reasoning_output_tokens,
                    ),
                    provider_micros: None,
                    source: CostSource::Estimated,
                });
                self.close_turn(
                    TurnEnd::Answer(turn.text.clone()),
                    usage,
                    json!({ "transport": turn.transport.as_str() }),
                )
                .await
            }
            Err(error) => {
                self.close_turn(TurnEnd::Failed(error.to_string()), None, json!({}))
                    .await
            }
        }
    }

    /// Claude reports the CLI's own `total_cost_usd`, so a provider cost wins when present.
    pub async fn finish_claude(
        &self,
        result: &Result<ClaudeTurn, AdapterError>,
        model: &str,
    ) -> Option<rigel_core::Usage> {
        match result {
            Ok(turn) => {
                let usage = turn.usage.map(|reported| rigel_core::Usage {
                    input_tokens: reported.input_tokens,
                    cached_input_tokens: reported.cached_input_tokens,
                    output_tokens: reported.output_tokens,
                    reasoning_tokens: 0,
                    cost_micros: rigel_core::agent::pricing::cost_micros(
                        model,
                        reported.input_tokens,
                        reported.cached_input_tokens,
                        reported.output_tokens,
                    ),
                    provider_micros: reported.cost_micros,
                    source: if reported.cost_micros.is_some() {
                        CostSource::Provider
                    } else {
                        CostSource::Estimated
                    },
                });
                self.close_turn(
                    TurnEnd::Answer(turn.text.clone()),
                    usage,
                    json!({ "session_id": turn.session_id }),
                )
                .await
            }
            Err(error) => {
                self.close_turn(TurnEnd::Failed(error.to_string()), None, json!({}))
                    .await
            }
        }
    }
}

/// The MCP config Claude reads with `--mcp-config`: one stdio server, this binary, this project.
///
/// The data directory is spelled out so the child resolves the same approval channel the caller
/// watches, even when `--data-dir` pointed somewhere other than the default.
pub fn write_mcp_config(
    dir: &Path,
    project: &Path,
    session: &str,
    agent: &str,
) -> Result<PathBuf, String> {
    let exe =
        std::env::current_exe().map_err(|error| format!("could not find this binary: {error}"))?;
    let config = json!({
        "mcpServers": {
            "rigel": {
                "command": exe,
                "args": [
                    "--data-dir", dir.display().to_string(),
                    "mcp",
                    "--project", project.display().to_string(),
                    "--session", session,
                    "--agent", agent,
                ],
            }
        }
    });
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    let path = dir.join("claude-mcp.json");
    let text = serde_json::to_string_pretty(&config).map_err(|error| error.to_string())?;
    std::fs::write(&path, text)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod session_tests {
    use super::*;
    use crate::{ClaudeUsage, CodexUsage, Transport};
    use std::time::Duration;

    fn result(input: u64, cached: u64, output: u64) -> TurnResult {
        TurnResult {
            text: "pong".to_string(),
            usage: Some(CodexUsage {
                input_tokens: input,
                cached_input_tokens: cached,
                output_tokens: output,
                reasoning_output_tokens: 0,
            }),
            transport: Transport::AppServer,
            thread_id: Some("th-1".to_string()),
            duration: Duration::from_millis(5),
            fell_back: false,
        }
    }

    #[tokio::test]
    async fn a_codex_turn_records_its_tokens_and_the_next_estimate_uses_them() {
        let dir = tempfile::tempdir().unwrap();
        let first = Recorded::open(dir.path(), None, "codex", "gpt-test", "say pong")
            .await
            .expect("a session opens");
        assert_eq!(
            first.last_input, None,
            "a fresh session has nothing to copy"
        );

        let recorded = first
            .finish_codex(&Ok(result(155_895, 95, 4)), "gpt-test")
            .await
            .expect("the turn reported usage");
        assert_eq!(recorded.input_tokens, 155_895);
        assert_eq!(recorded.cached_input_tokens, 95);
        assert!(recorded.cost_micros > 0, "a price is attached to the row");

        let stored = first.service.turn(&first.turn.id).await.unwrap();
        assert_eq!(stored.status, "completed");
        assert_eq!(
            stored
                .usage
                .expect("usage_json is what reconcile reads")
                .input_tokens,
            155_895
        );

        let second = Recorded::open(dir.path(), None, "codex", "gpt-test", "again")
            .await
            .expect("the same session reopens");
        assert_eq!(
            second.last_input,
            Some(155_895 + 95),
            "the next estimate starts from a measured turn"
        );
    }

    #[tokio::test]
    async fn a_killed_turn_is_marked_interrupted_and_another_agent_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let recorded = Recorded::open(dir.path(), None, "codex", "gpt-test", "first")
            .await
            .unwrap();
        let session = recorded
            .service
            .agent(&recorded.turn.agent_id)
            .await
            .unwrap()
            .session_id;

        // A second agent in the same session, with work in flight. Opening a Codex turn must not
        // touch it: P4.12's promise is that killing one agent leaves the others running.
        let other = recorded
            .service
            .register_agent_with(&session, "native", Some("worker"), None)
            .await
            .unwrap();
        let other_turn = recorded
            .service
            .append_turn(&other.id, Some("still working"))
            .await
            .unwrap();

        // The first Codex turn was never finished: exactly what a killed process leaves behind.
        let reopened = Recorded::open(dir.path(), None, "codex", "gpt-test", "second")
            .await
            .unwrap();
        let stale = reopened.service.turn(&recorded.turn.id).await.unwrap();
        assert_eq!(stale.status, "failed");
        assert_eq!(stale.stop_reason.as_deref(), Some("interrupted"));

        let untouched = reopened.service.turn(&other_turn.id).await.unwrap();
        assert_eq!(untouched.status, "running");
    }

    #[tokio::test]
    async fn a_claude_turn_is_billed_to_its_own_agent_with_the_cli_cost() {
        let dir = tempfile::tempdir().unwrap();
        let recorded = Recorded::open(dir.path(), None, "claude", "claude-sonnet", "hello")
            .await
            .expect("a session opens");
        assert_eq!(recorded.agent.harness, "claude");
        assert_eq!(recorded.agent.role.as_deref(), Some("claude"));

        let turn = ClaudeTurn {
            text: "pong".to_string(),
            usage: Some(ClaudeUsage {
                input_tokens: 1_200,
                cached_input_tokens: 200,
                output_tokens: 30,
                cost_micros: Some(4_500),
            }),
            session_id: Some("claude-sess-1".to_string()),
            duration: Duration::from_millis(5),
        };
        let usage = recorded
            .finish_claude(&Ok(turn), "claude-sonnet")
            .await
            .expect("the turn reported usage");
        assert_eq!(usage.input_tokens, 1_200);
        assert_eq!(
            usage.provider_micros,
            Some(4_500),
            "the CLI's own cost is recorded, not replaced by the table"
        );
        assert!(usage.cost_micros > 0, "a local price is attached as well");

        let stored = recorded
            .service
            .transcript(&recorded.session_id)
            .await
            .unwrap();
        let usage_item = stored
            .iter()
            .flat_map(|row| &row.items)
            .find(|item| item.kind == "usage")
            .expect("the bill is a transcript item");
        let raw = usage_item
            .raw
            .as_ref()
            .expect("the item keeps its provenance");
        assert_eq!(raw["harness"], "claude");
    }

    #[tokio::test]
    async fn adopting_a_turn_records_onto_the_row_the_caller_returned() {
        let dir = tempfile::tempdir().unwrap();
        let first = Recorded::open(dir.path(), None, "claude", "claude-sonnet", "first")
            .await
            .unwrap();
        first
            .close_turn(TurnEnd::Answer("done".to_string()), None, json!({}))
            .await;

        // What `turn_send` does: create the row, hand it to the renderer, then run.
        let turn = first
            .service
            .append_turn(&first.agent.id, Some("second"))
            .await
            .unwrap();
        let adopted = Recorded::adopt(
            first.service.clone(),
            first.session_id.clone(),
            first.agent.clone(),
            turn.clone(),
            "claude",
            "second",
        )
        .await
        .expect("the row is adopted, not duplicated");

        assert_eq!(adopted.turn.id, turn.id);
        let rows = adopted.service.transcript(&first.session_id).await.unwrap();
        let row = rows
            .iter()
            .find(|row| row.turn.id == turn.id)
            .expect("one turn, the one the window showed");
        assert!(
            row.items
                .iter()
                .any(|item| item.kind == "message" && item.body["text"] == "second"),
            "the prompt lands on the returned row"
        );

        adopted
            .close_turn(TurnEnd::Answer("again".to_string()), None, json!({}))
            .await;
        let stored = adopted.service.turn(&turn.id).await.unwrap();
        assert_eq!(stored.status, "completed");
    }

    #[tokio::test]
    async fn the_mcp_config_names_this_binary_and_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_mcp_config(
            dir.path(),
            Path::new("C:/work/project"),
            "session-1",
            "agent-1",
        )
        .expect("the config is written");

        let text = std::fs::read_to_string(&path).unwrap();
        let config: Value = serde_json::from_str(&text).unwrap();
        let server = &config["mcpServers"]["rigel"];
        let command = server["command"].as_str().unwrap();
        let current = std::env::current_exe().unwrap().display().to_string();
        assert_eq!(command, current, "the child runs this build");

        let args: Vec<&str> = server["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|arg| arg.as_str())
            .collect();
        assert!(args.contains(&"mcp"), "the stdio server is the subcommand");
        assert!(args.contains(&"session-1"));
        assert!(args.contains(&"agent-1"));
        assert!(
            args.contains(&dir.path().display().to_string().as_str()),
            "the child is told the same data directory the pump watches"
        );
    }
}

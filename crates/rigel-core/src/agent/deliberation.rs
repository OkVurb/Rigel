//! The round table before a destructive command (D32).
//!
//! Three read-only advisors — advocate, opponent and referee — each get one bounded model call
//! with the command, its working directory and the policy rows the classifier matched. They have
//! no tools, so they cannot read, write, fetch or approve anything. Their answers are advisory:
//! the approval gate never reads them to decide, and the human approval is still the only thing
//! that lets the command run. A prompt-injected model cannot vote the gate away.
//!
//! The round is its own turn and its own bill, so the cost of asking is visible next to the
//! command. [`NoDeliberation`] is the headless default: it records that advice was unavailable
//! rather than pretending three agents spoke.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::destructive::Finding;
use super::model::{usage_from_raw, ChatMessage, ChatRequest, Delta, Model};
use crate::budget::BudgetService;
use crate::domain::Usage;
use crate::session::SessionService;

const ADVISOR: &str = "You are one of three read-only advisors consulted before a destructive \
shell command. You have no tools and cannot read the project, approve, deny or execute anything. \
The human decides; you must not claim authority to decide. Be concrete and brief (at most 120 \
words), and name only what is visible in the material you were given.";

const STANCES: [(&str, &str); 3] = [
    (
        "advocate",
        "Give the strongest honest case FOR running this command now. What real work does it \
advance, and why does this exact command fit?",
    ),
    (
        "opponent",
        "Give the strongest honest case AGAINST running this command now. Name what could be lost \
that has no other copy, and what irreversible step it contains.",
    ),
    (
        "referee",
        "State the facts a human should check before deciding: what the command actually matches, \
what the classifier may have missed, and what the safer equivalent would be. Do not vote.",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Position {
    /// `advocate`, `opponent` or `referee`.
    pub stance: String,
    pub text: String,
    pub tokens: u64,
    pub cost_micros: i64,
}

/// What the round produced. `turn_id` is the turn whose transcript holds the three messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Deliberation {
    pub turn_id: Option<String>,
    pub positions: Vec<Position>,
    pub note: String,
}

impl Deliberation {
    pub fn available(&self) -> bool {
        !self.positions.is_empty()
    }
}

/// Who produces the round. Implementations must not be able to stop or allow a command.
#[async_trait]
pub trait Deliberator: Send + Sync {
    async fn deliberate(
        &self,
        agent_id: &str,
        command: &str,
        cwd: &Path,
        findings: &[Finding],
    ) -> Deliberation;
}

/// The headless default. It says advice was unavailable, which is the truth.
pub struct NoDeliberation;

#[async_trait]
impl Deliberator for NoDeliberation {
    async fn deliberate(
        &self,
        _agent_id: &str,
        _command: &str,
        _cwd: &Path,
        _findings: &[Finding],
    ) -> Deliberation {
        Deliberation {
            turn_id: None,
            positions: Vec::new(),
            note: "deliberation is not configured for this run; the approval gate is unchanged"
                .to_string(),
        }
    }
}

/// Three calls to the parent's model, recorded as one turn under the same agent. Reusing the
/// parent's model keeps this working on every provider — the cheap-subagent allowlist is
/// OpenRouter-shaped — but it also means the round costs what the parent's model costs.
pub struct ModelDeliberator {
    model: Arc<dyn Model>,
    model_name: String,
    session: SessionService,
    budget: Arc<BudgetService>,
    /// The session's reasoning effort, so the advisory round runs at the same level as the turn
    /// it is advising (D28). `None` leaves the provider default.
    effort: Option<String>,
}

impl ModelDeliberator {
    pub fn new(
        model: Arc<dyn Model>,
        model_name: impl Into<String>,
        session: SessionService,
        budget: Arc<BudgetService>,
        effort: Option<String>,
    ) -> Self {
        Self {
            model,
            model_name: model_name.into(),
            session,
            budget,
            effort,
        }
    }

    async fn one(&self, stance: &'static str, question: &str, context: &str) -> (Position, Usage) {
        let request = ChatRequest {
            model: self.model_name.clone(),
            messages: vec![
                ChatMessage::system(ADVISOR),
                ChatMessage::user(format!("{context}\n\n{question}")),
            ],
            tools: Vec::new(),
            max_tokens: Some(320),
            temperature: 0.2,
            effort: self.effort.clone(),
        };
        let mut text = String::new();
        let mut usage = Usage::default();
        let cancel = CancellationToken::new();
        let mut sink = |delta: Delta| match delta {
            Delta::Text(piece) => text.push_str(&piece),
            Delta::Usage(raw) => usage = usage_from_raw(raw, &self.model_name),
            _ => {}
        };
        let outcome = self.model.chat(request, &cancel, &mut sink).await;
        let text = match outcome {
            Ok(()) => text.trim().to_string(),
            Err(error) => format!("unavailable: {error}"),
        };
        let text = if text.chars().count() > 900 {
            let cut: String = text.chars().take(900).collect();
            format!("{cut}…")
        } else if text.is_empty() {
            "no answer".to_string()
        } else {
            text
        };
        (
            Position {
                stance: stance.to_string(),
                text,
                tokens: usage.total_tokens(),
                cost_micros: usage.cost_micros,
            },
            usage,
        )
    }
}

#[async_trait]
impl Deliberator for ModelDeliberator {
    async fn deliberate(
        &self,
        agent_id: &str,
        command: &str,
        cwd: &Path,
        findings: &[Finding],
    ) -> Deliberation {
        let rows = if findings.is_empty() {
            "  (the classifier matched no rows)".to_string()
        } else {
            findings
                .iter()
                .map(|finding| {
                    format!(
                        "  - {}: {} ({})",
                        finding.id, finding.summary, finding.detail
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let context = format!(
            "The command the agent wants to run:\n  {command}\n\
             Working directory: {}\nPolicy rows the deterministic classifier matched:\n{rows}\n",
            cwd.display()
        );

        let turn = match self
            .session
            .append_turn(agent_id, Some("deliberation before a destructive command"))
            .await
        {
            Ok(turn) => turn,
            Err(error) => {
                return Deliberation {
                    turn_id: None,
                    positions: Vec::new(),
                    note: format!(
                        "could not open a deliberation turn: {error}; the gate is unchanged"
                    ),
                };
            }
        };

        let futures = STANCES
            .iter()
            .map(|(stance, question)| self.one(stance, question, &context));
        let results = futures::future::join_all(futures).await;

        let mut positions = Vec::with_capacity(results.len());
        let mut total = Usage::default();
        for (position, usage) in results {
            total.add(&usage);
            positions.push(position);
        }

        let note = match self.budget.account(agent_id, &turn.id, &total).await {
            Ok(_) => "advisory only: the human approval still decides".to_string(),
            Err(error) => {
                format!("advisory only; the round could not be billed ({error})")
            }
        };

        for position in &positions {
            let body = serde_json::json!({
                "role": "assistant",
                "stance": &position.stance,
                "text": &position.text,
                "cost_micros": position.cost_micros,
                "tokens": position.tokens,
            });
            let raw = serde_json::json!({
                "provider": "deliberation",
                "model": self.model_name,
                "kind": "message",
            });
            let _ = self
                .session
                .append_item(&turn.id, "message", "ok", &body, &raw)
                .await;
        }
        let _ = self
            .session
            .finish_turn_with_reason(&turn.id, "completed", Some("completed"))
            .await;

        Deliberation {
            turn_id: Some(turn.id),
            positions,
            note,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Deliberator, ModelDeliberator};
    use crate::agent::destructive::{classify, Finding};
    use crate::agent::model::{ChatRequest, Delta, DeltaSink, Model, ModelError};
    use crate::agent::ScriptedModel;
    use crate::{BudgetService, Cancels, SessionService};
    use async_trait::async_trait;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tokio_util::sync::CancellationToken;

    const SCRIPT: &str = r#"{
      "steps": [
        {"text": "Pro: the migration is ready and the old branch is obsolete.", "usage": {"input_tokens": 40, "output_tokens": 20, "cost_usd": 0.0002}},
        {"text": "Con: it rewrites history other people have pulled.", "usage": {"input_tokens": 40, "output_tokens": 20, "cost_usd": 0.0002}},
        {"text": "Check that a tagged backup exists and no one else is building on the branch.", "usage": {"input_tokens": 40, "output_tokens": 20, "cost_usd": 0.0002}}
      ]
    }"#;

    #[tokio::test]
    async fn three_advisors_answer_in_one_billed_turn() {
        let tmp = tempfile::tempdir().unwrap();
        let service = SessionService::open(tmp.path()).await.unwrap();
        let session = service.start(Some("native"), Some(25)).await.unwrap();
        let agent = service
            .register_agent(&session.id, "native", Some("worker"))
            .await
            .unwrap();
        let cancels = Arc::new(Cancels::new());
        let budget = Arc::new(BudgetService::new(
            service.pool().clone(),
            cancels as Arc<dyn crate::InterruptSink>,
        ));
        let model = Arc::new(ScriptedModel::from_json(SCRIPT).unwrap());
        let deliberator = ModelDeliberator::new(model, "script", service.clone(), budget, None);
        let findings: Vec<Finding> = classify(
            "git push --force origin main",
            Path::new("/work/demo"),
            Path::new("/work/demo"),
        );

        let round = deliberator
            .deliberate(
                &agent.id,
                "git push --force origin main",
                Path::new("/work/demo"),
                &findings,
            )
            .await;

        assert!(round.available());
        let stances: Vec<&str> = round.positions.iter().map(|p| p.stance.as_str()).collect();
        assert_eq!(stances, vec!["advocate", "opponent", "referee"]);
        assert!(round.positions[1].text.starts_with("Con:"));
        assert_eq!(
            round.positions.iter().map(|p| p.cost_micros).sum::<i64>(),
            600
        );
        let turn_id = round.turn_id.expect("one turn for the round");
        let items = service.items(&turn_id).await.unwrap();
        assert_eq!(items.len(), 3);
        assert!(items.iter().all(|item| item.kind.as_str() == "message"));
        let stored = service.get(&session.id).await.unwrap();
        assert!(stored.spend_micros >= 600, "the round is billed");
        assert!(round.note.contains("human approval still decides"));
    }

    /// Records the effort on every request it receives, and answers in one chunk.
    struct EffortSpy {
        seen: Mutex<Vec<Option<String>>>,
    }

    #[async_trait]
    impl Model for EffortSpy {
        fn id(&self) -> String {
            "effort-spy".to_string()
        }

        async fn chat(
            &self,
            request: ChatRequest,
            _cancel: &CancellationToken,
            on_delta: &mut DeltaSink<'_>,
        ) -> Result<(), ModelError> {
            self.seen.lock().unwrap().push(request.effort.clone());
            on_delta(Delta::Text("one opinion".to_string()));
            on_delta(Delta::Finish("stop".to_string()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn the_advisory_round_sends_the_sessions_effort() {
        let tmp = tempfile::tempdir().unwrap();
        let service = SessionService::open(tmp.path()).await.unwrap();
        let session = service.start(Some("native"), Some(25)).await.unwrap();
        let agent = service
            .register_agent(&session.id, "native", Some("worker"))
            .await
            .unwrap();
        let cancels = Arc::new(Cancels::new());
        let budget = Arc::new(BudgetService::new(
            service.pool().clone(),
            cancels as Arc<dyn crate::InterruptSink>,
        ));
        let spy = Arc::new(EffortSpy {
            seen: Mutex::new(Vec::new()),
        });
        let deliberator = ModelDeliberator::new(
            spy.clone(),
            "effort-spy",
            service.clone(),
            budget,
            Some("high".to_string()),
        );

        let findings: Vec<Finding> = classify(
            "git push --force origin main",
            Path::new("/work/demo"),
            Path::new("/work/demo"),
        );
        let round = deliberator
            .deliberate(
                &agent.id,
                "git push --force origin main",
                Path::new("/work/demo"),
                &findings,
            )
            .await;

        assert_eq!(round.positions.len(), 3);
        let seen = spy.seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 3, "advocate, opponent and referee");
        assert!(
            seen.iter().all(|effort| effort.as_deref() == Some("high")),
            "every advisory request carries the session's effort: {seen:?}"
        );
    }

    #[tokio::test]
    async fn a_model_failure_is_recorded_as_an_unavailable_advisor() {
        let tmp = tempfile::tempdir().unwrap();
        let service = SessionService::open(tmp.path()).await.unwrap();
        let session = service.start(Some("native"), Some(25)).await.unwrap();
        let agent = service
            .register_agent(&session.id, "native", Some("worker"))
            .await
            .unwrap();
        let cancels = Arc::new(Cancels::new());
        let budget = Arc::new(BudgetService::new(
            service.pool().clone(),
            cancels as Arc<dyn crate::InterruptSink>,
        ));
        // Each step fails, so no advisor has an opinion; the round still records the turn.
        let script = r#"{"steps": [
            {"error": "provider down"}, {"error": "provider down"}, {"error": "provider down"}
        ]}"#;
        let model = Arc::new(ScriptedModel::from_json(script).unwrap());
        let deliberator = ModelDeliberator::new(model, "script", service.clone(), budget, None);

        let round = deliberator
            .deliberate(&agent.id, "rm -rf /", Path::new("/work/demo"), &[])
            .await;

        assert_eq!(round.positions.len(), 3);
        assert!(
            round
                .positions
                .iter()
                .all(|p| p.text.contains("unavailable")),
            "a silent advisor must not look like agreement"
        );
    }
}

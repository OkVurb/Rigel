//! The native loop end to end, with a scripted model.
//!
//! These are the same paths the Phase 3 exit test drives through the CLI: read three files,
//! patch one, stop at the cap, die after the journal. No network and no real key, but the
//! coordinator, the tools, the budget service, the patch pipeline and the session store are
//! the production ones.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rigel_core::agent::{
    Cancels, ChatRequest, Coordinator, Delta, DeltaSink, Model, ModelError, RawUsage,
    ScriptedModel, StopReason, Tools,
};
use rigel_core::{BudgetService, SessionService, StopAt, Usage};
use tokio_util::sync::CancellationToken;

const SCRIPT: &str = r#"{
  "steps": [
    {
      "text": "Reading first.\n",
      "tool_calls": [
        {"name": "read_file", "arguments": {"path": "README.md"}},
        {"name": "read_file", "arguments": {"path": "src/lib.rs"}},
        {"name": "read_file", "arguments": {"path": "src/main.rs"}}
      ],
      "usage": {"input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002}
    },
    {
      "text": "Patching the subtraction.\n",
      "tool_calls": [
        {"name": "edit_file", "arguments": {
          "path": "src/lib.rs",
          "old_string": "    a - b",
          "new_string": "    a.saturating_sub(b)"
        }}
      ],
      "usage": {"input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002}
    },
    {
      "text": "Done; no tests were run.\n",
      "usage": {"input_tokens": 50, "output_tokens": 10, "cost_usd": 0.0001}
    }
  ]
}"#;

fn repo() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("README.md"), "# Demo\nThree files.\n").unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn sub(a: i32, b: i32) -> i32 {\n    a - b\n}\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("src/main.rs"),
        "fn main() {\n    println!(\"{}\", demo::add(2, 3));\n}\n",
    )
    .unwrap();
    tmp
}

fn coordinator(
    root: &std::path::Path,
    service: &SessionService,
    script: &str,
    stop: StopAt,
    max_steps: usize,
) -> Coordinator {
    let cancels = Arc::new(Cancels::new());
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels.clone() as Arc<dyn rigel_core::InterruptSink>,
    ));
    let tools = Tools::new(
        root.to_path_buf(),
        service.clone(),
        budget.clone(),
        root.join("index"),
        stop,
    );
    Coordinator {
        session: service.clone(),
        budget,
        tools,
        model: Arc::new(ScriptedModel::from_json(script).unwrap()),
        sink: Arc::new(rigel_core::NullSink),
        cancels,
        project_root: root.to_path_buf(),
        model_name: "script".to_string(),
        effort: None,
        max_steps,
        subagents: rigel_core::agent::SubagentPolicy::default(),
        depth: 0,
        parent_cancel: None,
        worktree_dir: None,
        chat_parent: None,
    }
}

#[tokio::test]
async fn a_scripted_turn_reads_three_files_and_patches_one() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("native"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let run = coordinator(tmp.path(), &service, SCRIPT, StopAt::Never, 10)
        .run(&session.id, &agent.id, "make sub overflow-safe")
        .await
        .unwrap();

    assert_eq!(run.stop, StopReason::Completed);
    assert_eq!(run.status, "completed");
    assert_eq!(run.steps, 3);
    assert_eq!(run.tool_calls, 4);
    assert_eq!(run.usage.cost_micros, 500);

    let body = std::fs::read_to_string(tmp.path().join("src/lib.rs")).unwrap();
    assert!(body.contains("a.saturating_sub(b)"));
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("src/main.rs")).unwrap(),
        "fn main() {\n    println!(\"{}\", demo::add(2, 3));\n}\n",
        "the files that were only read are untouched"
    );

    let patches = rigel_core::PatchPipeline::new(tmp.path(), service.pool().clone())
        .list(&session.id)
        .await
        .unwrap();
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].status, "applied");

    let transcript = service.transcript(&session.id).await.unwrap();
    assert_eq!(transcript.len(), 1);
    let kinds: Vec<&str> = transcript[0]
        .items
        .iter()
        .map(|item| item.kind.as_str())
        .collect();
    assert_eq!(kinds.iter().filter(|kind| **kind == "mcp_call").count(), 3);
    assert_eq!(
        kinds.iter().filter(|kind| **kind == "file_change").count(),
        1
    );
    assert!(kinds.contains(&"usage"));
    assert!(transcript[0].items.iter().all(|item| item.raw.is_some()));

    let stored = service.get(&session.id).await.unwrap();
    assert_eq!(stored.spend_micros, 500);
    assert_eq!(stored.tasks_done, 1, "a completed turn with no error item");
    assert_eq!(stored.interventions, 0);
    assert_eq!(service.agents(&session.id).await.unwrap()[0].status, "idle");

    // The checkpoint matches the tree the turn ended on.
    let checkpoint = service
        .latest_checkpoint(&session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        checkpoint.tree_hash,
        rigel_core::tree::tree_hash(tmp.path()).unwrap()
    );
}

#[tokio::test]
async fn the_cap_stops_the_turn_and_refuses_the_next_one() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("budget"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let expensive = r#"{"steps":[{"text":"$0.31 against a $0.25 cap\n","usage":{"input_tokens":22000,"output_tokens":1200,"cost_usd":0.31}},{"text":"never reached"}]}"#;
    let run = coordinator(tmp.path(), &service, expensive, StopAt::Never, 10)
        .run(&session.id, &agent.id, "spend")
        .await
        .unwrap();
    assert_eq!(run.stop, StopReason::BudgetExceeded);
    assert_eq!(run.steps, 1, "the second step never ran");
    assert_eq!(run.usage.cost_micros, 310_000);

    let stored = service.get(&session.id).await.unwrap();
    assert_eq!(stored.spend_micros, 310_000);
    assert_eq!(stored.spend_cents, 31);

    let error = coordinator(tmp.path(), &service, SCRIPT, StopAt::Never, 10)
        .run(&session.id, &agent.id, "again")
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        rigel_core::AgentError::BudgetExhausted { .. }
    ));
    assert_eq!(
        service.transcript(&session.id).await.unwrap().len(),
        1,
        "a refused turn creates no turn row"
    );

    let budget = BudgetService::with_null_interrupt(service.pool().clone());
    assert!(budget.reconcile(&session.id).await.unwrap().ok);
    let usage: Usage = service.turn(&run.turn.id).await.unwrap().usage.unwrap();
    assert_eq!(usage.cost_micros, 310_000);
}

/// A parent that splits, collects and finishes, and children that answer by prompt. A single
/// scripted model would race its own step queue now that children run in the background.
#[derive(Default)]
struct SubagentScriptModel {
    parent_calls: AtomicUsize,
}

#[async_trait]
impl Model for SubagentScriptModel {
    fn id(&self) -> String {
        "subagent-script".to_string()
    }

    async fn chat(
        &self,
        request: ChatRequest,
        _cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError> {
        let system: String = request
            .messages
            .iter()
            .filter(|message| message.role == "system")
            .filter_map(|message| message.content.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .and_then(|message| message.content.clone())
            .unwrap_or_default();

        if system.contains("You are a read-only subagent") {
            let text = if last_user.contains("README") {
                "README says Demo.\n"
            } else {
                "lib.rs adds and subtracts.\n"
            };
            on_delta(Delta::Text(text.to_string()));
            on_delta(Delta::Usage(RawUsage {
                input_tokens: 50,
                cached_input_tokens: 0,
                output_tokens: 10,
                reasoning_tokens: 0,
                cost_usd: Some(0.0003),
            }));
            on_delta(Delta::Finish("stop".to_string()));
            return Ok(());
        }

        let call = self.parent_calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            on_delta(Delta::Text("Spawning two readers.\n".to_string()));
            on_delta(Delta::ToolCall {
                index: 0,
                id: Some("call_0".to_string()),
                name: Some("spawn_agents".to_string()),
                arguments: serde_json::json!({
                    "agents": [
                        { "role": "looker", "files": ["README.md"], "prompt": "Read README.md and summarise it." },
                        { "role": "looker-2", "files": ["src/lib.rs"], "prompt": "Read src/lib.rs and summarise it.", "model": "openai/gpt-5.4-nano" }
                    ]
                })
                .to_string(),
            });
            on_delta(Delta::Usage(RawUsage {
                input_tokens: 100,
                cached_input_tokens: 0,
                output_tokens: 20,
                reasoning_tokens: 0,
                cost_usd: Some(0.0004),
            }));
        } else if call == 1 {
            on_delta(Delta::ToolCall {
                index: 0,
                id: Some("call_1".to_string()),
                name: Some("subagent_results".to_string()),
                arguments: serde_json::json!({ "timeout_secs": 10 }).to_string(),
            });
        } else {
            on_delta(Delta::Text(
                "Two subagents read the files; no tests were run.\n".to_string(),
            ));
            on_delta(Delta::Usage(RawUsage {
                input_tokens: 80,
                cached_input_tokens: 0,
                output_tokens: 20,
                reasoning_tokens: 0,
                cost_usd: Some(0.0001),
            }));
        }
        on_delta(Delta::Finish(if call == 1 {
            "tool_calls".to_string()
        } else {
            "stop".to_string()
        }));
        Ok(())
    }
}

#[tokio::test]
async fn subagents_are_read_only_billed_separately_and_nested_under_the_parent() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("subagents"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let cancels = Arc::new(Cancels::new());
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels.clone() as Arc<dyn rigel_core::InterruptSink>,
    ));
    let tools = Tools::new(
        tmp.path().to_path_buf(),
        service.clone(),
        budget.clone(),
        tmp.path().join("index"),
        StopAt::Never,
    );
    let run = Coordinator {
        session: service.clone(),
        budget,
        tools,
        model: Arc::new(SubagentScriptModel::default()),
        sink: Arc::new(rigel_core::NullSink),
        cancels,
        project_root: tmp.path().to_path_buf(),
        model_name: "subagent-script".to_string(),
        effort: None,
        max_steps: 10,
        subagents: rigel_core::agent::SubagentPolicy::default(),
        depth: 0,
        worktree_dir: None,
        chat_parent: None,
        parent_cancel: None,
    }
    .run(&session.id, &agent.id, "read the repo")
    .await
    .unwrap();

    assert_eq!(run.stop, StopReason::Completed);
    assert_eq!(
        run.steps, 3,
        "spawn, collect and final are three parent steps"
    );
    assert_eq!(run.tool_calls, 2, "spawn and results are two tool calls");
    assert_eq!(
        run.usage.cost_micros, 500,
        "the parent pays only for its own steps"
    );

    let agents = service.agents(&session.id).await.unwrap();
    assert_eq!(agents.len(), 3, "the parent plus two children");
    let looker = agents
        .iter()
        .find(|candidate| candidate.role.as_deref() == Some("looker"))
        .expect("the first child was registered");
    assert_eq!(looker.model.as_deref(), Some("openai/gpt-5-nano"));
    assert_eq!(looker.spend_micros, 300);

    let turns = service.transcript(&session.id).await.unwrap();
    assert_eq!(turns.len(), 3);
    let parent = turns
        .iter()
        .find(|entry| entry.turn.id == run.turn.id)
        .unwrap();
    assert_eq!(parent.turn.parent_turn_id, None);
    let children: Vec<_> = turns
        .iter()
        .filter(|entry| entry.turn.parent_turn_id.as_deref() == Some(run.turn.id.as_str()))
        .collect();
    assert_eq!(children.len(), 2);
    assert!(
        children
            .iter()
            .all(|entry| entry.turn.status == "completed")
    );

    // The parent's context gets text, not the children's transcripts. Which child got which
    // scripted answer depends on scheduling, so the set is asserted, not the pairing; the
    // result array itself stays in the order the parent asked for.
    let spawn = parent
        .items
        .iter()
        .find(|item| item.body.get("tool").and_then(|tool| tool.as_str()) == Some("spawn_agents"))
        .expect("the spawn call is in the parent transcript");
    assert_eq!(
        spawn.body["result"]["started"].as_array().unwrap().len(),
        2,
        "spawn returns the children it started, not their answers"
    );
    assert_eq!(spawn.body["result"]["pending"], 2);
    let results = parent
        .items
        .iter()
        .find(|item| {
            item.body.get("tool").and_then(|tool| tool.as_str()) == Some("subagent_results")
        })
        .expect("the collect call is in the parent transcript");
    let returned = results.body["result"]["agents"].as_array().unwrap();
    assert_eq!(returned[0]["role"], "looker");
    assert_eq!(returned[1]["role"], "looker-2");
    let mut texts: Vec<&str> = returned
        .iter()
        .map(|agent| agent["text"].as_str().unwrap())
        .collect();
    texts.sort();
    assert_eq!(
        texts,
        ["README says Demo.\n", "lib.rs adds and subtracts.\n"]
    );
    assert_eq!(results.body["result"]["spent_micros"], 600);
    assert_eq!(results.body["result"]["pending"], false);

    let board = service.team(&session.id).await.unwrap();
    assert_eq!(board.len(), 2, "both children are on the work board");
    let first = board
        .iter()
        .find(|row| row.role.as_deref() == Some("looker"))
        .expect("the first assignment is on the board");
    assert_eq!(first.kind, "native");
    assert_eq!(first.files, vec!["README.md".to_string()]);
    assert_eq!(first.status, "done");
    let second = board
        .iter()
        .find(|row| row.role.as_deref() == Some("looker-2"))
        .expect("the second assignment is on the board");
    assert_eq!(second.files, vec!["src/lib.rs".to_string()]);
    assert_eq!(second.status, "done");

    let stored = service.get(&session.id).await.unwrap();
    assert_eq!(stored.spend_micros, 1_100, "parent 500 + children 600");
    assert_eq!(stored.tasks_done, 1, "a child turn is not a task");

    let budget = BudgetService::with_null_interrupt(service.pool().clone());
    let reconcile = budget.reconcile(&session.id).await.unwrap();
    assert!(reconcile.ok, "every child is in its own agent's sum");
}

#[tokio::test]
async fn a_parent_cannot_point_a_subagent_at_an_expensive_model() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("allowlist"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let script = r#"{
      "steps": [
        {
          "text": "Spawning a reader.\n",
          "tool_calls": [{
            "name": "spawn_agents",
            "arguments": { "agents": [
              { "role": "expensive", "files": ["README.md"], "prompt": "Read README.md.", "model": "openai/gpt-5.4-pro" }
            ] }
          }],
          "usage": { "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0004 }
        },
        { "text": "I could not delegate to that model.\n", "usage": { "input_tokens": 50, "output_tokens": 10, "cost_usd": 0.0002 } }
      ]
    }"#;
    let run = coordinator(tmp.path(), &service, script, StopAt::Never, 10)
        .run(&session.id, &agent.id, "read the repo")
        .await
        .unwrap();

    assert_eq!(run.stop, StopReason::Completed);
    assert_eq!(run.usage.cost_micros, 600, "no child was billed");
    assert_eq!(
        service.agents(&session.id).await.unwrap().len(),
        1,
        "a refused spawn registers no agent"
    );

    let entry = service
        .transcript(&session.id)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.turn.id == run.turn.id)
        .unwrap();
    let spawn = entry
        .items
        .iter()
        .find(|item| item.body.get("tool").and_then(|tool| tool.as_str()) == Some("spawn_agents"))
        .expect("the refused call is in the transcript");
    assert_eq!(spawn.body["result"]["ok"], false);
    assert_eq!(spawn.body["result"]["error"]["code"], "MODEL_NOT_ALLOWED");
}

#[tokio::test]
async fn a_subagent_cannot_write_even_if_it_asks() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("read-only"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let budget = Arc::new(BudgetService::with_null_interrupt(service.pool().clone()));
    let tools = Tools::new(
        tmp.path().to_path_buf(),
        service.clone(),
        budget,
        tmp.path().join("index"),
        StopAt::Never,
    );
    let child = tools.subagent();

    let names: Vec<String> = child.specs().into_iter().map(|spec| spec.name).collect();
    assert!(!names.contains(&"edit_file".to_string()));
    assert!(!names.contains(&"write_file".to_string()));
    assert!(!names.contains(&"spawn_agents".to_string()));
    assert!(!names.contains(&"subagent_results".to_string()));
    assert!(!names.contains(&"assign_agents".to_string()));
    assert!(
        Tools::specs_all()
            .iter()
            .any(|spec| spec.name == "spawn_agents"),
        "a top-level agent is offered the tool"
    );
    assert!(
        Tools::specs_all()
            .iter()
            .any(|spec| spec.name == "assign_agents"),
        "a top-level agent can hand work to the chat room"
    );

    let outcome = child
        .execute(
            &session.id,
            &agent.id,
            "edit_file",
            serde_json::json!({ "path": "README.md", "old_string": "Demo", "new_string": "Nope" }),
        )
        .await;
    assert!(!outcome.ok);
    assert_eq!(outcome.body["error"]["code"], "READ_ONLY");
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("README.md")).unwrap(),
        "# Demo\nThree files.\n",
        "the refusal happened before the pipeline"
    );
}

#[tokio::test]
async fn a_scoped_subagent_reads_only_its_assigned_files() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("scope"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let budget = Arc::new(BudgetService::with_null_interrupt(service.pool().clone()));
    let tools = Tools::new(
        tmp.path().to_path_buf(),
        service.clone(),
        budget,
        tmp.path().join("index"),
        StopAt::Never,
    );
    let child = tools.subagent().with_scope(vec!["README.md".to_string()]);

    let allowed = child
        .execute(
            &session.id,
            &agent.id,
            "read_file",
            serde_json::json!({ "path": "README.md" }),
        )
        .await;
    assert!(
        allowed.ok,
        "the assigned file is readable: {:?}",
        allowed.body
    );

    let denied = child
        .execute(
            &session.id,
            &agent.id,
            "read_file",
            serde_json::json!({ "path": "src/lib.rs" }),
        )
        .await;
    assert!(!denied.ok, "an unassigned file must not be read");
    assert_eq!(denied.body["error"]["code"], "OUT_OF_SCOPE");
}

#[tokio::test]
async fn the_coordinator_cannot_give_one_file_to_two_helpers() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("duplicate"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let script = r#"{
      "steps": [
        {
          "text": "Spawning two readers on the same file.\n",
          "tool_calls": [{
            "name": "spawn_agents",
            "arguments": { "agents": [
              { "role": "one", "files": ["README.md"], "prompt": "Read README.md." },
              { "role": "two", "files": ["README.md"], "prompt": "Read README.md again." }
            ] }
          }],
          "usage": { "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0004 }
        },
        { "text": "I will split the files differently.\n", "usage": { "input_tokens": 50, "output_tokens": 10, "cost_usd": 0.0002 } }
      ]
    }"#;
    let run = coordinator(tmp.path(), &service, script, StopAt::Never, 10)
        .run(&session.id, &agent.id, "read the repo")
        .await
        .unwrap();

    assert_eq!(run.stop, StopReason::Completed);
    assert_eq!(
        service.agents(&session.id).await.unwrap().len(),
        1,
        "a refused split registers no child"
    );
    let entry = service
        .transcript(&session.id)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.turn.id == run.turn.id)
        .unwrap();
    let spawn = entry
        .items
        .iter()
        .find(|item| item.body.get("tool").and_then(|tool| tool.as_str()) == Some("spawn_agents"))
        .expect("the refused call is in the transcript");
    assert_eq!(spawn.body["result"]["error"]["code"], "DUPLICATE_FILE");
}

#[tokio::test]
async fn a_quote_reads_the_bill_without_creating_a_turn() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("quote"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let budget = BudgetService::with_null_interrupt(service.pool().clone());
    let quote = budget
        .quote(
            &agent.id,
            "anthropic/claude-sonnet-4.5",
            1_600,
            "fix the flaky test",
        )
        .await
        .unwrap();

    assert_eq!(quote.spend_micros, 0);
    assert_eq!(quote.cap_micros, Some(250_000));
    assert_eq!(quote.remaining_micros, Some(250_000));
    assert_eq!(quote.reserve_micros, 10_000);
    assert!(quote.input_tokens >= 400, "the fixed context is priced too");
    assert!(quote.first_step_micros > 0);
    assert!(!quote.over_remaining);

    // The composer quotes before an agent exists, so the session-scoped path must agree.
    let same = budget
        .quote_session(
            &session.id,
            "anthropic/claude-sonnet-4.5",
            1_600,
            "fix the flaky test",
        )
        .await
        .unwrap();
    assert_eq!(same.first_step_micros, quote.first_step_micros);
    assert_eq!(same.remaining_micros, quote.remaining_micros);

    let stored = service.get(&session.id).await.unwrap();
    assert_eq!(stored.spend_micros, 0, "a quote is not a charge");
    assert!(
        service.transcript(&session.id).await.unwrap().is_empty(),
        "a quote creates no turn row"
    );
}

#[tokio::test]
async fn a_kill_after_the_journal_leaves_a_pending_patch_and_a_running_turn() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("crash"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let run = coordinator(tmp.path(), &service, SCRIPT, StopAt::AfterJournal, 10)
        .run(&session.id, &agent.id, "crash")
        .await
        .unwrap();
    assert_eq!(run.stop, StopReason::CrashSimulated);
    assert_eq!(
        service.turn(&run.turn.id).await.unwrap().status,
        "running",
        "a kill leaves the turn open for resume to close"
    );
    assert!(
        service
            .latest_checkpoint(&session.id)
            .await
            .unwrap()
            .is_none(),
        "a killed turn records no checkpoint"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("src/lib.rs")).unwrap(),
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn sub(a: i32, b: i32) -> i32 {\n    a - b\n}\n"
    );

    let outcome = rigel_core::resume::resume(&service, tmp.path(), &session.id, false)
        .await
        .unwrap();
    assert!(!outcome.clean);
    assert_eq!(outcome.closed_turns.len(), 1);
    assert_eq!(outcome.closed_turns[0].status, "cancelled");
    assert_eq!(outcome.patches.len(), 1);
    assert_eq!(outcome.patches[0].state, rigel_core::RecoveryState::Pending);
    assert_eq!(
        service.get(&session.id).await.unwrap().status,
        rigel_core::SessionStatus::Interrupted
    );

    let pipeline = rigel_core::PatchPipeline::new(tmp.path(), service.pool().clone());
    let patches = pipeline.list(&session.id).await.unwrap();
    assert_eq!(patches.len(), 1);
    let applied = pipeline.apply(&patches[0].id).await.unwrap();
    assert_eq!(applied.status, "applied");
    assert!(
        std::fs::read_to_string(tmp.path().join("src/lib.rs"))
            .unwrap()
            .contains("a.saturating_sub(b)")
    );

    let forced = rigel_core::resume::resume(&service, tmp.path(), &session.id, true)
        .await
        .unwrap();
    assert!(
        !forced.clean,
        "the tree changed after the would-be checkpoint"
    );
    assert_eq!(forced.session.interventions, 1);
    let clean = rigel_core::resume::resume(&service, tmp.path(), &session.id, false)
        .await
        .unwrap();
    assert!(clean.clean, "the forced resume wrote a new baseline");
}

/// Counts how many child model calls are in flight at once. The parent call is answered
/// immediately; a child call sleeps long enough that a sequential implementation could only
/// ever show a peak of one.
#[derive(Default)]
struct OverlapModel {
    active: AtomicUsize,
    peak: AtomicUsize,
    parent_calls: AtomicUsize,
    /// How many child calls were inside the model when the parent made its second step.
    parent_saw_active: AtomicUsize,
    /// When set, a child call stops the parent agent — the cancellation-propagation test.
    canceller: Option<(Arc<Cancels>, String)>,
}

#[async_trait]
impl Model for OverlapModel {
    fn id(&self) -> String {
        "overlap".to_string()
    }

    async fn chat(
        &self,
        request: ChatRequest,
        _cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError> {
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .and_then(|message| message.content.clone())
            .unwrap_or_default();

        if last_user == "work" {
            let call = self.parent_calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                on_delta(Delta::Text("Spawning two readers.\n".to_string()));
                let arguments = serde_json::json!({
                    "agents": [
                        { "role": "a", "files": ["README.md"], "prompt": "child-a" },
                        { "role": "b", "files": ["src/lib.rs"], "prompt": "child-b" }
                    ]
                })
                .to_string();
                on_delta(Delta::ToolCall {
                    index: 0,
                    id: Some("call_0".to_string()),
                    name: Some("spawn_agents".to_string()),
                    arguments,
                });
            } else if call == 1 {
                // The point of the background fan-out: this parent step is running while the
                // children are still inside their model calls, so it can talk to them.
                for _ in 0..20 {
                    if self.active.load(Ordering::SeqCst) > 0 {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                self.parent_saw_active
                    .store(self.active.load(Ordering::SeqCst), Ordering::SeqCst);
                on_delta(Delta::Text("Collecting the readers.\n".to_string()));
                on_delta(Delta::ToolCall {
                    index: 0,
                    id: Some("call_1".to_string()),
                    name: Some("subagent_results".to_string()),
                    arguments: serde_json::json!({ "timeout_secs": 10 }).to_string(),
                });
            } else {
                on_delta(Delta::Text("Both readers answered.\n".to_string()));
            }
            on_delta(Delta::Finish("stop".to_string()));
            return Ok(());
        }

        if let Some((cancels, agent_id)) = &self.canceller {
            cancels.cancel_agent(agent_id);
        }
        let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(250)).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        on_delta(Delta::Text(format!("{last_user} done.\n")));
        on_delta(Delta::Finish("stop".to_string()));
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sibling_subagents_overlap_their_model_calls() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("parallel"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let model = Arc::new(OverlapModel::default());
    let cancels = Arc::new(Cancels::new());
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels.clone() as Arc<dyn rigel_core::InterruptSink>,
    ));
    let tools = Tools::new(
        tmp.path().to_path_buf(),
        service.clone(),
        budget.clone(),
        tmp.path().join("index"),
        StopAt::Never,
    );
    let run = Coordinator {
        session: service.clone(),
        budget,
        tools,
        model: model.clone(),
        sink: Arc::new(rigel_core::NullSink),
        cancels,
        project_root: tmp.path().to_path_buf(),
        model_name: "overlap".to_string(),
        effort: None,
        max_steps: 10,
        subagents: rigel_core::agent::SubagentPolicy::default(),
        depth: 0,
        worktree_dir: None,
        chat_parent: None,
        parent_cancel: None,
    }
    .run(&session.id, &agent.id, "work")
    .await
    .unwrap();

    assert_eq!(run.stop, StopReason::Completed);
    assert_eq!(
        model.peak.load(Ordering::SeqCst),
        2,
        "both children were inside the model at the same time"
    );
    assert!(
        model.parent_saw_active.load(Ordering::SeqCst) >= 1,
        "the parent took a step while the children were still working, so the room can be used"
    );
    assert_eq!(model.active.load(Ordering::SeqCst), 0);

    let entry = service
        .transcript(&session.id)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.turn.id == run.turn.id)
        .unwrap();
    let results = entry
        .items
        .iter()
        .find(|item| {
            item.body.get("tool").and_then(|tool| tool.as_str()) == Some("subagent_results")
        })
        .expect("the result-collection call is in the parent transcript");
    let agents = results.body["result"]["agents"].as_array().unwrap();
    assert_eq!(
        agents[0]["text"], "child-a done.\n",
        "asked order, not finish order: {agents:?}"
    );
    assert_eq!(agents[1]["text"], "child-b done.\n", "result: {agents:?}");
    assert!(agents.iter().all(|agent| agent["ok"] == true));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_scripted_subagent_fixture_uses_the_child_queue_and_results_tool() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("fixture"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/fixtures/p4-subagents.json");
    let script = std::fs::read_to_string(fixture).unwrap();

    let run = coordinator(tmp.path(), &service, &script, StopAt::Never, 10)
        .run(&session.id, &agent.id, "summarise the repo")
        .await
        .unwrap();

    assert_eq!(run.stop, StopReason::Completed);
    assert_eq!(run.steps, 3, "spawn, collect, answer");
    assert_eq!(run.usage.cost_micros, 1_000);
    let turns = service.transcript(&session.id).await.unwrap();
    assert_eq!(turns.len(), 3, "parent plus two children");
    let children: Vec<_> = turns
        .iter()
        .filter(|entry| entry.turn.parent_turn_id.as_deref() == Some(run.turn.id.as_str()))
        .collect();
    assert_eq!(children.len(), 2);
    assert!(
        children
            .iter()
            .all(|entry| entry.turn.status == "completed")
    );
}

#[derive(Default)]
struct RecordingModel {
    parent_calls: AtomicUsize,
    systems: Mutex<Vec<String>>,
}

#[async_trait]
impl Model for RecordingModel {
    fn id(&self) -> String {
        "recording".to_string()
    }

    async fn chat(
        &self,
        request: ChatRequest,
        _cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError> {
        let system: String = request
            .messages
            .iter()
            .filter(|message| message.role == "system")
            .filter_map(|message| message.content.clone())
            .collect::<Vec<_>>()
            .join(
                "
",
            );
        self.systems.lock().unwrap().push(system.clone());

        if system.contains("You are Rigel's native agent") {
            let call = self.parent_calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                on_delta(Delta::Text(
                    "Spawning two readers.
"
                    .to_string(),
                ));
                let arguments = serde_json::json!({
                    "agents": [
                        { "role": "a", "files": ["README.md"], "prompt": "child-a", "task": "read the readme" },
                        { "role": "b", "files": ["src/lib.rs"], "prompt": "child-b", "task": "read the library" }
                    ]
                })
                .to_string();
                on_delta(Delta::ToolCall {
                    index: 0,
                    id: Some("call_0".to_string()),
                    name: Some("spawn_agents".to_string()),
                    arguments,
                });
            } else if call == 1 {
                on_delta(Delta::Text("Collecting the readers.\n".to_string()));
                on_delta(Delta::ToolCall {
                    index: 0,
                    id: Some("call_1".to_string()),
                    name: Some("subagent_results".to_string()),
                    arguments: serde_json::json!({ "timeout_secs": 10 }).to_string(),
                });
            } else {
                on_delta(Delta::Text(
                    "Both readers answered.
"
                    .to_string(),
                ));
            }
        } else {
            on_delta(Delta::Text(
                "Working on the assigned file.
"
                .to_string(),
            ));
        }
        on_delta(Delta::Finish("stop".to_string()));
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn children_see_their_files_and_the_shared_board() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("board"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let model = Arc::new(RecordingModel::default());
    let cancels = Arc::new(Cancels::new());
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels.clone() as Arc<dyn rigel_core::InterruptSink>,
    ));
    let tools = Tools::new(
        tmp.path().to_path_buf(),
        service.clone(),
        budget.clone(),
        tmp.path().join("index"),
        StopAt::Never,
    );
    let run = Coordinator {
        session: service.clone(),
        budget,
        tools,
        model: model.clone(),
        sink: Arc::new(rigel_core::NullSink),
        cancels,
        project_root: tmp.path().to_path_buf(),
        model_name: "recording".to_string(),
        effort: None,
        max_steps: 10,
        subagents: rigel_core::agent::SubagentPolicy::default(),
        depth: 0,
        worktree_dir: None,
        chat_parent: None,
        parent_cancel: None,
    }
    .run(&session.id, &agent.id, "work")
    .await
    .unwrap();

    assert_eq!(run.stop, StopReason::Completed);
    let systems = model.systems.lock().unwrap();
    let parent = systems
        .iter()
        .find(|system| system.contains("You are Rigel's native agent"))
        .expect("the coordinator was prompted");
    assert!(
        parent.contains("Session chat: you are @worker-"),
        "the coordinator is told its own room name: {parent}"
    );
    assert!(
        parent.contains("chat_read")
            && parent.contains("chat_send")
            && parent.contains("chat_wait"),
        "the coordinator is given the chat tools in its prompt"
    );
    let children: Vec<&String> = systems
        .iter()
        .filter(|system| system.contains("You are a read-only subagent"))
        .collect();
    assert_eq!(children.len(), 2, "both children were prompted");
    for child in &children {
        assert!(
            child.contains("Your assignment from the coordinator:"),
            "a child is told what it owns: {child}"
        );
        assert!(
            child.contains("Your parent is @worker-"),
            "a child is told who to answer: {child}"
        );
        assert!(
            child.contains("Teammates:") && child.contains("chat_wait"),
            "a child is told how to talk to its peers: {child}"
        );
        assert!(child.contains("Working on:"), "the answer format is stated");
        assert!(
            child.contains("README.md") && child.contains("src/lib.rs"),
            "a child sees its own files and its sibling's: {child}"
        );
    }
    assert!(
        children.iter().any(|child| child.contains("[a]")),
        "the board labels each helper by its role"
    );
    assert!(
        systems
            .iter()
            .any(|system| system.contains("Work board for this session")),
        "the coordinator restates the board after the split"
    );
}

/// A decision the user made in the window (D115's Diff-view merge) is posted on the previous
/// turn. Every turn starts from a fresh message list, so restating it is the only way the note
/// reaches the model at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_note_the_user_left_on_the_last_turn_reaches_the_next_prompt() {
    const NOTE: &str =
        "the user merged writer 0b7e3021's work from the Diff view: 1 file landed in the main tree";
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("notes"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let model = Arc::new(RecordingModel::default());
    let cancels = Arc::new(Cancels::new());
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels.clone() as Arc<dyn rigel_core::InterruptSink>,
    ));
    let tools = Tools::new(
        tmp.path().to_path_buf(),
        service.clone(),
        budget.clone(),
        tmp.path().join("index"),
        StopAt::Never,
    );
    let coordinator = Coordinator {
        session: service.clone(),
        budget,
        tools,
        model: model.clone(),
        sink: Arc::new(rigel_core::NullSink),
        cancels,
        project_root: tmp.path().to_path_buf(),
        model_name: "recording".to_string(),
        effort: None,
        max_steps: 10,
        subagents: rigel_core::agent::SubagentPolicy::default(),
        depth: 0,
        worktree_dir: None,
        chat_parent: None,
        parent_cancel: None,
    };
    let first = coordinator
        .run(&session.id, &agent.id, "split the work")
        .await
        .unwrap();

    // The user merges the child's work in the Diff view: the note lands on the turn that ran.
    let body = serde_json::json!({ "text": NOTE, "kind": "user_merge" });
    service
        .append_item(&first.turn.id, "note", "ok", &body, &body)
        .await
        .unwrap();

    let second = coordinator
        .run(&session.id, &agent.id, "carry on")
        .await
        .unwrap();
    assert_eq!(second.stop, StopReason::Completed);

    let notes = service
        .previous_notes(&agent.id, &second.turn.id)
        .await
        .unwrap();
    assert_eq!(notes.len(), 1, "the previous turn holds exactly one note");

    let systems = model.systems.lock().unwrap();
    let parents: Vec<&String> = systems
        .iter()
        .filter(|system| system.contains("You are Rigel's native agent"))
        .collect();
    assert_eq!(
        parents
            .iter()
            .filter(|system| system.contains(NOTE))
            .count(),
        1,
        "the note is stated once, in the turn that follows it: {parents:?}"
    );
    let last = parents.last().expect("the second turn was prompted");
    assert!(
        last.contains("The user acted in the window") && last.contains(NOTE),
        "the note reaches the next prompt: {last}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_a_parent_stops_the_children_it_spawned() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("cancel"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let cancels = Arc::new(Cancels::new());
    let model = Arc::new(OverlapModel {
        canceller: Some((cancels.clone(), agent.id.clone())),
        ..OverlapModel::default()
    });
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels.clone() as Arc<dyn rigel_core::InterruptSink>,
    ));
    let tools = Tools::new(
        tmp.path().to_path_buf(),
        service.clone(),
        budget.clone(),
        tmp.path().join("index"),
        StopAt::Never,
    );
    let run = Coordinator {
        session: service.clone(),
        budget,
        tools,
        model,
        sink: Arc::new(rigel_core::NullSink),
        cancels,
        project_root: tmp.path().to_path_buf(),
        model_name: "overlap".to_string(),
        effort: None,
        max_steps: 10,
        subagents: rigel_core::agent::SubagentPolicy::default(),
        depth: 0,
        worktree_dir: None,
        chat_parent: None,
        parent_cancel: None,
    }
    .run(&session.id, &agent.id, "work")
    .await
    .unwrap();

    assert_eq!(run.stop, StopReason::Cancelled);
    let children: Vec<_> = service
        .transcript(&session.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|entry| entry.turn.parent_turn_id.as_deref() == Some(run.turn.id.as_str()))
        .collect();
    assert_eq!(children.len(), 2);
    let statuses: Vec<&str> = children
        .iter()
        .map(|entry| entry.turn.status.as_str())
        .collect();
    assert!(
        statuses.iter().all(|status| *status == "cancelled"),
        "a stopped parent leaves no child running: {statuses:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_usage_is_counted_once_per_charge() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("accounting"), Some(25)).await.unwrap();
    let budget = Arc::new(BudgetService::with_null_interrupt(service.pool().clone()));

    let mut tasks = Vec::new();
    for index in 0..4 {
        let agent = service
            .register_agent(&session.id, "native", Some(&format!("agent-{index}")))
            .await
            .unwrap();
        let turn = service
            .append_turn_with_parent(&agent.id, Some("work"), None)
            .await
            .unwrap();
        let budget = budget.clone();
        tasks.push(tokio::spawn(async move {
            for _ in 0..5 {
                let usage = Usage {
                    cost_micros: 100,
                    ..Usage::default()
                };
                budget
                    .account(&agent.id, &turn.id, &usage)
                    .await
                    .unwrap_or_else(|error| panic!("account failed: {error:?}"));
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }

    let stored = service.get(&session.id).await.unwrap();
    assert_eq!(stored.spend_micros, 2_000, "20 charges of 100µ$");
    for agent in service.agents(&session.id).await.unwrap() {
        assert_eq!(agent.spend_micros, 500);
    }
    assert!(budget.reconcile(&session.id).await.unwrap().ok);
}

/// Records the effort on every request and answers in one step.
struct EffortRecorder {
    seen: Mutex<Vec<Option<String>>>,
}

#[async_trait]
impl Model for EffortRecorder {
    fn id(&self) -> String {
        "effort-recorder".to_string()
    }

    async fn chat(
        &self,
        request: ChatRequest,
        _cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError> {
        self.seen.lock().unwrap().push(request.effort.clone());
        on_delta(Delta::Text(
            "done.
"
            .to_string(),
        ));
        on_delta(Delta::Finish("stop".to_string()));
        Ok(())
    }
}

#[tokio::test]
async fn the_coordinators_effort_reaches_every_step_request() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("native"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let cancels = Arc::new(Cancels::new());
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels.clone() as Arc<dyn rigel_core::InterruptSink>,
    ));
    let tools = Tools::new(
        tmp.path().to_path_buf(),
        service.clone(),
        budget.clone(),
        tmp.path().join("index"),
        StopAt::Never,
    );
    let model = Arc::new(EffortRecorder {
        seen: Mutex::new(Vec::new()),
    });
    let run = Coordinator {
        session: service.clone(),
        budget,
        tools,
        model: model.clone(),
        sink: Arc::new(rigel_core::NullSink),
        cancels,
        project_root: tmp.path().to_path_buf(),
        model_name: "effort-recorder".to_string(),
        effort: Some("low".to_string()),
        max_steps: 3,
        subagents: rigel_core::agent::SubagentPolicy::default(),
        depth: 0,
        worktree_dir: None,
        chat_parent: None,
        parent_cancel: None,
    }
    .run(&session.id, &agent.id, "say done")
    .await
    .unwrap();

    assert_eq!(run.stop, StopReason::Completed);
    let seen = model.seen.lock().unwrap().clone();
    assert!(
        !seen.is_empty(),
        "the model must have been asked at least once"
    );
    assert!(
        seen.iter().all(|effort| effort.as_deref() == Some("low")),
        "every step request carries the coordinator's effort: {seen:?}"
    );
}

#[tokio::test]
async fn plan_mode_refuses_the_edit_until_the_plan_is_approved() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("plan"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("implementer"))
        .await
        .unwrap();
    let script = r#"{
      "steps": [
        {"tool_calls": [{"name": "read_file", "arguments": {"path": "src/lib.rs"}}],
         "usage": {"input_tokens": 10, "output_tokens": 2, "cost_usd": 0.0001}},
        {"tool_calls": [{"name": "edit_file", "arguments": {
            "path": "src/lib.rs",
            "old_string": "    a - b",
            "new_string": "    a.saturating_sub(b)"
        }}],
         "usage": {"input_tokens": 10, "output_tokens": 2, "cost_usd": 0.0001}},
        {"text": "Plan: make subtraction saturating.", "usage": {"input_tokens": 5, "output_tokens": 2, "cost_usd": 0.0001}}
      ]
    }"#;

    rigel_core::plan::set(service.pool(), &session.id, true)
        .await
        .unwrap();
    let planned = coordinator(tmp.path(), &service, script, StopAt::Never, 10)
        .run(&session.id, &agent.id, "plan the fix")
        .await
        .unwrap();
    assert_eq!(planned.stop, StopReason::Completed);
    assert!(
        std::fs::read_to_string(tmp.path().join("src/lib.rs"))
            .unwrap()
            .contains("    a - b"),
        "plan mode must not change the tree"
    );
    let transcript = service.transcript(&session.id).await.unwrap();
    let body = serde_json::to_string(&transcript[0].items).unwrap();
    assert!(
        body.contains("PLAN_MODE"),
        "the refusal is in the transcript"
    );

    rigel_core::plan::set(service.pool(), &session.id, false)
        .await
        .unwrap();
    let worked = coordinator(tmp.path(), &service, script, StopAt::Never, 10)
        .run(&session.id, &agent.id, "do it")
        .await
        .unwrap();
    assert_eq!(worked.stop, StopReason::Completed);
    assert!(
        std::fs::read_to_string(tmp.path().join("src/lib.rs"))
            .unwrap()
            .contains("a.saturating_sub(b)"),
        "approval lets the same edit through"
    );
}

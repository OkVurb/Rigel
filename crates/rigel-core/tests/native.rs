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
use rigel_core::agent::dag::NodeState;
use rigel_core::agent::{
    Cancels, ChatRequest, Coordinator, Delta, DeltaSink, Model, ModelError, RawUsage,
    ScriptedModel, StopReason, Tools,
};
use rigel_core::{BudgetService, SessionService, StopAt, Usage};
use tokio::sync::Notify;
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

#[derive(Default)]
struct CapturingModel {
    seen: Mutex<Vec<ChatRequest>>,
}

#[async_trait]
impl Model for CapturingModel {
    fn id(&self) -> String {
        "script".to_string()
    }

    async fn chat(
        &self,
        request: ChatRequest,
        _cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError> {
        self.seen.lock().unwrap().push(request);
        on_delta(Delta::Text("done\n".to_string()));
        on_delta(Delta::Finish("stop".to_string()));
        Ok(())
    }
}

fn coordinator_with_model(
    root: &std::path::Path,
    service: &SessionService,
    model: Arc<dyn Model>,
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
        model,
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
async fn two_hundred_turns_compact_and_carry_pins_and_the_summary_forward() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("compaction"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    service
        .pin_skill(&session.id, "pdf-tools", "sha-pinned", false)
        .await
        .unwrap();

    for index in 0..200 {
        let turn = service
            .append_turn(&agent.id, Some(&format!("request {index}")))
            .await
            .unwrap();
        service
            .append_item(
                &turn.id,
                "message",
                "complete",
                &serde_json::json!({
                    "role": "assistant",
                    "text": format!("answer {index} {}", "x".repeat(4_000)),
                }),
                &serde_json::json!({}),
            )
            .await
            .unwrap();
        service
            .append_item(
                &turn.id,
                "mcp_call",
                "ok",
                &serde_json::json!({
                    "tool": "read_file",
                    "result": { "content": format!("TOOL-RESULT-{index}") },
                }),
                &serde_json::json!({}),
            )
            .await
            .unwrap();
        service.finish_turn(&turn.id, "completed").await.unwrap();
    }

    let model = Arc::new(CapturingModel::default());
    let coordinator = coordinator_with_model(
        tmp.path(),
        &service,
        model.clone() as Arc<dyn Model>,
        StopAt::Never,
        2,
    );
    let run = coordinator
        .run(&session.id, &agent.id, "finish the task")
        .await
        .unwrap();
    assert_eq!(run.status, "completed");

    let items = service.items(&run.turn.id).await.unwrap();
    let record = items
        .iter()
        .find(|item| item.status == "compacted")
        .expect("the turn records compaction");
    let dropped = record.body["dropped"].as_array().unwrap();
    assert_eq!(dropped.len(), 196);
    assert_eq!(record.body["retained_turns"].as_array().unwrap().len(), 4);
    assert!(dropped.iter().all(|span| span["turn_id"].is_string()));
    let summary = record.body["summary"].as_str().unwrap();
    assert!(summary.contains("pdf-tools @ sha-pinned"), "pins survive");
    assert!(summary.contains("TOOL-RESULT-0"), "tool results survive");

    let seen = model.seen.lock().unwrap();
    let sent = seen[0]
        .messages
        .iter()
        .filter_map(|message| message.content.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(sent.contains("Compacted history"));
    assert!(sent.contains("pdf-tools @ sha-pinned"));
    assert!(sent.contains("request 199"), "recent turns stay verbatim");
    assert_eq!(seen[0].messages.last().unwrap().role, "user");
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
async fn a_step_limit_turn_can_be_continued_with_a_fresh_budget() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("steps"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let script = r#"{
      "steps": [
        {
          "text": "Reading first.\n",
          "tool_calls": [{"name": "read_file", "arguments": {"path": "README.md"}}],
          "usage": {"input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002}
        },
        {"text": "Finished on the next turn.\n"}
      ]
    }"#;

    let first = coordinator(tmp.path(), &service, script, StopAt::Never, 1)
        .run(&session.id, &agent.id, "same task")
        .await
        .unwrap();
    assert_eq!(first.stop, StopReason::StepLimit);
    assert_eq!(first.status, "failed");
    assert_eq!(first.steps, 1);
    assert_eq!(first.turn.stop_reason.as_deref(), Some("step_limit"));

    // Q249: the user's Continue starts a new turn with the same task and a fresh bound. The
    // stopped turn stays closed, so the transcript tells the truth about what ran.
    let second = coordinator(tmp.path(), &service, script, StopAt::Never, 2)
        .run(&session.id, &agent.id, "same task")
        .await
        .unwrap();
    assert_eq!(second.stop, StopReason::Completed);
    assert_eq!(second.status, "completed");
    assert_eq!(second.steps, 2);
    assert_eq!(second.turn.prompt.as_deref(), Some("same task"));

    let transcript = service.transcript(&session.id).await.unwrap();
    assert_eq!(transcript.len(), 2);
    assert_eq!(transcript[1].turn.stop_reason.as_deref(), Some("completed"));
}

#[tokio::test]
async fn a_paused_turn_waits_for_resume_and_keeps_the_same_turn() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("pause"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let script = r#"{
      "steps": [
        {"text": "First step.\n"},
        {"text": "Second step.\n"}
      ]
    }"#;

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
        model: Arc::new(ScriptedModel::from_json(script).unwrap()),
        sink: Arc::new(rigel_core::NullSink),
        cancels: cancels.clone(),
        project_root: tmp.path().to_path_buf(),
        model_name: "script".to_string(),
        effort: None,
        max_steps: 10,
        subagents: rigel_core::agent::SubagentPolicy::default(),
        depth: 0,
        parent_cancel: None,
        worktree_dir: None,
        chat_parent: None,
    };

    let turn = service
        .append_turn(&agent.id, Some("pause me"))
        .await
        .unwrap();
    let turn_id = turn.id.clone();
    cancels.pause_turn(&turn_id);
    let paused = service.clone();
    let resume = cancels.clone();
    let paused_turn = turn_id.clone();
    let inspect = async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let items = paused.items(&paused_turn).await.unwrap();
        assert!(
            items.iter().any(|item| item.status == "paused"),
            "the boundary records the pause before it waits"
        );
        assert!(
            !items.iter().any(|item| item.status == "resumed"),
            "the turn is still waiting"
        );
        let stored = rigel_core::store::paused::PauseStore::new(paused.pool().clone())
            .load(&paused_turn)
            .await
            .unwrap()
            .expect("the pause is durable while the turn waits");
        assert!(stored.remaining_steps <= 10);
        assert_eq!(stored.messages.last().unwrap().role, "user");
        resume.resume_turn(&paused_turn);
    };
    let (run, ()) = tokio::join!(
        coordinator.run_existing(&session.id, &agent.id, turn, "pause me"),
        inspect
    );
    let run = run.unwrap();
    assert_eq!(run.stop, StopReason::Completed);
    assert_eq!(run.turn.id, turn_id, "resume continues the same turn");

    let items = service.items(&turn_id).await.unwrap();
    assert!(items.iter().any(|item| item.status == "resumed"));
    let stored = rigel_core::store::paused::PauseStore::new(service.pool().clone())
        .load(&turn_id)
        .await
        .unwrap()
        .expect("the pause row remains for audit");
    assert!(
        stored.resumed_at.is_some(),
        "the same-process resume claims it"
    );
}

#[tokio::test]
async fn a_paused_turn_reopens_with_the_same_messages_and_budget_once() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("reopen"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let turn = service
        .append_turn(&agent.id, Some("finish the report"))
        .await
        .unwrap();
    let messages = vec![
        rigel_core::agent::ChatMessage::system("RESTORED-SYSTEM-MARKER"),
        rigel_core::agent::ChatMessage::user("finish the report"),
    ];
    rigel_core::store::paused::PauseStore::new(service.pool().clone())
        .save(&rigel_core::store::paused::PausedTurn {
            turn_id: turn.id.clone(),
            session_id: session.id.clone(),
            agent_id: agent.id.clone(),
            messages: messages.clone(),
            remaining_steps: 2,
            remaining_micros: Some(1_234),
            paused_at: rigel_core::migrate::now_ms(),
            resumed_at: None,
        })
        .await
        .unwrap();

    let model = Arc::new(CapturingModel::default());
    let coordinator = coordinator_with_model(
        tmp.path(),
        &service,
        model.clone() as Arc<dyn Model>,
        StopAt::Never,
        10,
    );
    let run = coordinator
        .resume_paused(&session.id, &agent.id, &turn.id)
        .await
        .unwrap();
    assert_eq!(run.status, "completed");
    {
        let seen = model.seen.lock().unwrap();
        assert_eq!(seen[0].messages.len(), 2);
        assert_eq!(
            seen[0].messages[0].content.as_deref(),
            Some("RESTORED-SYSTEM-MARKER")
        );
        assert_eq!(
            seen[0].messages[1].content.as_deref(),
            Some("finish the report")
        );
    }
    assert!(
        coordinator
            .resume_paused(&session.id, &agent.id, &turn.id)
            .await
            .is_err(),
        "a double resume is refused"
    );
}

#[tokio::test]
async fn a_stream_that_dies_mid_answer_keeps_the_partial_text_and_bills_it() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("partial"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let script = r#"{
      "steps": [
        {
          "text": "The fix starts in src/lib.rs, but the connection",
          "usage": {"input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002},
          "error_after": "connection reset by peer"
        }
      ]
    }"#;

    let error = coordinator(tmp.path(), &service, script, StopAt::Never, 10)
        .run(&session.id, &agent.id, "finish the fix")
        .await
        .unwrap_err();
    assert!(
        matches!(error, rigel_core::AgentError::Model(_)),
        "unexpected error: {error:?}"
    );

    let transcript = service.transcript(&session.id).await.unwrap();
    let turn = &transcript[0].turn;
    assert_eq!(turn.status, "incomplete");
    assert_eq!(turn.stop_reason.as_deref(), Some("model_error"));

    let message = transcript[0]
        .items
        .iter()
        .find(|item| item.kind == "message")
        .expect("the partial answer is kept");
    assert_eq!(message.status, "incomplete");
    assert!(
        message.body["text"]
            .as_str()
            .unwrap()
            .contains("src/lib.rs")
    );

    // The provider reported usage before the connection broke, so that is the bill. The row
    // names the step that spent it, and the turn/session totals reconcile with that row.
    let usage = turn.usage.as_ref().unwrap();
    assert_eq!(usage.cost_micros, 200);
    let usage_row = transcript[0]
        .items
        .iter()
        .find(|item| item.kind == "usage")
        .expect("the partial step has a usage row");
    assert_eq!(usage_row.turn_id, turn.id);
    assert_eq!(usage_row.body["step"], 1);
    assert_eq!(usage_row.body["incomplete"], true);
    assert_eq!(service.get(&session.id).await.unwrap().spend_micros, 200);

    // A provider that only reports usage at the end never sent it: charge the local estimate
    // for the tokens it did deliver rather than recording a free step.
    let estimated =
        r#"{"steps":[{"text":"partial words with no usage chunk","error_after":"gone"}]}"#;
    let second = service.start(Some("estimated"), Some(25)).await.unwrap();
    let second_agent = service
        .register_agent(&second.id, "native", Some("worker"))
        .await
        .unwrap();
    coordinator(tmp.path(), &service, estimated, StopAt::Never, 10)
        .run(&second.id, &second_agent.id, "estimate it")
        .await
        .unwrap_err();
    let transcript = service.transcript(&second.id).await.unwrap();
    let usage = transcript[0].turn.usage.as_ref().unwrap();
    assert!(usage.cost_micros > 0, "partial output is never free");
    assert_eq!(usage.source, rigel_core::CostSource::Estimated);

    // A stream that dies before any content is still a plain failure with no message row.
    let empty = r#"{"steps":[{"error":"boom"}]}"#;
    let failed = service.start(Some("empty"), Some(25)).await.unwrap();
    let failed_agent = service
        .register_agent(&failed.id, "native", Some("worker"))
        .await
        .unwrap();
    coordinator(tmp.path(), &service, empty, StopAt::Never, 10)
        .run(&failed.id, &failed_agent.id, "nothing")
        .await
        .unwrap_err();
    let transcript = service.transcript(&failed.id).await.unwrap();
    assert_eq!(transcript[0].turn.status, "failed");
    assert!(
        transcript[0]
            .items
            .iter()
            .all(|item| item.kind != "message")
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
    child_stuck: bool,
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
            if self.child_stuck {
                on_delta(Delta::ToolCall {
                    index: 0,
                    id: Some("child_read".to_string()),
                    name: Some("read_file".to_string()),
                    arguments: serde_json::json!({ "path": "README.md" }).to_string(),
                });
                on_delta(Delta::Usage(RawUsage {
                    input_tokens: 50,
                    cached_input_tokens: 0,
                    output_tokens: 10,
                    reasoning_tokens: 0,
                    cost_usd: Some(0.0003),
                    served_model: None,
                }));
                on_delta(Delta::Finish("tool_calls".to_string()));
                return Ok(());
            }
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
                served_model: None,
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
                served_model: None,
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
                served_model: None,
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
async fn a_subagent_stops_at_its_own_step_cap() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("child-cap"), Some(25)).await.unwrap();
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
        model: Arc::new(SubagentScriptModel {
            child_stuck: true,
            ..Default::default()
        }),
        sink: Arc::new(rigel_core::NullSink),
        cancels,
        project_root: tmp.path().to_path_buf(),
        model_name: "subagent-script".to_string(),
        effort: None,
        max_steps: 10,
        subagents: rigel_core::agent::SubagentPolicy {
            max_steps: 2,
            ..Default::default()
        },
        depth: 0,
        parent_cancel: None,
        worktree_dir: None,
        chat_parent: None,
    }
    .run(&session.id, &agent.id, "cap the children")
    .await
    .unwrap();

    assert_eq!(run.stop, StopReason::Completed);
    let transcript = service.transcript(&session.id).await.unwrap();
    let children: Vec<_> = transcript
        .iter()
        .filter(|entry| entry.turn.parent_turn_id.as_deref() == Some(run.turn.id.as_str()))
        .collect();
    assert_eq!(children.len(), 2);
    for child in &children {
        assert_eq!(child.turn.status, "failed");
        assert_eq!(child.turn.stop_reason.as_deref(), Some("step_limit"));
    }

    let parent = transcript
        .iter()
        .find(|entry| entry.turn.id == run.turn.id)
        .expect("the parent turn is in the transcript");
    let results = parent
        .items
        .iter()
        .find(|item| {
            item.body.get("tool").and_then(|tool| tool.as_str()) == Some("subagent_results")
        })
        .expect("the collect call is in the parent transcript");
    for child in results.body["result"]["agents"].as_array().unwrap() {
        assert_eq!(child["stop"], "step_limit");
    }
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

/// A provider stream that delivers text and usage, then waits for the stop. This is the shape a
/// real cancelled stream has: the provider already charged for what it sent.
struct HangingModel;

#[async_trait]
impl Model for HangingModel {
    fn id(&self) -> String {
        "hanging".to_string()
    }

    async fn chat(
        &self,
        _request: ChatRequest,
        cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError> {
        on_delta(Delta::Text("Half an answer".to_string()));
        on_delta(Delta::Usage(RawUsage {
            input_tokens: 100,
            cached_input_tokens: 0,
            output_tokens: 20,
            reasoning_tokens: 0,
            cost_usd: Some(0.0002),
            served_model: None,
        }));
        while !cancel.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Err(ModelError::Cancelled)
    }
}

#[tokio::test]
async fn a_cancelled_stream_is_billed_once_and_closes_as_cancelled() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("cancel"), Some(25)).await.unwrap();
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
    let coordinator = Coordinator {
        session: service.clone(),
        budget,
        tools,
        model: Arc::new(HangingModel),
        sink: Arc::new(rigel_core::NullSink),
        cancels: cancels.clone(),
        project_root: tmp.path().to_path_buf(),
        model_name: "hanging".to_string(),
        effort: None,
        max_steps: 5,
        subagents: rigel_core::agent::SubagentPolicy::default(),
        depth: 0,
        parent_cancel: None,
        worktree_dir: None,
        chat_parent: None,
    };

    let turn = service
        .append_turn(&agent.id, Some("cancel me"))
        .await
        .unwrap();
    let turn_id = turn.id.clone();
    let stop = cancels.clone();
    let stop_id = turn_id.clone();
    let trigger = async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        stop.cancel_turn(&stop_id);
    };
    let (run, ()) = tokio::join!(
        coordinator.run_existing(&session.id, &agent.id, turn, "cancel me"),
        trigger
    );
    let run = run.unwrap();
    assert_eq!(run.stop, StopReason::Cancelled);
    assert_eq!(run.status, "cancelled");
    assert_eq!(
        run.usage.cost_micros, 200,
        "the cancelled stream was charged by the provider"
    );

    let items = service.items(&turn_id).await.unwrap();
    assert_eq!(
        items.iter().filter(|item| item.kind == "usage").count(),
        1,
        "one partial usage row, not zero and not two"
    );
    let partial = items
        .iter()
        .find(|item| item.kind == "message")
        .expect("the partial answer is kept");
    assert_eq!(partial.status, "incomplete");
    assert!(
        partial.body["text"]
            .as_str()
            .is_some_and(|text| text.contains("Half an answer")),
        "the partial text is kept and marked incomplete"
    );
    assert_eq!(
        service.get(&session.id).await.unwrap().spend_micros,
        200,
        "the cancelled turn is on the bill exactly once"
    );
}

/// A provider that reports a limiter wait before it answers. The delta is the real contract:
/// both HTTP adapters emit it from the shared bucket.
struct RateLimitedModel;

#[async_trait]
impl Model for RateLimitedModel {
    fn id(&self) -> String {
        "rate-limited".to_string()
    }

    async fn chat(
        &self,
        _request: ChatRequest,
        _cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError> {
        on_delta(Delta::RateLimit {
            provider: "groq".to_string(),
            waited_ms: 1_200,
        });
        on_delta(Delta::Text("Answered after the wait.\n".to_string()));
        on_delta(Delta::Finish("stop".to_string()));
        Ok(())
    }
}

#[tokio::test]
async fn a_limiter_wait_is_a_rate_limit_row_in_the_transcript() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("rate-limit"), Some(25)).await.unwrap();
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
    let coordinator = Coordinator {
        session: service.clone(),
        budget,
        tools,
        model: Arc::new(RateLimitedModel),
        sink: Arc::new(rigel_core::NullSink),
        cancels,
        project_root: tmp.path().to_path_buf(),
        model_name: "rate-limited".to_string(),
        effort: None,
        max_steps: 5,
        subagents: rigel_core::agent::SubagentPolicy::default(),
        depth: 0,
        parent_cancel: None,
        worktree_dir: None,
        chat_parent: None,
    };

    let run = coordinator
        .run(&session.id, &agent.id, "wait for the door")
        .await
        .unwrap();
    assert_eq!(run.status, "completed");

    let items = service.items(&run.turn.id).await.unwrap();
    let wait = items
        .iter()
        .find(|item| item.status == "rate_limit")
        .expect("the wait is a transcript row, not a silent delay");
    assert_eq!(wait.kind, "message");
    assert_eq!(wait.body["provider"], "groq");
    assert_eq!(wait.body["waited_ms"], 1_200);
}

#[tokio::test]
async fn continue_turn_records_its_bound_and_keeps_the_session_spend() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("continue"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let script = r#"{
      "steps": [
        {
          "text": "Reading first.\n",
          "tool_calls": [{"name": "read_file", "arguments": {"path": "README.md"}}],
          "usage": {"input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002}
        },
        {"text": "Finished on the continuation.\n", "usage": {"input_tokens": 50, "output_tokens": 10, "cost_usd": 0.0001}}
      ]
    }"#;

    let first = coordinator(tmp.path(), &service, script, StopAt::Never, 1)
        .run(&session.id, &agent.id, "same task")
        .await
        .unwrap();
    assert_eq!(first.stop, StopReason::StepLimit);
    assert_eq!(first.step_budget, 1);
    assert_eq!(first.usage.cost_micros, 200);

    let continued = coordinator(tmp.path(), &service, script, StopAt::Never, 9)
        .continue_turn(&session.id, &agent.id, "same task", 2)
        .await
        .unwrap();
    assert_eq!(continued.stop, StopReason::Completed);
    assert_eq!(continued.step_budget, 2);
    assert_eq!(continued.steps, 2);
    assert_eq!(continued.usage.cost_micros, 300);

    let items = service.items(&continued.turn.id).await.unwrap();
    let bound = items
        .iter()
        .find(|item| item.status == "continue")
        .expect("the continuation records its stated bound");
    assert_eq!(bound.body["max_steps"], 2);
    assert_eq!(bound.body["previous_spend_micros"], 200);

    // The ledger continues: 200 from the stopped turn plus 300 from the continuation.
    assert_eq!(service.get(&session.id).await.unwrap().spend_micros, 500);
}

#[tokio::test]
async fn continue_turn_refuses_a_zero_bound() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("continue"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let error = coordinator(tmp.path(), &service, SCRIPT, StopAt::Never, 1)
        .continue_turn(&session.id, &agent.id, "same task", 0)
        .await
        .unwrap_err();
    assert!(matches!(error, rigel_core::AgentError::InvalidStepBudget));
    assert!(service.transcript(&session.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_near_miss_tool_call_is_repaired_once_and_a_second_failure_keeps_the_raw_call() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("repair"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let script = r#"{
      "steps": [
        {
          "text": "Reading with the wrong spelling.\n",
          "tool_calls": [
            {"name": "Read-File", "arguments": {"file_path": "README.md"}},
            {"name": "read_file", "arguments": "```json\n{\"path\":\"src/lib.rs\"}\n```"}
          ]
        },
        {
          "text": "Trying a repaired call with no path.\n",
          "tool_calls": [{"name": "Read-File", "arguments": {}}]
        },
        {"text": "Done.\n"}
      ]
    }"#;

    let run = coordinator(tmp.path(), &service, script, StopAt::Never, 4)
        .run(&session.id, &agent.id, "repair these calls")
        .await
        .unwrap();
    assert_eq!(run.status, "completed");

    let items = service.items(&run.turn.id).await.unwrap();
    let repaired: Vec<_> = items
        .iter()
        .filter(|item| item.status == "repaired")
        .collect();
    assert_eq!(repaired.len(), 3, "one repair row per corrected call");
    assert!(repaired.iter().all(|item| item.body["tool"].is_string()));
    assert!(
        repaired
            .iter()
            .any(|item| item.body["reason"].as_str().unwrap().contains("file_path")),
        "the reason names the field it changed"
    );

    // The repaired-but-still-invalid call is a tool error, and the row carries both the
    // attempted repair and the raw call the model actually sent.
    let failed = items
        .iter()
        .find(|item| item.status == "error" && item.body["repair_failed"] == true)
        .expect("the second failure is recorded");
    assert_eq!(failed.body["raw_call"]["name"], "Read-File");
    assert_eq!(failed.body["requested_tool"], "Read-File");
    assert!(
        failed.body["repair_attempted"]
            .as_str()
            .unwrap()
            .contains("unknown tool"),
        "the attempted repair is named"
    );
    assert_eq!(failed.body["result"]["error"]["code"], "BAD_ARGUMENTS");
}

#[tokio::test]
async fn usage_rows_carry_step_wall_and_queue_time() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("timing"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let run = coordinator(tmp.path(), &service, SCRIPT, StopAt::Never, 3)
        .run(&session.id, &agent.id, "time the steps")
        .await
        .unwrap();
    let items = service.items(&run.turn.id).await.unwrap();
    let usage = items
        .iter()
        .find(|item| item.kind == "usage")
        .expect("a usage row");
    assert_eq!(usage.body["step"], 1);
    assert!(usage.body["duration_ms"].is_u64());
    assert!(usage.body["queue_ms"].is_u64());
    assert!(usage.body["cost_micros"].as_i64().unwrap() > 0);
    let exported = serde_json::to_value(usage).unwrap();
    assert_eq!(
        exported["body"], usage.body,
        "export carries the same step numbers"
    );
}

#[derive(Default)]
struct SteeringModel {
    started: Notify,
    release: Notify,
    calls: AtomicUsize,
}

#[async_trait]
impl Model for SteeringModel {
    fn id(&self) -> String {
        "script".to_string()
    }

    async fn chat(
        &self,
        _request: ChatRequest,
        _cancel: &CancellationToken,
        on_delta: &mut DeltaSink<'_>,
    ) -> Result<(), ModelError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.started.notify_one();
            self.release.notified().await;
            on_delta(Delta::ToolCall {
                index: 0,
                id: Some("call_0".to_string()),
                name: Some("read_file".to_string()),
                arguments: r#"{"path":"README.md"}"#.to_string(),
            });
            on_delta(Delta::Finish("tool_calls".to_string()));
        } else {
            on_delta(Delta::Text("done\n".to_string()));
            on_delta(Delta::Finish("stop".to_string()));
        }
        Ok(())
    }
}

#[tokio::test]
async fn a_steered_note_lands_once_at_the_next_step_and_names_that_step() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("steer"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let model = Arc::new(SteeringModel::default());
    let coordinator = coordinator_with_model(
        tmp.path(),
        &service,
        model.clone() as Arc<dyn Model>,
        StopAt::Never,
        3,
    );
    let cancels = coordinator.cancels.clone();
    let run = coordinator.run(&session.id, &agent.id, "start the work");
    tokio::pin!(run);
    let mut steered = false;
    let run = loop {
        tokio::select! {
            run = &mut run => break run.unwrap(),
            _ = model.started.notified(), if !steered => {
                steered = true;
                let turn_id = loop {
                    if let Some(turn) = service
                        .running_turns(&session.id)
                        .await
                        .unwrap()
                        .into_iter()
                        .find(|turn| turn.agent_id == agent.id)
                    {
                        break turn.id;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                };
                assert!(cancels.steer(&turn_id, "also read src/main.rs"));
                model.release.notify_waiters();
            }
        }
    };
    assert_eq!(run.status, "completed");

    let items = service.items(&run.turn.id).await.unwrap();
    let notes = items
        .iter()
        .filter(|item| item.status == "user_intervention")
        .collect::<Vec<_>>();
    assert_eq!(notes.len(), 1, "the note lands exactly once");
    assert_eq!(notes[0].body["text"], "also read src/main.rs");
    assert_eq!(notes[0].body["step"], 2, "it names the step it joined");
}

#[tokio::test]
async fn a_guaranteed_context_overflow_is_refused_before_the_model_runs() {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("overflow"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let huge = "x".repeat(2_000_000);

    let error = coordinator(tmp.path(), &service, SCRIPT, StopAt::Never, 2)
        .run(&session.id, &agent.id, &huge)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            rigel_core::AgentError::Model(ModelError::ContextOverflow { .. })
        ),
        "unexpected error: {error:?}"
    );

    let transcript = service.transcript(&session.id).await.unwrap();
    assert_eq!(transcript[0].turn.status, "failed");
    let refusal = transcript[0]
        .items
        .iter()
        .find(|item| item.kind == "error")
        .expect("the refusal is on the transcript");
    assert_eq!(refusal.body["context"]["fits"], false);
    assert_eq!(refusal.body["context"]["margin_pct"], 15);
    assert!(refusal.body["context"]["window"].as_u64().unwrap() > 0);
    assert!(
        refusal.body["context"]["low"].as_u64().unwrap()
            > refusal.body["context"]["window"].as_u64().unwrap()
    );
}

/// Roadmap 42: a parent names two children and makes the second wait for the first. The child
/// turns are real coordinator turns; the model only chooses the answers and the spawn order.
#[derive(Default)]
struct DagScriptModel {
    parent_calls: AtomicUsize,
    child_calls: AtomicUsize,
    fail_first: bool,
}

#[async_trait]
impl Model for DagScriptModel {
    fn id(&self) -> String {
        "dag-script".to_string()
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
            let call = self.child_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_first && call == 0 {
                return Err(ModelError::Other("the first child failed".to_string()));
            }
            let text = if last_user.contains("FIRST") {
                "first answer\n"
            } else {
                "second answer\n"
            };
            on_delta(Delta::Text(text.to_string()));
            on_delta(Delta::Finish("stop".to_string()));
            return Ok(());
        }

        let call = self.parent_calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            on_delta(Delta::ToolCall {
                index: 0,
                id: Some("call_0".to_string()),
                name: Some("spawn_agents".to_string()),
                arguments: serde_json::json!({
                    "agents": [
                        {
                            "id": "first",
                            "role": "first",
                            "files": ["README.md"],
                            "prompt": "Produce the FIRST answer."
                        },
                        {
                            "id": "second",
                            "role": "second",
                            "files": ["src/lib.rs"],
                            "prompt": "Produce the SECOND answer.",
                            "depends_on": ["first"]
                        }
                    ]
                })
                .to_string(),
            });
            on_delta(Delta::Finish("tool_calls".to_string()));
        } else if call == 1 {
            on_delta(Delta::ToolCall {
                index: 0,
                id: Some("call_1".to_string()),
                name: Some("subagent_results".to_string()),
                arguments: serde_json::json!({ "timeout_secs": 20 }).to_string(),
            });
            on_delta(Delta::Finish("tool_calls".to_string()));
        } else {
            on_delta(Delta::Text("Coordinated the two children.\n".to_string()));
            on_delta(Delta::Finish("stop".to_string()));
        }
        Ok(())
    }
}

async fn dag_coordinator(
    fail_first: bool,
) -> (
    tempfile::TempDir,
    SessionService,
    String,
    String,
    Coordinator,
) {
    let tmp = repo();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("subagent-dag"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("lead"))
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
    let coordinator = Coordinator {
        session: service.clone(),
        budget,
        tools,
        model: Arc::new(DagScriptModel {
            fail_first,
            ..DagScriptModel::default()
        }),
        sink: Arc::new(rigel_core::NullSink),
        cancels,
        project_root: tmp.path().to_path_buf(),
        model_name: "dag-script".to_string(),
        effort: None,
        max_steps: 5,
        subagents: rigel_core::agent::SubagentPolicy::default(),
        depth: 0,
        worktree_dir: None,
        chat_parent: None,
        parent_cancel: None,
    };
    (tmp, service, session.id, agent.id, coordinator)
}

#[tokio::test]
async fn a_subagent_waits_for_its_named_sibling_and_hands_its_answer_back() {
    let (_tmp, service, session_id, agent_id, coordinator) = dag_coordinator(false).await;
    let run = coordinator
        .run(&session_id, &agent_id, "coordinate the two readers")
        .await
        .unwrap();
    assert_eq!(run.stop, StopReason::Completed);

    let agents = service.agents(&session_id).await.unwrap();
    let role_of = |agent_id: &str| {
        agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .and_then(|agent| agent.role.clone())
    };
    let turns = service.transcript(&session_id).await.unwrap();
    let children: Vec<_> = turns
        .iter()
        .filter(|entry| entry.turn.parent_turn_id.as_deref() == Some(run.turn.id.as_str()))
        .collect();
    assert_eq!(children.len(), 2, "both children ran a real child turn");
    let first = children
        .iter()
        .find(|entry| role_of(&entry.turn.agent_id).as_deref() == Some("first"))
        .expect("the first child has a turn");
    let second = children
        .iter()
        .find(|entry| role_of(&entry.turn.agent_id).as_deref() == Some("second"))
        .expect("the second child has a turn");
    assert_eq!(first.turn.status, "completed");
    assert_eq!(second.turn.status, "completed");
    assert!(
        second.turn.started_at >= first.turn.completed_at.unwrap(),
        "the dependent child starts only after its prerequisite completed"
    );

    let graphs = service.dag(&session_id).await.unwrap();
    assert_eq!(graphs.len(), 1);
    assert_eq!(graphs[0].parent_turn_id, run.turn.id);
    let first_node = graphs[0]
        .nodes
        .iter()
        .find(|node| node.id == "first")
        .unwrap();
    let second_node = graphs[0]
        .nodes
        .iter()
        .find(|node| node.id == "second")
        .unwrap();
    assert_eq!(first_node.state, NodeState::Done);
    assert_eq!(second_node.state, NodeState::Done);
    assert_eq!(second_node.depends_on, vec!["first".to_string()]);

    let parent = turns
        .iter()
        .find(|entry| entry.turn.id == run.turn.id)
        .unwrap();
    let spawn = parent
        .items
        .iter()
        .find(|item| item.body.get("tool").and_then(|tool| tool.as_str()) == Some("spawn_agents"))
        .expect("the spawn call is in the transcript");
    assert_eq!(spawn.body["result"]["edges"][0]["from"], "first");
    assert_eq!(spawn.body["result"]["edges"][0]["to"], "second");
    let results = parent
        .items
        .iter()
        .find(|item| {
            item.body.get("tool").and_then(|tool| tool.as_str()) == Some("subagent_results")
        })
        .expect("the collect call is in the transcript");
    assert!(
        results.body["result"]["agents"][1]["text"]
            .as_str()
            .unwrap()
            .contains("second answer"),
        "the dependent's answer is handed back to the parent"
    );
    assert_eq!(
        results.body["result"]["graph"][0]["nodes"][1]["state"],
        "done"
    );
}

#[tokio::test]
async fn a_failed_prerequisite_blocks_its_dependent_and_reports_why() {
    let (_tmp, service, session_id, agent_id, coordinator) = dag_coordinator(true).await;
    let run = coordinator
        .run(&session_id, &agent_id, "coordinate despite a failure")
        .await
        .unwrap();
    assert_eq!(run.stop, StopReason::Completed);

    let agents = service.agents(&session_id).await.unwrap();
    let role_of = |agent_id: &str| {
        agents
            .iter()
            .find(|agent| agent.id == agent_id)
            .and_then(|agent| agent.role.clone())
    };
    let turns = service.transcript(&session_id).await.unwrap();
    let children: Vec<_> = turns
        .iter()
        .filter(|entry| entry.turn.parent_turn_id.as_deref() == Some(run.turn.id.as_str()))
        .collect();
    let first = children
        .iter()
        .find(|entry| role_of(&entry.turn.agent_id).as_deref() == Some("first"))
        .expect("the failed child has a turn");
    assert_eq!(first.turn.status, "failed");
    assert!(
        children
            .iter()
            .all(|entry| role_of(&entry.turn.agent_id).as_deref() != Some("second")),
        "the blocked child never starts a turn"
    );

    let graphs = service.dag(&session_id).await.unwrap();
    let first_node = graphs[0]
        .nodes
        .iter()
        .find(|node| node.id == "first")
        .unwrap();
    let second_node = graphs[0]
        .nodes
        .iter()
        .find(|node| node.id == "second")
        .unwrap();
    assert_eq!(first_node.state, NodeState::Failed);
    assert_eq!(second_node.state, NodeState::Blocked);
    assert!(
        second_node
            .detail
            .as_deref()
            .unwrap_or_default()
            .contains("first"),
        "the blocked node names the prerequisite that broke"
    );

    let parent = turns
        .iter()
        .find(|entry| entry.turn.id == run.turn.id)
        .unwrap();
    let results = parent
        .items
        .iter()
        .find(|item| {
            item.body.get("tool").and_then(|tool| tool.as_str()) == Some("subagent_results")
        })
        .expect("the collect call is in the transcript");
    let blocked = &results.body["result"]["agents"][1];
    assert_eq!(blocked["ok"], false);
    assert_eq!(blocked["code"], "DEPENDENCY_FAILED");
    assert!(blocked["error"].as_str().unwrap().contains("first"));
}

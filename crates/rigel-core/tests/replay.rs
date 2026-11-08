//! Deterministic replay (roadmap 38).
//!
//! A real turn is recorded from its `AgentEvent` stream. The recorded assistant/tool items are
//! turned back into a script, replayed through the production coordinator against the same
//! project, and compared by tool order and a transcript hash. The second test changes the
//! recorded tool order and asserts the comparison fails by name.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::{Arc, Mutex};

use rigel_core::agent::scripted::{ScriptStep, ScriptToolCall};
use rigel_core::agent::{Cancels, Coordinator, Script, ScriptedModel};
use rigel_core::{AgentEvent, BudgetService, EventSink, Item, SessionService, StopAt, Tools};
use serde_json::json;

#[derive(Default)]
struct RecordingSink(Mutex<Vec<AgentEvent>>);

impl EventSink for RecordingSink {
    fn emit(&self, event: AgentEvent) {
        self.0.lock().unwrap().push(event);
    }
}

fn tools(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter(|item| matches!(item.kind.as_str(), "mcp_call" | "command" | "file_change"))
        .filter_map(|item| item.body["tool"].as_str().map(str::to_string))
        .collect()
}

/// The transcript hash deliberately excludes ids, timestamps and the timing fields that are
/// measured rather than replayed. Everything a tool or model decided stays in.
fn transcript_hash(items: &[Item]) -> u64 {
    let mut canonical = Vec::new();
    for item in items {
        let mut body = item.body.clone();
        if item.kind == "usage"
            && let Some(object) = body.as_object_mut()
        {
            object.remove("duration_ms");
            object.remove("queue_ms");
        }
        canonical.push(json!({
            "kind": item.kind,
            "status": item.status,
            "body": body,
        }));
    }
    let text = serde_json::to_string(&canonical).unwrap();
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, PartialEq, Eq)]
enum Mismatch {
    ToolOrder,
    Transcript,
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ToolOrder => f.write_str("tool order changed"),
            Self::Transcript => f.write_str("transcript hash changed"),
        }
    }
}

fn compare(recorded: &[Item], replayed: &[Item]) -> Result<(), Mismatch> {
    if tools(recorded) != tools(replayed) {
        return Err(Mismatch::ToolOrder);
    }
    if transcript_hash(recorded) != transcript_hash(replayed) {
        return Err(Mismatch::Transcript);
    }
    Ok(())
}

/// Convert the recorded assistant/tool items back into a model script. An assistant message
/// starts the next step; each following tool item belongs to it.
fn script_from_items(items: &[Item]) -> Script {
    let mut steps: Vec<ScriptStep> = Vec::new();
    for item in items {
        if item.kind == "message" && item.status == "complete" && item.body["role"] == "assistant" {
            steps.push(ScriptStep {
                text: item.body["text"].as_str().map(str::to_string),
                ..ScriptStep::default()
            });
            continue;
        }
        if !matches!(item.kind.as_str(), "mcp_call" | "command" | "file_change") {
            continue;
        }
        let Some(step) = steps.last_mut() else {
            continue;
        };
        let (Some(name), Some(arguments)) = (
            item.body["tool"].as_str(),
            item.body.get("arguments").cloned(),
        ) else {
            continue;
        };
        step.tool_calls.push(ScriptToolCall {
            name: name.to_string(),
            arguments,
        });
    }
    Script {
        steps,
        subagent_steps: Vec::new(),
    }
}

async fn run(
    root: &Path,
    service: &SessionService,
    session: &str,
    agent: &str,
    script: Script,
    sink: Arc<RecordingSink>,
) -> Vec<Item> {
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
        StopAt::Never,
    );
    let coordinator = Coordinator {
        session: service.clone(),
        budget,
        tools,
        model: Arc::new(ScriptedModel::new(script)),
        sink,
        cancels,
        project_root: root.to_path_buf(),
        model_name: "script".to_string(),
        effort: None,
        max_steps: 5,
        subagents: rigel_core::agent::SubagentPolicy::default(),
        depth: 0,
        parent_cancel: None,
        worktree_dir: None,
        chat_parent: None,
    };
    let run = coordinator
        .run(session, agent, "replay this turn")
        .await
        .unwrap();
    service.items(&run.turn.id).await.unwrap()
}

#[tokio::test]
async fn a_recorded_turn_replays_with_the_same_tool_order_and_transcript_hash() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("README.md"), "hello\n").unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("replay"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let script = serde_json::from_value::<Script>(json!({
        "steps": [
            {
                "text": "Reading two files.\n",
                "tool_calls": [
                    {"name": "read_file", "arguments": {"path": "README.md"}},
                    {"name": "list_files", "arguments": {"path": "."}}
                ]
            },
            {"text": "Done.\n"}
        ]
    }))
    .unwrap();

    let sink = Arc::new(RecordingSink::default());
    let recorded = run(
        tmp.path(),
        &service,
        &session.id,
        &agent.id,
        script.clone(),
        sink.clone(),
    )
    .await;
    let recorded_events = sink.0.lock().unwrap().clone();
    assert_eq!(tools(&recorded), vec!["read_file", "list_files"]);
    assert!(
        recorded_events
            .iter()
            .any(|event| matches!(event, AgentEvent::Item { .. }))
    );

    let replayed_script = script_from_items(&recorded);
    let replayed = run(
        tmp.path(),
        &service,
        &session.id,
        &agent.id,
        replayed_script,
        Arc::new(RecordingSink::default()),
    )
    .await;
    compare(&recorded, &replayed).unwrap();
}

#[tokio::test]
async fn a_changed_tool_order_fails_the_replay_by_name() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("README.md"), "hello\n").unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("replay-order"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let mut script = serde_json::from_value::<Script>(json!({
        "steps": [
            {
                "text": "Reading two files.\n",
                "tool_calls": [
                    {"name": "read_file", "arguments": {"path": "README.md"}},
                    {"name": "list_files", "arguments": {"path": "."}}
                ]
            },
            {"text": "Done.\n"}
        ]
    }))
    .unwrap();
    let sink = Arc::new(RecordingSink::default());
    let recorded = run(
        tmp.path(),
        &service,
        &session.id,
        &agent.id,
        script.clone(),
        sink,
    )
    .await;

    script.steps[0].tool_calls.swap(0, 1);
    let replayed = run(
        tmp.path(),
        &service,
        &session.id,
        &agent.id,
        script,
        Arc::new(RecordingSink::default()),
    )
    .await;
    let error = compare(&recorded, &replayed).unwrap_err();
    assert_eq!(error, Mismatch::ToolOrder);
    assert!(error.to_string().contains("tool order changed"));
}

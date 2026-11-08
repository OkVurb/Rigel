//! Roadmap 41: one approved tool plan, no second ask inside it, fresh approval outside it.
//!
//! The test goes through the production `Tools` dispatcher, the same one the coordinator calls,
//! with a real recorder in place of the window's approval card.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rigel_core::{
    ApprovalScope, BudgetService, Cancels, CommandApprover, CommandDecision, CommandRequest,
    SessionService, StopAt, Tools, plan,
};
use serde_json::json;

/// Counts how many times the ordinary command card would have been shown.
struct Recorder {
    asks: Mutex<usize>,
}

impl Recorder {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            asks: Mutex::new(0),
        })
    }

    fn asks(&self) -> usize {
        *self.asks.lock().unwrap()
    }
}

#[async_trait]
impl CommandApprover for Recorder {
    async fn approve(&self, _request: &CommandRequest) -> CommandDecision {
        *self.asks.lock().unwrap() += 1;
        CommandDecision::Allow(ApprovalScope::Once)
    }
}

#[tokio::test]
async fn the_approved_tools_run_without_asking_and_an_outside_tool_rearms_the_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("approved-plan"), None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let cancels = Arc::new(Cancels::new());
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels as Arc<dyn rigel_core::InterruptSink>,
    ));
    let recorder = Recorder::new();
    let tools = Tools::new(
        tmp.path().to_path_buf(),
        service.clone(),
        budget,
        tmp.path().join("index"),
        StopAt::Never,
    )
    .with_command_approval(recorder.clone());

    // The agent proposes a tool plan while plan mode is on. Nothing is approved or executable.
    tools.set_plan_mode(true);
    let proposed = tools
        .execute(
            &session.id,
            &agent.id,
            "propose_plan",
            json!({
                "plan": "1. Run echo inside.",
                "tools": ["run_command"]
            }),
        )
        .await;
    assert!(proposed.ok, "{:?}", proposed.body);
    let state = plan::state(service.pool(), &session.id).await.unwrap();
    assert!(state.mode);
    assert_eq!(state.tools, vec!["run_command".to_string()]);
    assert_eq!(state.plan.as_deref(), Some("1. Run echo inside."));

    let refused = tools
        .execute(
            &session.id,
            &agent.id,
            "run_command",
            json!({ "command": "echo before-approval" }),
        )
        .await;
    assert_eq!(refused.body["error"]["code"], "PLAN_MODE");
    assert_eq!(recorder.asks(), 0, "plan mode refuses before any card");

    // The user approves once; the next turn reads the approved tool set from the session.
    let approved = plan::set(service.pool(), &session.id, false).await.unwrap();
    assert!(!approved.mode);
    assert!(approved.approved_at.is_some());
    tools.set_plan_mode(false);
    tools.set_approved_plan(Some(approved.tools.clone()));

    let inside = tools
        .execute(
            &session.id,
            &agent.id,
            "run_command",
            json!({ "command": "echo inside-plan" }),
        )
        .await;
    assert!(inside.ok, "{:?}", inside.body);
    assert_eq!(
        recorder.asks(),
        0,
        "a tool inside the approved plan must not ask again"
    );

    // A tool outside the plan does not run on the old approval: it re-arms the plan gate and
    // keeps the proposal so the window can ask for a fresh approval.
    tools.set_approved_plan(Some(vec!["edit_file".to_string()]));
    let outside = tools
        .execute(
            &session.id,
            &agent.id,
            "run_command",
            json!({ "command": "echo outside-plan" }),
        )
        .await;
    assert!(!outside.ok);
    assert_eq!(outside.body["error"]["code"], "PLAN_OUTSIDE");
    assert_eq!(recorder.asks(), 0, "the outside tool never runs silently");
    let rearmed = plan::state(service.pool(), &session.id).await.unwrap();
    assert!(rearmed.mode, "an outside tool turns the plan guard back on");
    assert!(rearmed.approved_at.is_none(), "the old hand-off is cleared");
    assert_eq!(
        rearmed.tools,
        vec!["run_command".to_string()],
        "the proposal stays available for the fresh approval"
    );
}

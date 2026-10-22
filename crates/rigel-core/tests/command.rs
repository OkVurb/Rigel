//! `run_command` against a real shell: consent, the cwd jail, the timeout and the exit code.
//!
//! Every test here goes through the same `Tools` dispatcher the coordinator calls, so what is
//! asserted is what the agent gets — including the item kind the transcript records.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rigel_core::{
    ApprovalScope, BudgetService, Cancels, CommandApprover, CommandDecision, CommandRequest,
    Deliberation, Deliberator, Finding, ItemKind, Position, Scopes, SessionService, StopAt,
    ToolOutcome, Tools,
};

/// Records every request it sees, then answers with one fixed decision.
struct Recorder {
    decision: CommandDecision,
    seen: Mutex<Vec<CommandRequest>>,
}

impl Recorder {
    fn new(decision: CommandDecision) -> Arc<Self> {
        Arc::new(Self {
            decision,
            seen: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<CommandRequest> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait]
impl CommandApprover for Recorder {
    async fn approve(&self, request: &CommandRequest) -> CommandDecision {
        self.seen.lock().unwrap().push(request.clone());
        self.decision
    }
}

async fn tools(root: &std::path::Path, approver: Arc<dyn CommandApprover>) -> Tools {
    tools_with(root, approver, None).await
}

async fn tools_with(
    root: &std::path::Path,
    approver: Arc<dyn CommandApprover>,
    deliberator: Option<Arc<dyn Deliberator>>,
) -> Tools {
    let service = SessionService::open(root).await.unwrap();
    let cancels = Arc::new(Cancels::new());
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels as Arc<dyn rigel_core::InterruptSink>,
    ));
    let tools = Tools::new(
        root.to_path_buf(),
        service,
        budget,
        root.join("index"),
        StopAt::Never,
    )
    .with_command_approval(approver);
    match deliberator {
        Some(deliberator) => tools.with_deliberation(deliberator),
        None => tools,
    }
}

struct FixedDeliberator {
    calls: std::sync::atomic::AtomicUsize,
}

#[async_trait]
impl Deliberator for FixedDeliberator {
    async fn deliberate(
        &self,
        _agent_id: &str,
        command: &str,
        _cwd: &std::path::Path,
        findings: &[Finding],
    ) -> Deliberation {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Deliberation {
            turn_id: Some("deliberation-turn".to_string()),
            positions: vec![Position {
                stance: "opponent".to_string(),
                text: format!(
                    "{} rows matched; this is advisory, not a vote",
                    findings.len()
                ),
                tokens: 10,
                cost_micros: 0,
            }],
            note: format!("advisory on {command}"),
        }
    }
}

async fn run(tools: &Tools, args: serde_json::Value) -> ToolOutcome {
    tools.execute("session", "agent", "run_command", args).await
}

#[tokio::test]
async fn an_approved_command_runs_and_records_its_exit_code() {
    let tmp = tempfile::tempdir().unwrap();
    let approver = Recorder::new(CommandDecision::ALLOW_ONCE);
    let tools = tools(tmp.path(), approver.clone()).await;

    let outcome = run(
        &tools,
        serde_json::json!({ "command": "echo rigel-command-ok" }),
    )
    .await;

    assert!(outcome.ok, "{:?}", outcome.body);
    assert_eq!(outcome.kind, ItemKind::Command);
    assert_eq!(outcome.body["command"], "echo rigel-command-ok");
    assert_eq!(outcome.body["exit_code"], 0);
    assert_eq!(outcome.body["cwd"], ".");
    assert_eq!(outcome.body["timed_out"], false);
    assert!(
        outcome.body["stdout"]
            .as_str()
            .unwrap()
            .contains("rigel-command-ok")
    );

    // What the user approved is exactly what ran: same text, same timeout as reported.
    let seen = approver.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].command, "echo rigel-command-ok");
    assert_eq!(seen[0].cwd, seen[0].project_root);
    assert_eq!(seen[0].timeout_secs, outcome.body["timeout_secs"]);
}

#[tokio::test]
async fn a_denied_command_never_runs() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("untouched.txt"), "still here").unwrap();
    let approver = Recorder::new(CommandDecision::Deny);
    let tools = tools(tmp.path(), approver.clone()).await;

    let outcome = run(
        &tools,
        serde_json::json!({ "command": "echo should-not-run > denied.txt" }),
    )
    .await;

    assert!(!outcome.ok);
    assert_eq!(outcome.kind, ItemKind::Command);
    assert_eq!(outcome.body["error"]["code"], "DENIED");
    assert_eq!(outcome.body["command"], "echo should-not-run > denied.txt");
    assert_eq!(outcome.body["approved"], false);
    assert_eq!(approver.requests().len(), 1);
    assert!(!tmp.path().join("denied.txt").exists());
}

#[tokio::test]
async fn a_destructive_command_records_its_policy_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let approver = Recorder::new(CommandDecision::Deny);
    let tools = tools(tmp.path(), approver.clone()).await;

    let outcome = run(
        &tools,
        serde_json::json!({ "command": "rm -rf ../secrets" }),
    )
    .await;

    assert!(!outcome.ok);
    let rows: Vec<&str> = outcome.body["destructive"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect();
    assert!(rows.contains(&"delete-outside-project"), "{rows:?}");
    assert!(rows.contains(&"recursive-remove"), "{rows:?}");
    // The approver saw the same rows before deciding; the classifier never runs the command.
    assert_eq!(approver.requests()[0].destructive.len(), rows.len());
    // The default deliberator records that advice was unavailable; it cannot change the gate.
    assert_eq!(
        outcome.body["deliberation"]["positions"],
        serde_json::json!([])
    );
}

#[tokio::test]
async fn deliberation_runs_only_for_destructive_commands_and_is_recorded() {
    let tmp = tempfile::tempdir().unwrap();
    let approver = Recorder::new(CommandDecision::Deny);
    let deliberator = Arc::new(FixedDeliberator {
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let tools = tools_with(tmp.path(), approver.clone(), Some(deliberator.clone())).await;

    let clean = run(&tools, serde_json::json!({ "command": "echo tidy" })).await;
    assert_eq!(
        deliberator.calls.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert!(clean.body["deliberation"].is_null());

    let destructive = run(&tools, serde_json::json!({ "command": "git push --force" })).await;
    assert_eq!(
        deliberator.calls.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert!(!destructive.ok, "advice never replaces approval");
    assert_eq!(
        destructive.body["deliberation"]["positions"][0]["stance"],
        "opponent"
    );
    assert_eq!(
        approver.requests()[1]
            .deliberation
            .as_ref()
            .unwrap()
            .positions
            .len(),
        1,
        "the approver sees the round before deciding"
    );
}

#[tokio::test]
async fn a_cwd_outside_the_project_is_refused_before_anyone_is_asked() {
    let tmp = tempfile::tempdir().unwrap();
    let approver = Recorder::new(CommandDecision::ALLOW_ONCE);
    let tools = tools(tmp.path(), approver.clone()).await;

    let outcome = run(
        &tools,
        serde_json::json!({ "command": "echo nope", "cwd": "../" }),
    )
    .await;

    // The patch pipeline refuses `..` as a non-project-relative path before it resolves, so
    // this is BAD_PATH rather than PATH_OUTSIDE; either way it never reaches the approver.
    assert!(!outcome.ok);
    assert!(matches!(
        outcome.body["error"]["code"].as_str(),
        Some("PATH_OUTSIDE") | Some("BAD_PATH")
    ));
    assert!(approver.requests().is_empty());
}

#[tokio::test]
async fn a_command_that_outlives_its_timeout_is_killed() {
    let tmp = tempfile::tempdir().unwrap();
    let approver = Recorder::new(CommandDecision::ALLOW_ONCE);
    let tools = tools(tmp.path(), approver.clone()).await;

    // Both spend about five seconds doing nothing; the tool is told to allow one.
    let command = if cfg!(windows) {
        "ping -n 6 127.0.0.1 >NUL"
    } else {
        "sleep 5"
    };
    let outcome = run(
        &tools,
        serde_json::json!({ "command": command, "timeout_secs": 1 }),
    )
    .await;

    assert!(!outcome.ok);
    assert_eq!(outcome.kind, ItemKind::Command);
    assert_eq!(outcome.body["timed_out"], true);
    assert_eq!(outcome.body["exit_code"], serde_json::Value::Null);
    assert_eq!(outcome.body["timeout_secs"], 1);
}

#[tokio::test]
async fn the_timeout_is_clamped_to_the_documented_maximum() {
    let tmp = tempfile::tempdir().unwrap();
    let approver = Recorder::new(CommandDecision::Deny);
    let tools = tools(tmp.path(), approver.clone()).await;

    let outcome = run(
        &tools,
        serde_json::json!({ "command": "echo hi", "timeout_secs": 100_000 }),
    )
    .await;

    // Denied, so the clamp is observed on the request rather than on the run.
    assert_eq!(outcome.body["error"]["code"], "DENIED");
    assert_eq!(approver.requests()[0].timeout_secs, 600);
}

#[tokio::test]
async fn a_session_grant_answers_the_next_identical_command_without_asking() {
    let tmp = tempfile::tempdir().unwrap();
    let approver = Recorder::new(CommandDecision::Allow(ApprovalScope::Session));
    let tools = tools(tmp.path(), approver.clone())
        .await
        .with_scopes(Scopes::new(tmp.path(), tmp.path().join("index")));

    let first = run(&tools, serde_json::json!({ "command": "echo twice" })).await;
    let second = run(&tools, serde_json::json!({ "command": "echo twice" })).await;

    assert!(first.ok && second.ok, "{:?}", second.body);
    assert_eq!(first.body["scope"], "session");
    assert_eq!(second.body["scope"], "session");
    // The question was asked once: the second run used the grant, not the approver.
    assert_eq!(approver.requests().len(), 1);
}

#[tokio::test]
async fn a_project_grant_survives_a_new_tool_set_and_a_near_miss_still_asks() {
    let tmp = tempfile::tempdir().unwrap();
    let approver = Recorder::new(CommandDecision::Deny);
    let scopes = Scopes::new(tmp.path(), tmp.path().join("index"));
    scopes
        .remember("echo pinned", ApprovalScope::Project)
        .unwrap();

    // A second tool set, as a restarted process would build, reads the same file.
    let tools = tools(tmp.path(), approver.clone())
        .await
        .with_scopes(scopes.clone());
    let granted = run(&tools, serde_json::json!({ "command": "echo pinned" })).await;
    let near_miss = run(&tools, serde_json::json!({ "command": "echo pinned!" })).await;

    assert!(granted.ok, "{:?}", granted.body);
    assert_eq!(granted.body["scope"], "project");
    assert_eq!(near_miss.body["error"]["code"], "DENIED");
    // Only the near miss was asked about; one character is enough to make it a new command.
    assert_eq!(approver.requests().len(), 1);
    assert_eq!(approver.requests()[0].command, "echo pinned!");
}

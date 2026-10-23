//! D80 and D113-D116: a writable child is isolated, approved, capped and reviewed.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use rigel_core::WorktreeService;
use rigel_core::agent::approval::{PlanApprover, PlanDecision, PlanRequest};
use rigel_core::safety::{MergeReviews, ReviewInput};
use rigel_core::{
    BudgetError, BudgetOutcome, BudgetService, Cancels, ClaimService, Coordinator, DecisionError,
    NullSink, PatchPipeline, ScriptedModel, SessionService, StopAt, StopReason, SubagentPolicy,
    Tools, Usage, Verdict, apply_review,
};

struct YesPlans;

#[async_trait]
impl PlanApprover for YesPlans {
    async fn approve_plan(&self, _request: &PlanRequest) -> PlanDecision {
        PlanDecision::Approve
    }
}

struct NoPlans;

#[async_trait]
impl PlanApprover for NoPlans {
    async fn approve_plan(&self, _request: &PlanRequest) -> PlanDecision {
        PlanDecision::Deny
    }
}

fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A committed repository, because a worktree needs a HEAD. The `.rigel` directory is
/// ignored before the first commit, so the session database never shows up as a change.
fn repo() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("README.md"), "# Demo\n").unwrap();
    std::fs::write(tmp.path().join(".gitignore"), ".rigel/\n").unwrap();
    git(tmp.path(), &["init", "-q"]);
    git(tmp.path(), &["add", "-A"]);
    git(
        tmp.path(),
        &[
            "-c",
            "user.name=Rigel",
            "-c",
            "user.email=rigel@example.invalid",
            "commit",
            "-q",
            "-m",
            "init",
        ],
    );
    tmp
}

fn coordinator(
    root: &Path,
    service: &SessionService,
    script: &str,
    plans: Arc<dyn PlanApprover>,
    worktree_dir: Option<PathBuf>,
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
        StopAt::Never,
    )
    .with_plan_approval(plans);
    Coordinator {
        session: service.clone(),
        budget,
        tools,
        model: Arc::new(ScriptedModel::from_json(script).unwrap()),
        sink: Arc::new(NullSink),
        cancels,
        project_root: root.to_path_buf(),
        model_name: "script".to_string(),
        effort: None,
        max_steps: 10,
        subagents: SubagentPolicy::default(),
        depth: 0,
        parent_cancel: None,
        worktree_dir,
    }
}

fn spawn_item<'a>(
    transcript: &'a [rigel_core::TurnTranscript],
    turn_id: &str,
) -> &'a serde_json::Value {
    &transcript
        .iter()
        .find(|entry| entry.turn.id == turn_id)
        .expect("the parent turn is in the transcript")
        .items
        .iter()
        .find(|item| item.body.get("tool").and_then(|tool| tool.as_str()) == Some("spawn_agents"))
        .expect("the spawn call is in the transcript")
        .body["result"]
}

#[tokio::test]
async fn a_writable_child_edits_its_worktree_and_lands_only_through_review() {
    let tmp = repo();
    let data = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("writable"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("boss"))
        .await
        .unwrap();

    let script = r#"{
      "steps": [
        {
          "text": "Spawning a writer.\n",
          "tool_calls": [{ "name": "spawn_agents", "arguments": { "agents": [
            { "role": "writer", "files": ["src/lib.rs", "src/notes.rs"],
              "prompt": "Add a doc comment to add() and a short notes file.",
              "task": "document add", "mode": "write",
              "plan": "1. add one doc comment above add()\n2. change nothing else" }
          ] } }],
          "usage": { "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002 }
        },
        {
          "tool_calls": [
            { "name": "read_file", "arguments": { "path": "src/lib.rs" } },
            { "name": "edit_file", "arguments": {
              "path": "src/lib.rs",
              "old_string": "pub fn add(a: i32, b: i32) -> i32 {",
              "new_string": "/// Adds two numbers.\npub fn add(a: i32, b: i32) -> i32 {" } },
            { "name": "write_file", "arguments": {
              "path": "src/notes.rs",
              "content": "//! Why the add helper is documented.\n" } }
          ],
          "usage": { "input_tokens": 80, "output_tokens": 20, "cost_usd": 0.0001 }
        },
        {
          "text": "Working on: src/lib.rs\nAdded the doc comment.\n",
          "usage": { "input_tokens": 40, "output_tokens": 10, "cost_usd": 0.0001 }
        },
        {
          "text": "The writer is done; merging.\n",
          "tool_calls": [{ "name": "merge_subagent", "arguments": { "role": "writer" } }],
          "usage": { "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002 }
        },
        {
          "text": "APPROVE\nOne doc comment, one file, no behavior change.\n",
          "usage": { "input_tokens": 300, "output_tokens": 20, "cost_usd": 0.0001 }
        },
        {
          "text": "Merged the writer's change; the main tree now has the comment.\n",
          "usage": { "input_tokens": 40, "output_tokens": 10, "cost_usd": 0.0001 }
        }
      ]
    }"#;

    let run = coordinator(
        tmp.path(),
        &service,
        script,
        Arc::new(YesPlans),
        Some(data.path().to_path_buf()),
    )
    .run(&session.id, &agent.id, "document add")
    .await
    .unwrap();

    assert_eq!(run.stop, StopReason::Completed);
    let body = std::fs::read_to_string(tmp.path().join("src/lib.rs")).unwrap();
    assert!(
        body.contains("/// Adds two numbers."),
        "the reviewed change reached the main tree"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("src/notes.rs")).unwrap(),
        "//! Why the add helper is documented.\n",
        "a new file the coordinator assigned is created by the merge"
    );

    let agents = service.agents(&session.id).await.unwrap();
    let child = agents
        .iter()
        .find(|candidate| candidate.role.as_deref() == Some("writer"))
        .expect("the writable child was registered");
    assert!(service.is_writable(&child.id).await.unwrap());
    assert_eq!(
        service.plan_of(&child.id).await.unwrap().as_deref(),
        Some("1. add one doc comment above add()\n2. change nothing else"),
        "the approved plan is recorded next to the agent"
    );

    let transcript = service.transcript(&session.id).await.unwrap();
    let spawn = spawn_item(&transcript, &run.turn.id);
    let returned = spawn["agents"].as_array().unwrap();
    assert_eq!(returned[0]["mode"], "write");
    let mut changed: Vec<&str> = returned[0]["changed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path.as_str().unwrap())
        .collect();
    changed.sort();
    assert_eq!(changed, ["src/lib.rs", "src/notes.rs"]);
    assert_eq!(returned[0]["review_required"], true);
    let worktree = returned[0]["worktree"].as_str().unwrap();
    assert!(
        !Path::new(worktree).exists(),
        "a clean merge removes the worktree it came from"
    );

    let review = MergeReviews::new(service.pool().clone())
        .latest(&child.id)
        .await
        .unwrap()
        .expect("the review is recorded");
    assert_eq!(review.verdict, Verdict::Approved);
    assert!(review.merged);
    let mut files = review.files.clone();
    files.sort();
    assert_eq!(files, ["src/lib.rs", "src/notes.rs"]);

    let claims = ClaimService::new(service.pool().clone(), tmp.path());
    assert!(
        claims.held(None).await.unwrap().is_empty(),
        "the claim is released when the child's turn ends"
    );

    let patches = PatchPipeline::new(tmp.path(), service.pool().clone())
        .list(&session.id)
        .await
        .unwrap();
    assert!(
        patches.iter().any(|patch| patch.agent_id == child.id),
        "the merge is attributed to the child, not the coordinator"
    );
}

#[tokio::test]
async fn a_denied_plan_starts_nothing_and_leaves_no_worktree() {
    let tmp = repo();
    let data = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("denied"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("boss"))
        .await
        .unwrap();

    let script = r#"{
      "steps": [
        {
          "text": "Spawning a writer.\n",
          "tool_calls": [{ "name": "spawn_agents", "arguments": { "agents": [
            { "role": "writer", "files": ["src/lib.rs"], "prompt": "Rewrite add().",
              "mode": "write", "plan": "1. rewrite add()" }
          ] } }],
          "usage": { "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002 }
        },
        { "text": "The plan was refused, so I did not start it.\n",
          "usage": { "input_tokens": 30, "output_tokens": 10, "cost_usd": 0.0001 } }
      ]
    }"#;

    let run = coordinator(
        tmp.path(),
        &service,
        script,
        Arc::new(NoPlans),
        Some(data.path().to_path_buf()),
    )
    .run(&session.id, &agent.id, "rewrite add")
    .await
    .unwrap();
    assert_eq!(run.stop, StopReason::Completed, "the parent turn continues");

    let transcript = service.transcript(&session.id).await.unwrap();
    let spawn = spawn_item(&transcript, &run.turn.id);
    assert_eq!(spawn["ok"], false);
    assert_eq!(spawn["error"]["code"], "PLAN_DENIED");
    assert_eq!(
        service.agents(&session.id).await.unwrap().len(),
        1,
        "the denial happens before any agent is registered"
    );
    let body = std::fs::read_to_string(tmp.path().join("src/lib.rs")).unwrap();
    assert!(body.contains("a + b"), "the file is untouched");
    let worktrees = data.path().join("worktrees");
    assert!(
        !worktrees.exists() || std::fs::read_dir(worktrees).unwrap().next().is_none(),
        "a denial creates no checkout"
    );
}

#[tokio::test]
async fn the_writable_cap_counts_the_whole_fan_out_before_it_starts() {
    let tmp = repo();
    let data = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("cap"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("boss"))
        .await
        .unwrap();
    let budget = BudgetService::with_null_interrupt(service.pool().clone());
    let defaults = budget.safety_settings().await.unwrap();
    assert_eq!(defaults.writable_agents, 3, "D113's default");
    assert_eq!(
        defaults.daily_cap_micros,
        Some(rigel_core::DEFAULT_DAILY_CAP_MICROS),
        "D116's default is on"
    );
    budget.set_writable_agents(1).await.unwrap();

    let script = r#"{
      "steps": [
        {
          "text": "Spawning two writers.\n",
          "tool_calls": [{ "name": "spawn_agents", "arguments": { "agents": [
            { "role": "one", "files": ["src/lib.rs"], "prompt": "One.", "mode": "write", "plan": "1. one" },
            { "role": "two", "files": ["README.md"], "prompt": "Two.", "mode": "write", "plan": "1. two" }
          ] } }],
          "usage": { "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002 }
        },
        { "text": "The cap refused the split.\n",
          "usage": { "input_tokens": 30, "output_tokens": 10, "cost_usd": 0.0001 } }
      ]
    }"#;

    let run = coordinator(
        tmp.path(),
        &service,
        script,
        Arc::new(YesPlans),
        Some(data.path().to_path_buf()),
    )
    .run(&session.id, &agent.id, "split it")
    .await
    .unwrap();
    assert_eq!(run.stop, StopReason::Completed);
    let transcript = service.transcript(&session.id).await.unwrap();
    let spawn = spawn_item(&transcript, &run.turn.id);
    assert_eq!(spawn["error"]["code"], "WRITABLE_LIMIT");
    assert_eq!(service.agents(&session.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_blocked_review_needs_a_recorded_override_to_merge() {
    let tmp = repo();
    let data = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("review"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("boss"))
        .await
        .unwrap();

    let script = r#"{
      "steps": [
        {
          "text": "Spawning a writer.\n",
          "tool_calls": [{ "name": "spawn_agents", "arguments": { "agents": [
            { "role": "writer", "files": ["src/lib.rs"], "prompt": "Add a comment.",
              "mode": "write", "plan": "1. add a comment" }
          ] } }],
          "usage": { "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002 }
        },
        {
          "tool_calls": [
            { "name": "read_file", "arguments": { "path": "src/lib.rs" } },
            { "name": "edit_file", "arguments": {
              "path": "src/lib.rs",
              "old_string": "pub fn add(a: i32, b: i32) -> i32 {",
              "new_string": "/// Adds two numbers.\npub fn add(a: i32, b: i32) -> i32 {" } }
          ],
          "usage": { "input_tokens": 80, "output_tokens": 20, "cost_usd": 0.0001 }
        },
        { "text": "Working on: src/lib.rs\nDone.\n",
          "usage": { "input_tokens": 40, "output_tokens": 10, "cost_usd": 0.0001 } },
        {
          "text": "Trying to merge.\n",
          "tool_calls": [{ "name": "merge_subagent", "arguments": { "role": "writer" } }],
          "usage": { "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002 }
        },
        { "text": "BLOCK\nA new public doc comment with no test of the documented claim.\n",
          "usage": { "input_tokens": 300, "output_tokens": 20, "cost_usd": 0.0001 } },
        {
          "text": "Overriding with a reason.\n",
          "tool_calls": [{ "name": "merge_subagent", "arguments": {
            "role": "writer",
            "override_reason": "the change is one comment and the caller asked for exactly this"
          } }],
          "usage": { "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002 }
        },
        { "text": "BLOCK\nStill only a comment.\n",
          "usage": { "input_tokens": 300, "output_tokens": 20, "cost_usd": 0.0001 } },
        { "text": "Merged under the recorded override.\n",
          "usage": { "input_tokens": 40, "output_tokens": 10, "cost_usd": 0.0001 } }
      ]
    }"#;

    let run = coordinator(
        tmp.path(),
        &service,
        script,
        Arc::new(YesPlans),
        Some(data.path().to_path_buf()),
    )
    .run(&session.id, &agent.id, "add a comment")
    .await
    .unwrap();
    assert_eq!(run.stop, StopReason::Completed);

    let agents = service.agents(&session.id).await.unwrap();
    let child = agents
        .iter()
        .find(|candidate| candidate.role.as_deref() == Some("writer"))
        .expect("the writer exists");
    let reviews = MergeReviews::new(service.pool().clone());
    let review = reviews.latest(&child.id).await.unwrap().unwrap();
    assert_eq!(review.verdict, Verdict::Blocked);
    assert_eq!(
        review.override_note.as_deref(),
        Some("the change is one comment and the caller asked for exactly this"),
        "the override reason is kept beside the verdict"
    );
    assert!(
        review.merged,
        "the second attempt merged under the override"
    );

    // What the Diff view renders for this child: the plan, then the verdict with the reason
    // the reviewer gave and the override the user typed over it.
    let rows = service.writable_agents(&session.id).await.unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.agent.id, child.id);
    assert_eq!(row.plan.as_deref(), Some("1. add a comment"));
    let listed = reviews
        .latest(&row.agent.id)
        .await
        .unwrap()
        .expect("the review is on the row");
    assert_eq!(listed.verdict, Verdict::Blocked);
    assert!(
        listed.merged,
        "the row says the block was overridden, not hidden"
    );
    assert_eq!(
        listed.override_note.as_deref(),
        review.override_note.as_deref()
    );
    assert!(
        row.agent.worktree.is_none(),
        "the checkout is gone once the merge lands; the row says so instead of pretending"
    );

    let body = std::fs::read_to_string(tmp.path().join("src/lib.rs")).unwrap();
    assert!(body.contains("/// Adds two numbers."));
}

#[tokio::test]
async fn a_pending_worktree_diff_reads_without_writing_the_main_tree() {
    let tmp = repo();
    let data = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("pending"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("boss"))
        .await
        .unwrap();

    let script = r#"{
      "steps": [
        {
          "text": "Spawning a writer.\n",
          "tool_calls": [{ "name": "spawn_agents", "arguments": { "agents": [
            { "role": "writer", "files": ["src/lib.rs"], "prompt": "Add a comment.",
              "mode": "write", "plan": "1. add one doc comment above add()" }
          ] } }],
          "usage": { "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002 }
        },
        {
          "tool_calls": [
            { "name": "read_file", "arguments": { "path": "src/lib.rs" } },
            { "name": "edit_file", "arguments": {
              "path": "src/lib.rs",
              "old_string": "pub fn add(a: i32, b: i32) -> i32 {",
              "new_string": "/// Adds two numbers.\npub fn add(a: i32, b: i32) -> i32 {" } }
          ],
          "usage": { "input_tokens": 80, "output_tokens": 20, "cost_usd": 0.0001 }
        },
        { "text": "I left the change in its worktree.\n",
          "usage": { "input_tokens": 40, "output_tokens": 10, "cost_usd": 0.0001 } },
        { "text": "Waiting for the merge decision.\n",
          "usage": { "input_tokens": 40, "output_tokens": 10, "cost_usd": 0.0001 } }
      ]
    }"#;

    let run = coordinator(
        tmp.path(),
        &service,
        script,
        Arc::new(YesPlans),
        Some(data.path().to_path_buf()),
    )
    .run(&session.id, &agent.id, "add a comment")
    .await
    .unwrap();
    assert_eq!(run.stop, StopReason::Completed);

    let rows = service.writable_agents(&session.id).await.unwrap();
    assert_eq!(rows.len(), 1, "the read-only parent is not a writable row");
    let row = &rows[0];
    assert_eq!(row.agent.role.as_deref(), Some("writer"));
    assert_eq!(
        row.plan.as_deref(),
        Some("1. add one doc comment above add()")
    );
    assert!(row.plan_note.is_none());
    assert!(
        MergeReviews::new(service.pool().clone())
            .latest(&row.agent.id)
            .await
            .unwrap()
            .is_none(),
        "nothing was merged, so nothing was reviewed"
    );
    assert!(
        row.agent.worktree.is_some(),
        "the unmerged worktree is where the Diff view reads from"
    );

    let src = WorktreeService::new(tmp.path(), data.path(), service.pool().clone());
    let (diff, files) = src.diff(&row.agent.id).await.unwrap();
    assert_eq!(files, ["src/lib.rs"]);
    assert!(
        diff.contains("+/// Adds two numbers."),
        "the pending diff is readable before any merge:
{diff}"
    );
    let body = std::fs::read_to_string(tmp.path().join("src/lib.rs")).unwrap();
    assert!(
        !body.contains("Adds two numbers"),
        "reading the diff writes nothing into the main tree"
    );
}

#[tokio::test]
async fn the_daily_cap_refuses_the_next_turn_after_the_day_is_spent() {
    let service = SessionService::open(tempfile::TempDir::new().unwrap().path())
        .await
        .unwrap();
    let session = service.start(Some("daily"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let budget = BudgetService::with_null_interrupt(service.pool().clone());
    budget
        .set_daily_cap(&session.id, Some(1_000))
        .await
        .unwrap();
    let turn = service
        .append_turn(&agent.id, Some("spend it"))
        .await
        .unwrap();

    let usage = Usage {
        input_tokens: 10,
        output_tokens: 5,
        cost_micros: 2_000,
        ..Usage::default()
    };
    assert_eq!(
        budget.account(&agent.id, &turn.id, &usage).await.unwrap(),
        BudgetOutcome::DailyExceeded
    );
    assert!(
        matches!(
            budget.reserve_with(&agent.id, 1).await,
            Err(BudgetError::DailyExhausted {
                spend_micros: 2_000,
                cap_micros: 1_000,
                ..
            })
        ),
        "the day's ledger refuses the next turn"
    );
    let status = budget.daily_status().await.unwrap();
    assert_eq!(status.spend_micros, 2_000);
    assert_eq!(status.cap_micros, Some(1_000));
    assert_eq!(status.day, rigel_core::utc_day(rigel_core::now_ms()));
    assert_eq!(rigel_core::utc_day(0), "1970-01-01");
}

#[tokio::test]
async fn a_writable_spawn_without_a_plan_fails_before_any_approval() {
    let tmp = repo();
    let data = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("no plan"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("boss"))
        .await
        .unwrap();
    let script = r#"{
      "steps": [
        {
          "text": "Spawning.\n",
          "tool_calls": [{ "name": "spawn_agents", "arguments": { "agents": [
            { "role": "writer", "files": ["src/lib.rs"], "prompt": "Edit.", "mode": "write" }
          ] } }],
          "usage": { "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002 }
        },
        { "text": "I need a plan first.\n",
          "usage": { "input_tokens": 30, "output_tokens": 10, "cost_usd": 0.0001 } }
      ]
    }"#;
    let run = coordinator(
        tmp.path(),
        &service,
        script,
        Arc::new(YesPlans),
        Some(data.path().to_path_buf()),
    )
    .run(&session.id, &agent.id, "edit")
    .await
    .unwrap();
    let transcript = service.transcript(&session.id).await.unwrap();
    let spawn = spawn_item(&transcript, &run.turn.id);
    assert_eq!(spawn["error"]["code"], "BAD_ARGUMENTS");
    assert_eq!(service.agents(&session.id).await.unwrap().len(), 1);
}

/// A parent turn that spawns one writable child and edits its file, with no merge: the
/// worktree is left for the Diff view to read and decide about.
const PENDING_SCRIPT: &str = r#"{
  "steps": [
    {
      "text": "Spawning a writer.\n",
      "tool_calls": [{ "name": "spawn_agents", "arguments": { "agents": [
        { "role": "writer", "files": ["src/lib.rs"], "prompt": "Add a comment.",
          "mode": "write", "plan": "1. add one doc comment above add()" }
      ] } }],
      "usage": { "input_tokens": 100, "output_tokens": 20, "cost_usd": 0.0002 }
    },
    {
      "tool_calls": [
        { "name": "read_file", "arguments": { "path": "src/lib.rs" } },
        { "name": "edit_file", "arguments": {
          "path": "src/lib.rs",
          "old_string": "pub fn add(a: i32, b: i32) -> i32 {",
          "new_string": "/// Adds two numbers.\npub fn add(a: i32, b: i32) -> i32 {" } }
      ],
      "usage": { "input_tokens": 80, "output_tokens": 20, "cost_usd": 0.0001 }
    },
    { "text": "I left the change in its worktree.\n",
      "usage": { "input_tokens": 40, "output_tokens": 10, "cost_usd": 0.0001 } },
    { "text": "Waiting for the merge decision.\n",
      "usage": { "input_tokens": 40, "output_tokens": 10, "cost_usd": 0.0001 } }
  ]
}"#;

/// One child's pending worktree, plus the services the Diff view would build for it.
struct Pending {
    tmp: tempfile::TempDir,
    data: tempfile::TempDir,
    service: SessionService,
    session: rigel_core::Session,
    child: rigel_core::Agent,
}

/// The notes the window posted into the transcript, in order. A merge decided in the Diff view
/// is posted on the parent turn that spawned the child, where the child's rows already are.
async fn merge_notes(service: &SessionService, session_id: &str) -> Vec<serde_json::Value> {
    service
        .transcript(session_id)
        .await
        .unwrap()
        .into_iter()
        .flat_map(|entry| entry.items)
        .filter(|item| item.kind == "note")
        .map(|item| item.body)
        .collect()
}

async fn pending_worktree() -> Pending {
    let tmp = repo();
    let data = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("decide"), Some(25)).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("boss"))
        .await
        .unwrap();
    let run = coordinator(
        tmp.path(),
        &service,
        PENDING_SCRIPT,
        Arc::new(YesPlans),
        Some(data.path().to_path_buf()),
    )
    .run(&session.id, &agent.id, "add a comment")
    .await
    .unwrap();
    assert_eq!(run.stop, StopReason::Completed);
    let child = service
        .agents(&session.id)
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.role.as_deref() == Some("writer"))
        .expect("the writable child exists");
    Pending {
        tmp,
        data,
        service,
        session,
        child,
    }
}

#[tokio::test]
async fn an_approved_review_is_applied_from_the_view_without_a_model_call() {
    let pending = pending_worktree().await;
    let worktrees = WorktreeService::new(
        pending.tmp.path(),
        pending.data.path(),
        pending.service.pool().clone(),
    );
    let reviews = MergeReviews::new(pending.service.pool().clone());
    let (diff, files) = worktrees.diff(&pending.child.id).await.unwrap();
    reviews
        .record(ReviewInput {
            session_id: &pending.session.id,
            agent_id: &pending.child.id,
            reviewer_model: "openai/gpt-5-nano",
            verdict: Verdict::Approved,
            reason: Some("one doc comment, nothing else moved"),
            diff: &diff,
            files: &files,
            override_note: None,
        })
        .await
        .unwrap();

    let outcome = apply_review(
        &pending.service,
        &worktrees,
        &reviews,
        &pending.session.id,
        &pending.child.id,
        None,
    )
    .await
    .unwrap();

    assert!(outcome.report.clean(), "the approved diff merged");
    assert_eq!(outcome.report.applied, 1);
    assert!(outcome.review.merged);
    assert!(
        outcome.worktree_removed,
        "a clean merge removes the checkout"
    );
    let body = std::fs::read_to_string(pending.tmp.path().join("src/lib.rs")).unwrap();
    assert!(body.contains("/// Adds two numbers."));
    let patches = PatchPipeline::new(pending.tmp.path(), pending.service.pool().clone())
        .list(&pending.session.id)
        .await
        .unwrap();
    assert!(
        patches
            .iter()
            .any(|patch| patch.agent_id == pending.child.id),
        "the merge is attributed to the child, not the window"
    );
    let notes = merge_notes(&pending.service, &pending.session.id).await;
    assert_eq!(notes.len(), 1, "one merge, one note: {notes:?}");
    let text = notes[0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("the user merged writer ")
            && text.contains("1 file landed in the main tree"),
        "the note says what happened: {text}"
    );
    assert_eq!(notes[0]["overridden"], false);
    assert_eq!(notes[0]["review_id"], outcome.review.id);
    assert!(
        apply_review(
            &pending.service,
            &worktrees,
            &reviews,
            &pending.session.id,
            &pending.child.id,
            None
        )
        .await
        .is_err(),
        "the same review cannot be applied twice"
    );
}

#[tokio::test]
async fn a_blocked_review_is_only_crossed_with_a_typed_reason_on_the_diff_it_judged() {
    let pending = pending_worktree().await;
    let worktrees = WorktreeService::new(
        pending.tmp.path(),
        pending.data.path(),
        pending.service.pool().clone(),
    );
    let reviews = MergeReviews::new(pending.service.pool().clone());
    let (diff, files) = worktrees.diff(&pending.child.id).await.unwrap();
    let blocked = reviews
        .record(ReviewInput {
            session_id: &pending.session.id,
            agent_id: &pending.child.id,
            reviewer_model: "openai/gpt-5-nano",
            verdict: Verdict::Blocked,
            reason: Some("a public doc comment with no test of the documented claim"),
            diff: &diff,
            files: &files,
            override_note: None,
        })
        .await
        .unwrap();

    let refused = apply_review(
        &pending.service,
        &worktrees,
        &reviews,
        &pending.session.id,
        &pending.child.id,
        None,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(refused, DecisionError::Refused(_)),
        "a block is not crossed without a reason: {refused}"
    );
    let body = std::fs::read_to_string(pending.tmp.path().join("src/lib.rs")).unwrap();
    assert!(
        !body.contains("Adds two numbers"),
        "the refusal wrote nothing"
    );

    // The worktree moves after the verdict: the stored review no longer describes these bytes,
    // so even a typed reason cannot apply it.
    let worktree = pending
        .child
        .worktree
        .as_ref()
        .expect("the checkout exists");
    let path = Path::new(worktree).join("src/lib.rs");
    let mut moved = std::fs::read_to_string(&path).unwrap();
    moved.push_str("\n// touched after the review\n");
    std::fs::write(&path, moved).unwrap();
    let stale = apply_review(
        &pending.service,
        &worktrees,
        &reviews,
        &pending.session.id,
        &pending.child.id,
        Some("the missing test is not my concern"),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&stale, DecisionError::Refused(message) if message.contains("changed after this review")),
        "a stale verdict is refused: {stale}"
    );

    let (diff, files) = worktrees.diff(&pending.child.id).await.unwrap();
    reviews
        .record(ReviewInput {
            session_id: &pending.session.id,
            agent_id: &pending.child.id,
            reviewer_model: "openai/gpt-5-nano",
            verdict: Verdict::Blocked,
            reason: Some("the trailing comment is unexplained"),
            diff: &diff,
            files: &files,
            override_note: None,
        })
        .await
        .unwrap();
    let outcome = apply_review(
        &pending.service,
        &worktrees,
        &reviews,
        &pending.session.id,
        &pending.child.id,
        Some("  the caller asked for exactly this comment  "),
    )
    .await
    .unwrap();

    assert!(outcome.report.clean());
    assert!(outcome.review.merged);
    assert_eq!(
        outcome.review.verdict,
        Verdict::Blocked,
        "the verdict is not rewritten"
    );
    assert_eq!(
        outcome.review.override_note.as_deref(),
        Some("the caller asked for exactly this comment"),
        "the typed reason is stored beside the verdict, trimmed"
    );
    let stored = reviews.latest(&pending.child.id).await.unwrap().unwrap();
    assert_eq!(stored.override_note, outcome.review.override_note);
    assert!(stored.merged);
    assert_ne!(
        stored.id, blocked.id,
        "the override answers the review that was current"
    );
    let body = std::fs::read_to_string(pending.tmp.path().join("src/lib.rs")).unwrap();
    assert!(body.contains("Adds two numbers"));
    let notes = merge_notes(&pending.service, &pending.session.id).await;
    assert_eq!(
        notes.len(),
        1,
        "the refusals and the stale review posted nothing; only the merge did"
    );
    let text = notes[0]["text"].as_str().unwrap();
    assert!(
        text.contains("overriding a blocked review")
            && text.contains("the caller asked for exactly this comment"),
        "the note carries the user's reason: {text}"
    );
    assert_eq!(notes[0]["overridden"], true);
    assert_eq!(notes[0]["agent_id"], pending.child.id);
}

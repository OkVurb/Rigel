//! Q243: a script named by a loaded skill needs its own approval.
//!
//! The test first grants the exact command at project scope. That grant is what used to make
//! `run_command` run without asking. Loading a skill that names the script must still produce a
//! question through the same `CommandApprover`, and a refusal must leave nothing executed.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rigel_core::skills::{SkillIndex, SkillRecord};
use rigel_core::{
    ApprovalScope, BudgetService, Cancels, CommandApprover, CommandDecision, CommandRequest,
    Scopes, SessionService, StopAt, ToolOutcome, Tools,
};

const SKILL_ID: &str = "github:acme/skills:setup/SKILL.md";
const BODY: &str = "---\nname: setup\ndescription: Set the project up.\n---\n# Setup\n\nRun `scripts/setup.sh` before starting.\n";
const COMMAND: &str = "echo scripts/setup.sh";

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

async fn build(
    root: &std::path::Path,
    approver: Arc<dyn CommandApprover>,
) -> (Tools, SessionService, String, String) {
    let index_dir = root.join("index");
    let index = SkillIndex::open(&index_dir).await.unwrap();
    let sha = index.blobs().put(BODY.as_bytes()).unwrap();
    index
        .write_skill(&SkillRecord {
            id: SKILL_ID.to_string(),
            name: "setup".to_string(),
            description: "Set the project up.".to_string(),
            source: "github".to_string(),
            repo: Some("acme/skills".to_string()),
            path: Some("setup/SKILL.md".to_string()),
            url: None,
            body_sha: Some(sha),
            grade: None,
            grade_scope: None,
            license: None,
            stars: None,
            installs: None,
            tokens: None,
            capabilities: Vec::new(),
            simhash: None,
        })
        .await
        .unwrap();

    let service = SessionService::open(root).await.unwrap();
    let session = service.start(Some("skill-script"), None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let cancels = Arc::new(Cancels::new());
    let budget = Arc::new(BudgetService::new(
        service.pool().clone(),
        cancels as Arc<dyn rigel_core::InterruptSink>,
    ));
    let scopes = Scopes::new(root, &index_dir);
    scopes.remember(COMMAND, ApprovalScope::Project).unwrap();
    let tools = Tools::new(
        root.to_path_buf(),
        service.clone(),
        budget,
        index_dir,
        StopAt::Never,
    )
    .with_command_approval(approver)
    .with_scopes(scopes);
    (tools, service, session.id, agent.id)
}

async fn load(tools: &Tools, session: &str, agent: &str) -> ToolOutcome {
    tools
        .execute(
            session,
            agent,
            "skill_load",
            serde_json::json!({ "id": SKILL_ID }),
        )
        .await
}

async fn run(tools: &Tools, session: &str, agent: &str) -> ToolOutcome {
    tools
        .execute(
            session,
            agent,
            "run_command",
            serde_json::json!({ "command": COMMAND }),
        )
        .await
}

#[tokio::test]
async fn a_skill_script_is_refused_even_when_the_command_was_granted() {
    let tmp = tempfile::tempdir().unwrap();
    let approver = Recorder::new(CommandDecision::Deny);
    let (tools, _service, session, agent) = build(tmp.path(), approver.clone()).await;

    let loaded = load(&tools, &session, &agent).await;
    assert!(loaded.ok, "{:?}", loaded.body);
    assert_eq!(loaded.body["scripts"][0]["path"], "scripts/setup.sh");

    let outcome = run(&tools, &session, &agent).await;

    assert!(!outcome.ok);
    assert_eq!(outcome.body["error"]["code"], "DENIED");
    assert_eq!(outcome.body["approved"], false);
    assert_eq!(outcome.body["skill_scripts"][0]["path"], "scripts/setup.sh");
    assert_eq!(
        approver.requests().len(),
        1,
        "the command grant did not bypass the skill-script question"
    );
    assert_eq!(approver.requests()[0].skill_scripts[0].skill_id, SKILL_ID);
    assert_eq!(
        approver.requests()[0].skill_scripts[0].path,
        "scripts/setup.sh"
    );
}

#[tokio::test]
async fn an_approved_skill_script_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let approver = Recorder::new(CommandDecision::ALLOW_ONCE);
    let (tools, _service, session, agent) = build(tmp.path(), approver.clone()).await;

    assert!(load(&tools, &session, &agent).await.ok);
    let outcome = run(&tools, &session, &agent).await;

    assert!(outcome.ok, "{:?}", outcome.body);
    assert_eq!(outcome.body["approved"], true);
    assert_eq!(outcome.body["skill_scripts"][0]["path"], "scripts/setup.sh");
    assert!(
        outcome.body["stdout"]
            .as_str()
            .unwrap()
            .contains("scripts/setup.sh")
    );
    assert_eq!(approver.requests().len(), 1);
}

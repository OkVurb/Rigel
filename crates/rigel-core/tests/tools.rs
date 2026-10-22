//! Phase 6's guarantees, exercised without touching the real machine.
//!
//! Detection, the ledger and the queue are all decided by [`ToolService`], so the test injects
//! an [`Environment`] (files and managers, no processes) and a [`Runner`] (records the command,
//! returns an exit code). The one thing that is *not* faked is the real probe at the bottom of
//! this file: if `cargo` cannot be found by the production code path, the build itself is
//! lying about this machine.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rigel_core::{
    Environment, Host, InstallPlan, Manager, Pin, RunOutcome, Runner, SystemEnvironment, ToolError,
    ToolService, TrustReport, trust_for,
};
use tempfile::TempDir;

/* ------------------------------------------------------------------ *
 * fakes
 * ------------------------------------------------------------------ */

struct FakeEnvironment {
    entries: Vec<PathBuf>,
    versions: HashMap<PathBuf, String>,
    managers: Vec<Manager>,
    version_calls: AtomicUsize,
}

impl FakeEnvironment {
    fn new(entries: Vec<PathBuf>, versions: Vec<(PathBuf, &str)>) -> Self {
        FakeEnvironment {
            entries,
            versions: versions
                .into_iter()
                .map(|(path, v)| (path, v.to_string()))
                .collect(),
            managers: Vec::new(),
            version_calls: AtomicUsize::new(0),
        }
    }

    fn with_managers(mut self, managers: Vec<Manager>) -> Self {
        self.managers = managers;
        self
    }
}

#[async_trait]
impl Environment for FakeEnvironment {
    fn path_entries(&self) -> Vec<PathBuf> {
        self.entries.clone()
    }

    async fn version(&self, exe: &std::path::Path, _args: &[&str]) -> Option<String> {
        self.version_calls.fetch_add(1, Ordering::SeqCst);
        self.versions.get(exe).cloned()
    }

    fn sha256(&self, exe: &std::path::Path) -> Option<String> {
        // Read the real file, so a test can replace a binary and watch the pin notice.
        use sha2::{Digest, Sha256};
        let bytes = std::fs::read(exe).ok()?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        Some(format!("{:x}", hasher.finalize()))
    }

    fn manager_available(&self, manager: Manager) -> bool {
        self.managers.contains(&manager)
    }
}

#[derive(Default)]
struct RecordingRunner {
    calls: Mutex<Vec<String>>,
    exit_code: i32,
    spawned: bool,
    stderr: String,
}

impl RecordingRunner {
    fn ok() -> Self {
        RecordingRunner {
            calls: Mutex::new(Vec::new()),
            exit_code: 0,
            spawned: true,
            stderr: String::new(),
        }
    }

    fn failing(code: i32, stderr: &str) -> Self {
        RecordingRunner {
            calls: Mutex::new(Vec::new()),
            exit_code: code,
            spawned: true,
            stderr: stderr.to_string(),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl Runner for RecordingRunner {
    async fn run(&self, command: &str, _cwd: &std::path::Path, _timeout: Duration) -> RunOutcome {
        self.calls.lock().unwrap().push(command.to_string());
        RunOutcome {
            spawned: self.spawned,
            exit_code: if self.spawned {
                Some(self.exit_code)
            } else {
                None
            },
            stdout: String::new(),
            stderr: self.stderr.clone(),
            duration_ms: 42,
        }
    }
}

fn binary_name(binary: &str) -> String {
    #[cfg(windows)]
    {
        format!("{binary}.exe")
    }
    #[cfg(not(windows))]
    {
        binary.to_string()
    }
}

/// A data directory with an index database and a fake bin directory holding `rg`.
fn fixture() -> (TempDir, TempDir, FakeEnvironment) {
    let data = TempDir::new().unwrap();
    let bin = TempDir::new().unwrap();
    let rg = bin.path().join(binary_name("rg"));
    std::fs::write(&rg, b"ripgrep binary").unwrap();
    let env = FakeEnvironment::new(vec![bin.path().to_path_buf()], vec![(rg, "ripgrep 14.1.0")])
        .with_managers(vec![Manager::Winget]);
    (data, bin, env)
}

async fn service(data: &TempDir, env: FakeEnvironment) -> ToolService {
    ToolService::open(data.path())
        .await
        .expect("index opens")
        .with_environment(Arc::new(env))
}

/* ------------------------------------------------------------------ *
 * detection and the cache
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn detection_records_presence_and_reuses_a_fresh_row() {
    let (data, _bin, env) = fixture();
    let env = Arc::new(env);
    let service = ToolService::open(data.path())
        .await
        .unwrap()
        .with_environment(env.clone());

    let first = service.detect(&Host::Windows, false).await.unwrap();
    let rg = first.iter().find(|row| row.id == "ripgrep").unwrap();
    assert!(rg.present);
    assert_eq!(rg.version.as_deref(), Some("ripgrep 14.1.0"));
    assert_eq!(
        rg.path.as_ref().unwrap().file_name().unwrap(),
        binary_name("rg").as_str()
    );
    assert!(rg.sha256.is_some(), "detection records the binary's hash");
    let jq = first.iter().find(|row| row.id == "jq").unwrap();
    assert!(!jq.present);

    let probes = env.version_calls.load(Ordering::SeqCst);
    assert_eq!(probes, 1, "only the present tool is version-probed");

    // A second call inside the six-hour window does not spawn the tool again.
    let second = service.detect(&Host::Windows, false).await.unwrap();
    assert_eq!(
        env.version_calls.load(Ordering::SeqCst),
        probes,
        "the cached version was reused"
    );
    assert_eq!(second.len(), first.len());

    // `--refresh` does probe again.
    service.detect(&Host::Windows, true).await.unwrap();
    assert_eq!(env.version_calls.load(Ordering::SeqCst), probes + 1);
}

#[tokio::test]
async fn the_cache_is_per_host() {
    let (data, _bin, env) = fixture();
    let service = ToolService::open(data.path())
        .await
        .unwrap()
        .with_environment(Arc::new(env));

    service.detect(&Host::Windows, false).await.unwrap();
    let wsl = service
        .cached(&Host::Wsl("Ubuntu-24.04".into()))
        .await
        .unwrap();
    assert!(
        wsl.is_empty(),
        "a tool found on Windows is not installed in WSL"
    );

    service
        .detect(&Host::Wsl("Ubuntu-24.04".into()), false)
        .await
        .unwrap();
    let rows = service
        .cached(&Host::Wsl("Ubuntu-24.04".into()))
        .await
        .unwrap();
    assert!(rows.iter().any(|row| row.id == "ripgrep" && row.present));
}

#[tokio::test]
async fn the_real_probe_finds_the_toolchain_that_built_this_test() {
    let data = TempDir::new().unwrap();
    let service = ToolService::open(data.path()).await.unwrap();
    let host = Host::current();
    let rows = service.detect(&host, true).await.unwrap();
    let cargo = rows.iter().find(|row| row.id == "rust").expect("rust row");
    assert!(
        cargo.present,
        "cargo built these tests and must be on PATH: {:?}",
        cargo
    );
    assert!(
        cargo
            .version
            .as_deref()
            .is_some_and(|v| v.contains("cargo")),
        "unexpected version: {:?}",
        cargo.version
    );
    assert!(cargo.path.is_some());
    assert_eq!(
        SystemEnvironment::locate(
            "definitely-not-a-binary",
            &service.environment().path_entries()
        ),
        None
    );
}

/* ------------------------------------------------------------------ *
 * plans, installs, the ledger and the queue
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn the_plan_shows_the_command_that_will_run() {
    let (data, _bin, env) = fixture();
    let service = service(&data, env).await;
    let plan = service.plan("ripgrep", &Host::Windows).await.unwrap();
    assert_eq!(plan.manager, Manager::Winget);
    assert_eq!(plan.package, "BurntSushi.ripgrep.MSVC");
    assert_eq!(
        plan.command,
        "winget install --id BurntSushi.ripgrep.MSVC -e --accept-source-agreements --accept-package-agreements"
    );
    assert!(plan.trust.score >= 75.0, "{:?}", plan.trust);
    assert!(plan.verification.contains("SHA-256"));
    assert_eq!(plan.already_present.as_deref(), Some("ripgrep 14.1.0"));
}

#[tokio::test]
async fn a_queued_install_never_runs_and_says_so() {
    let (data, _bin, env) = fixture();
    let service = service(&data, env).await;
    let plan = service.plan("ripgrep", &Host::Windows).await.unwrap();
    let runner = Arc::new(RecordingRunner::ok());

    let record = service
        .install(&plan, runner.clone(), &Host::Windows, true, false)
        .await
        .unwrap();
    assert_eq!(record.decision, "queued");
    assert!(record.executed_at.is_none());
    assert!(runner.calls().is_empty(), "unattended mode ran something");
    assert!(record.recheck().contains("queued"));

    let queued = service.queued().await.unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].command, plan.command);
}

#[tokio::test]
async fn an_install_records_exit_code_and_duration() {
    let (data, _bin, env) = fixture();
    let service = service(&data, env).await;
    let plan = service.plan("ripgrep", &Host::Windows).await.unwrap();
    let runner = Arc::new(RecordingRunner::ok());

    let record = service
        .install(&plan, runner.clone(), &Host::Windows, false, false)
        .await
        .unwrap();
    assert_eq!(record.decision, "user-approved");
    assert_eq!(record.exit_code, Some(0));
    assert_eq!(record.duration_ms, Some(42));
    assert!(record.executed_at.is_some());
    assert_eq!(runner.calls(), vec![plan.command.clone()]);
    assert!(record.recheck().contains("rigel tools which"));

    let ledger = service.ledger(10).await.unwrap();
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger[0].id, record.id);
}

#[tokio::test]
async fn a_failed_install_keeps_the_reason_and_does_not_lie_about_success() {
    let (data, _bin, env) = fixture();
    let service = service(&data, env).await;
    let plan = service.plan("ripgrep", &Host::Windows).await.unwrap();
    let runner = Arc::new(RecordingRunner::failing(1, "no matching package found"));

    let record = service
        .install(&plan, runner, &Host::Windows, false, false)
        .await
        .unwrap();
    assert_eq!(record.exit_code, Some(1));
    assert_eq!(record.error.as_deref(), Some("no matching package found"));
    assert!(!record.recheck().contains("is installed"));
}

#[tokio::test]
async fn a_blocked_score_is_refused_and_leaves_a_denied_row() {
    let (data, _bin, env) = fixture();
    let service = service(&data, env).await;
    let mut plan = service.plan("ripgrep", &Host::Windows).await.unwrap();
    plan.trust = trust_for("left-pad-9000", &Host::Windows);
    assert_eq!(plan.trust.band, "blocked");

    let runner = Arc::new(RecordingRunner::ok());
    let error = service
        .install(&plan, runner.clone(), &Host::Windows, false, false)
        .await
        .expect_err("a blocked package is refused");
    assert!(matches!(error, ToolError::Blocked { .. }), "{error}");
    assert!(runner.calls().is_empty());

    let ledger = service.ledger(5).await.unwrap();
    assert_eq!(ledger[0].decision, "denied");
    assert!(ledger[0].error.as_deref().unwrap().contains("trust"));

    // The override exists, is explicit, and runs the same shown command.
    let runner = Arc::new(RecordingRunner::ok());
    let record = service
        .install(&plan, runner.clone(), &Host::Windows, false, true)
        .await
        .unwrap();
    assert_eq!(record.decision, "user-approved");
    assert_eq!(runner.calls().len(), 1);
}

#[tokio::test]
async fn an_apt_install_is_shown_and_refused_because_rigel_never_runs_sudo() {
    let data = TempDir::new().unwrap();
    let bin = TempDir::new().unwrap();
    let env = FakeEnvironment::new(vec![bin.path().to_path_buf()], Vec::new())
        .with_managers(vec![Manager::Apt]);
    let service = service(&data, env).await;

    let plan = service.plan("ripgrep", &Host::Linux).await.unwrap();
    assert!(plan.requires_root);
    assert_eq!(plan.command, "apt-get install -y ripgrep");

    let runner = Arc::new(RecordingRunner::ok());
    let error = service
        .install(&plan, runner.clone(), &Host::Linux, false, false)
        .await
        .expect_err("apt needs root");
    assert!(matches!(error, ToolError::NeedsRoot { .. }), "{error}");
    assert!(runner.calls().is_empty());
    assert!(error.to_string().contains("never runs sudo"));
}

#[tokio::test]
async fn a_tool_changed_underneath_the_pin_is_reported_once() {
    let (data, bin, env) = fixture();
    let rg = bin.path().join(binary_name("rg"));
    let service = service(&data, env).await;
    service.detect(&Host::Windows, false).await.unwrap();
    assert!(matches!(
        service.verify("ripgrep", &Host::Windows).await.unwrap(),
        Pin::Pinned { .. }
    ));

    // Same path, different bytes: a replaced binary.
    std::fs::write(&rg, b"a different ripgrep").unwrap();
    match service.verify("ripgrep", &Host::Windows).await.unwrap() {
        Pin::Changed { from, to, .. } => assert_ne!(from, to),
        other => panic!("expected a change, got {other:?}"),
    }
    assert!(
        matches!(
            service.verify("ripgrep", &Host::Windows).await.unwrap(),
            Pin::Pinned { .. }
        ),
        "the new hash becomes the pin"
    );
}

#[tokio::test]
async fn a_missing_manager_is_a_clear_refusal() {
    let data = TempDir::new().unwrap();
    let bin = TempDir::new().unwrap();
    let env = FakeEnvironment::new(vec![bin.path().to_path_buf()], Vec::new());
    let service = service(&data, env).await;
    let error = service
        .plan("ripgrep", &Host::Windows)
        .await
        .expect_err("no manager");
    assert!(matches!(error, ToolError::NoManager(_)), "{error}");

    let error = service
        .plan("not-a-real-tool", &Host::Windows)
        .await
        .expect_err("unknown tool");
    assert!(matches!(error, ToolError::UnknownTool(_)), "{error}");
}

#[tokio::test]
async fn requirements_name_the_line_and_the_registry_resolves_aliases() {
    let src = "---\nname: audit\nallowed-tools: Bash(rg:*), Read\n---\n\n```sh\njq '.items[]' report.json\n```\n";
    let found = rigel_core::requirements(src);
    let rg = found.iter().find(|row| row.tool == "ripgrep").unwrap();
    assert_eq!(rg.source.line, 3);
    assert_eq!(rg.source.kind, rigel_core::SourceKind::AllowedTools);
    let jq = found.iter().find(|row| row.tool == "jq").unwrap();
    assert_eq!(jq.source.line, 7);
    assert!(jq.source.text.contains("report.json"));

    assert!(rigel_core::tool_by_alias("Bash(node:*)").is_some());
    assert!(rigel_core::tool_by_alias("nonsense").is_none());
}

#[test]
fn the_explanation_sums_to_the_score() {
    let report: TrustReport = trust_for("ripgrep", &Host::Windows);
    let total: f64 = report.factors.iter().map(|factor| factor.points).sum();
    assert!((total - report.score).abs() < 1e-9);
    let _: &InstallPlan = &InstallPlan {
        tool: String::new(),
        name: String::new(),
        host: String::new(),
        manager: Manager::Winget,
        package: String::new(),
        command: String::new(),
        trust: report,
        verification: Manager::Winget.verification(),
        network: true,
        requires_root: false,
        already_present: None,
    };
}

//! The six guarantees from `docs/handoff.md` §12.
//!
//! Each test states the guarantee it pins. The last two used to be `#[ignore]`d
//! specifications; they are real checks now: the replay's fixture coverage (P4.7) and the
//! public-copy scan for a lock claim (P5.8).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rigel_core::budget::{BudgetOutcome, BudgetService, InterruptSink};
use rigel_core::memory::{MemoryError, MemoryService, WriteMode};
use rigel_core::patches::{PatchError, PatchPipeline, RecoveryState, StopAt};
use rigel_core::{SessionService, Usage};

/// Phase 3, P3.5 — a patch must never apply over a file that changed underneath it.
#[tokio::test]
async fn patch_apply_is_cas() {
    let tmp = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("cas"), None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();

    let path = tmp.path().join("answer.txt");
    std::fs::write(&path, "one\n").unwrap();

    let pipeline = PatchPipeline::new(tmp.path(), service.pool().clone());
    let (_, base) = pipeline.read("answer.txt").unwrap();

    // Someone else writes between the read and the proposal.
    std::fs::write(&path, "someone else\n").unwrap();

    let error = pipeline
        .propose(&session.id, &agent.id, "answer.txt", &base, "two\n")
        .await
        .unwrap_err();
    match error {
        PatchError::StaleBase { expected, current } => {
            assert_eq!(expected, base);
            assert_ne!(current, base);
        }
        other => panic!("expected STALE_BASE, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "someone else\n",
        "a stale patch writes nothing"
    );
    assert!(
        pipeline.list(&session.id).await.unwrap().is_empty(),
        "a stale patch is not even recorded"
    );

    // Re-read, then the same edit applies exactly once.
    let (_, fresh) = pipeline.read("answer.txt").unwrap();
    let patch = pipeline
        .propose(&session.id, &agent.id, "answer.txt", &fresh, "two\n")
        .await
        .unwrap();
    assert_eq!(patch.status, "applied");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n");

    let reverted = pipeline.revert(&patch.id).await.unwrap();
    assert_eq!(reverted.status, "reverted");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "someone else\n");
}

/// Phase 3, P3.11 — a crash between journalling and renaming must not double-apply.
#[tokio::test]
async fn resume_never_duplicates_patches() {
    let tmp = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("crash"), None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let path = tmp.path().join("src.txt");
    std::fs::write(&path, "alpha\nbeta\n").unwrap();

    let pipeline = PatchPipeline::new(tmp.path(), service.pool().clone());

    // Crash window A: row and journal are durable, the rename never happened.
    let (_, base) = pipeline.read("src.txt").unwrap();
    let error = pipeline
        .propose_with(
            &session.id,
            &agent.id,
            "src.txt",
            &base,
            "alpha\nBETA\n",
            StopAt::AfterJournal,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, PatchError::Stopped(StopAt::AfterJournal)));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "alpha\nbeta\n");

    let recovered = pipeline.recover(&session.id).await.unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].state, RecoveryState::Pending);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "alpha\nbeta\n",
        "recovery never applies a pending patch"
    );

    let patches = pipeline.list(&session.id).await.unwrap();
    assert_eq!(patches.len(), 1, "one row, not one per attempt");
    assert_eq!(patches[0].status, "proposed");

    // The user (or the next run) applies it; then it is applied exactly once.
    let applied = pipeline.apply(&patches[0].id).await.unwrap();
    assert_eq!(applied.status, "applied");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "alpha\nBETA\n");
    assert!(pipeline.recover(&session.id).await.unwrap().is_empty());
    assert_eq!(pipeline.list(&session.id).await.unwrap().len(), 1);

    // Crash window B: the rename happened, the row still says proposed.
    std::fs::write(&path, "alpha\ngamma\n").unwrap();
    let (_, base) = pipeline.read("src.txt").unwrap();
    let error = pipeline
        .propose_with(
            &session.id,
            &agent.id,
            "src.txt",
            &base,
            "alpha\nGAMMA\n",
            StopAt::AfterRename,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, PatchError::Stopped(StopAt::AfterRename)));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "alpha\nGAMMA\n");

    let recovered = pipeline.recover(&session.id).await.unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].state, RecoveryState::Applied);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "alpha\nGAMMA\n",
        "recovery marks the row, it does not write again"
    );
    assert!(pipeline.recover(&session.id).await.unwrap().is_empty());
    let patches = pipeline.list(&session.id).await.unwrap();
    assert_eq!(patches.len(), 2);
    assert_eq!(
        patches.iter().filter(|p| p.status == "applied").count(),
        2,
        "each patch applied once, neither twice"
    );

    // No temp files were left beside the target.
    let strays: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().contains(".rigel-"))
        .collect();
    assert!(strays.is_empty(), "orphan temp files: {strays:?}");
}

/// Phase 5, P5.8 — N1 as an executable check: public copy never claims a lock.
///
/// The scan covers what a user reads: the front-door documents, the renderer's strings and
/// the MCP tool descriptions. The design record (`handoff.md`, the interview answers and the
/// grilling/research notes) states the prohibition itself, so it is out of scope by
/// construction. A hit is allowed only on a line that also states the prohibition, and
/// cargo's `--locked` is a flag, not a claim.
#[test]
fn claim_never_reports_locked() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec![
        root.join("README.md"),
        root.join("CONTRIBUTING.md"),
        root.join("SECURITY.md"),
        root.join("ARCHITECTURE.md"),
        root.join("docs/PLATFORMS.md"),
        root.join("docs/what-works.md"),
        root.join("docs/migrating-prototype-index.md"),
        root.join("docs/design-prompt.md"),
        root.join("docs/skill-intelligence.md"),
    ];
    for dir in ["ui/src", "crates/rigel-mcp/src"] {
        collect_sources(&root.join(dir), &["ts", "tsx", "rs"], &mut files);
    }
    assert!(
        files.len() >= 15,
        "the scan found only {} files; the roots moved",
        files.len()
    );

    let mut hits = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        // A Rust test module asserts about the word itself; it is not product copy. The MCP
        // crate already checks its advertised descriptions the same way at runtime.
        let text = text.split("#[cfg(test)]").next().unwrap_or(&text);
        for (index, line) in text.lines().enumerate() {
            if claims_lock(line) && !states_prohibition(line) {
                hits.push(format!("{}:{}: {}", path.display(), index + 1, line.trim()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "public copy claims a lock (N1); say 'claimed by X, unverified':
{}",
        hits.join(
            "
"
        )
    );
}

/// `locked` as a word — not `blocked` or `unlocked` — and not cargo's `--locked`.
fn claims_lock(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.match_indices("locked").any(|(index, _)| {
        let before = lower[..index].chars().next_back();
        let after = lower[index + "locked".len()..].chars().next();
        before.is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '-')
            && after.is_none_or(|c| !c.is_alphanumeric() && c != '_')
    })
}

fn states_prohibition(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    ["never", "not ", "no ", "cannot", "without"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn collect_sources(dir: &Path, extensions: &[&str], into: &mut Vec<PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_sources(&path, extensions, into);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| extensions.contains(&ext))
        {
            into.push(path);
        }
    }
}

struct CountingInterrupt(AtomicUsize);

impl InterruptSink for CountingInterrupt {
    fn interrupt(&self, _agent_id: &str, _reason: &str) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Phase 3, P3.8 — the kill switch stops work within one turn.
#[tokio::test]
async fn budget_stops_within_one_turn() {
    let tmp = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service
        .start(Some("budget"), Some(25)) // $0.25
        .await
        .unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    service
        .set_agent_status(&agent.id, "running")
        .await
        .unwrap();
    let turn = service.append_turn(&agent.id, Some("go")).await.unwrap();

    let counter = Arc::new(CountingInterrupt(AtomicUsize::new(0)));
    let budget = BudgetService::new(service.pool().clone(), counter.clone());

    // $0.31 in one turn: over the cap, and the turn is charged in full.
    let usage = Usage {
        input_tokens: 20_000,
        output_tokens: 1_000,
        cost_micros: 310_000,
        provider_micros: Some(310_000),
        ..Usage::default()
    };
    let outcome = budget.account(&agent.id, &turn.id, &usage).await.unwrap();
    assert_eq!(outcome, BudgetOutcome::Exceeded);
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        1,
        "exactly one interrupt per session, not one per call"
    );

    // A second accounting call does not interrupt again.
    let usage = Usage {
        cost_micros: 10_000,
        ..Usage::default()
    };
    assert_eq!(
        budget.account(&agent.id, &turn.id, &usage).await.unwrap(),
        BudgetOutcome::Exceeded
    );
    assert_eq!(counter.0.load(Ordering::SeqCst), 1);

    // The next turn is refused before it makes a request.
    let refused = budget.reserve(&agent.id).await;
    assert!(matches!(
        refused,
        Err(rigel_core::BudgetError::Exhausted { .. })
    ));

    let stored = service.get(&session.id).await.unwrap();
    assert_eq!(stored.spend_micros, 320_000);
    assert_eq!(stored.spend_cents, 32);
    assert!(budget.reconcile(&session.id).await.unwrap().ok);

    // Raising the cap is a deliberate act and resets the kill switch.
    budget.raise_cap(&session.id, 100).await.unwrap();
    assert!(budget.reserve(&agent.id).await.is_ok());
    let stored = service.get(&session.id).await.unwrap();
    assert_eq!(
        stored.interventions, 1,
        "raising the cap is an intervention"
    );
}

/// Phase 3, P3.6 — memory is committed, so it must not carry machine paths or secrets.
#[tokio::test]
async fn memory_rejects_paths() {
    let tmp = tempfile::TempDir::new().unwrap();
    let service = SessionService::open(tmp.path()).await.unwrap();
    let memory = MemoryService::new(tmp.path(), service.pool().clone());

    let attempts = [
        "root: C:\\Users\\someone\\project",
        "home: /home/someone/x",
        "users: /Users/someone/x",
        "key sk-live-abcdefgh",
        "ssh-rsa AAAAB3NzaC1yc2E",
        "OPENROUTER_API_KEY=abcdefgh12345678",
    ];
    for content in attempts {
        let error = memory
            .write("agent-1", "memory", content, WriteMode::Replace)
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                MemoryError::PathLeak { .. } | MemoryError::Secret { .. }
            ),
            "{content:?} produced {error:?}"
        );
    }
    assert!(
        memory.read("agent-1", "memory").unwrap().is_none(),
        "a rejected write creates no file"
    );

    let written = memory
        .write(
            "agent-1",
            "memory",
            "The index lives in the app data directory.\n",
            WriteMode::Append,
        )
        .await
        .unwrap();
    assert!(written.path.starts_with(".rigel/memory/agent-1/"));
    assert!(!written.path.contains(tmp.path().to_string_lossy().as_ref()));
    let body = memory.read("agent-1", "memory").unwrap().unwrap();
    assert!(body.contains("app data directory"));

    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, path FROM memory_index WHERE id = 'agent-1:memory'")
            .fetch_all(service.pool())
            .await
            .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, ".rigel/memory/agent-1/MEMORY.md");

    let error = memory
        .write("agent-1", "../escape", "x", WriteMode::Append)
        .await
        .unwrap_err();
    assert!(matches!(error, MemoryError::InvalidTopic(_)));
}

/// Phase 4, P4.7 — every recorded stream the replay suite consumes exists and is non-empty.
///
/// The replay itself lives in `crates/rigel-adapters/tests/replay.rs`, where the adapters are
/// in scope; this pins the inputs it depends on. A harness or scenario the replay lists but
/// the fixtures lack — or a fixture left behind by a rename — fails here, in the same suite
/// as the other guarantees.
#[test]
fn fixture_replay_covers_every_harness_and_scenario() {
    const HARNESSES: [&str; 3] = ["claude", "codex", "acp"];
    const SCENARIOS: [&str; 5] = ["start", "tool_call", "approval", "error", "interrupt"];
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../rigel-adapters/tests/fixtures/golden");

    for harness in HARNESSES {
        let dir = root.join(harness);
        let mut found: Vec<String> = std::fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                name.strip_suffix(".jsonl").map(str::to_string)
            })
            .collect();
        found.sort();
        let mut expected: Vec<String> = SCENARIOS.iter().map(|name| name.to_string()).collect();
        expected.sort();
        assert_eq!(found, expected, "{harness} fixtures");

        for scenario in SCENARIOS {
            let path = dir.join(format!("{scenario}.jsonl"));
            let body = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(!body.trim().is_empty(), "{} is empty", path.display());
        }
    }
}

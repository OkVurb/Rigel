//! The six guarantees from `docs/handoff.md` §12.
//!
//! They are `#[ignore]`d stubs until the phase that can satisfy them lands. Each carries the
//! exact assertion it must make, so the test is a specification rather than a placeholder.
//! Un-ignore one only when its phase's implementation is in the same commit.

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

/// Phase 5, P5.8 — N1 as an executable check: the product never claims a lock.
#[tokio::test]
#[ignore = "P5.8: needs the claim surface and UI copy to scan"]
async fn claim_never_reports_locked() {
    todo!(
        "scan docs, ui copy and the MCP tool descriptions for the word 'locked'; assert every \
         hit is in a prohibition (N1), not in product copy"
    );
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

/// Phase 4, P4.7 — recorded harness streams must normalise to the same items every time.
#[tokio::test]
#[ignore = "P4.7: needs adapter fixtures"]
async fn fixture_replay_matches_normalisation() {
    todo!(
        "replay crates/rigel-adapters/tests/fixtures/*/*.jsonl and assert the normalised item \
         kinds, statuses and unmapped flags match the expected snapshot"
    );
}

//! The six guarantees from `docs/handoff.md` §12.
//!
//! They are `#[ignore]`d stubs until the phase that can satisfy them lands. Each carries the
//! exact assertion it must make, so the test is a specification rather than a placeholder.
//! Un-ignore one only when its phase's implementation is in the same commit.

/// Phase 3, P3.5 — a patch must never apply over a file that changed underneath it.
#[tokio::test]
#[ignore = "P3.5: needs the patch pipeline (base-hash compare-and-swap)"]
async fn patch_apply_is_cas() {
    todo!(
        "write a file, propose a patch with a stale base_hash, assert STALE_BASE is returned \
         and the file is byte-identical afterwards"
    );
}

/// Phase 3, P3.11 — a crash between journalling and renaming must not double-apply.
#[tokio::test]
#[ignore = "P3.11: needs checkpoint + patch journal"]
async fn resume_never_duplicates_patches() {
    todo!(
        "simulate a kill after the patch row is inserted but before the rename, resume the \
         session, assert one applied patch and an unchanged working tree"
    );
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

/// Phase 3, P3.8 — the kill switch stops work within one turn.
#[tokio::test]
#[ignore = "P3.8: needs BudgetService and a fake adapter"]
async fn budget_stops_within_one_turn() {
    todo!(
        "emit usage above the session cap from a fake adapter, assert interrupt is called exactly \
         once and the next turn is refused"
    );
}

/// Phase 3, P3.6 — memory is committed, so it must not carry machine paths or secrets.
#[tokio::test]
#[ignore = "P3.6: needs MemoryService validation"]
async fn memory_rejects_paths() {
    todo!(
        "attempt to write `C:\\\\Users\\\\someone\\\\project`, `/home/someone/x`, `sk-live-...` and an \
         ssh public key into memory; assert each is rejected with PATH_LEAK/SECRET"
    );
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

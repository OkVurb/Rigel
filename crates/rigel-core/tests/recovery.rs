//! The crash-recovery matrix (P7.2).
//!
//! Four places a process can die, and what has to be true afterwards:
//!
//! | died during | the guarantee |
//! |---|---|
//! | a migration | nothing from that migration is committed; the version is unchanged |
//! | a patch apply | the journal decides, exactly once (covered in `guarantees.rs`) |
//! | an install | the ledger keeps the decision and admits it never finished |
//! | a checkpoint | resume closes the running turn and never claims a clean tree it lacks |
//!
//! The patch rows are not repeated here: `resume_never_duplicates_patches` already drives both
//! `StopAt` windows through `recover`, and a second copy would only test the copy.

use rigel_core::patches::{PatchPipeline, RecoveryState, StopAt};
use rigel_core::{
    Migration, SessionService, SessionStatus, Store, ToolService, resume::resume, tree::tree_hash,
};
use serde_json::json;

/* ------------------------------------------------------------------ *
 * migrations
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn a_migration_killed_halfway_commits_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("partial.db");

    let broken = [
        Migration {
            version: 1,
            name: "0001_ok",
            sql: "CREATE TABLE first (id INTEGER PRIMARY KEY);",
        },
        Migration {
            version: 2,
            name: "0002_breaks",
            sql: "CREATE TABLE second (id INTEGER PRIMARY KEY);
                  INSERT INTO first (id) VALUES (1);
                  CREATE TABLE second (id INTEGER PRIMARY KEY);",
        },
    ];

    let error = match Store::open(&path, &broken).await {
        Ok(_) => panic!("the second migration cannot succeed"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("second"),
        "unexpected error: {error}"
    );
    assert!(path.exists(), "the database file itself is fine");

    // Version 1 committed; version 2 left nothing behind — not the table it created before the
    // failing statement, and not a schema_migrations row.
    let store = Store::open(&path, &[broken[0]]).await.unwrap();
    let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(version, 1);
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table'")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert!(tables.contains(&"first".to_string()));
    assert!(
        !tables.contains(&"second".to_string()),
        "a rolled-back migration left a table: {tables:?}"
    );
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM first")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(rows, 0, "the insert rolled back with the table");
    store.pool().close().await;

    // Fixing the migration and reopening applies it, once.
    let fixed = [
        broken[0],
        Migration {
            version: 2,
            name: "0002_breaks",
            sql: "CREATE TABLE IF NOT EXISTS second (id INTEGER PRIMARY KEY);",
        },
    ];
    let store = Store::open(&path, &fixed).await.unwrap();
    let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(version, 2);
}

#[tokio::test]
async fn a_database_from_a_newer_build_is_refused_not_half_read() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("future.db");
    let only_one = [Migration {
        version: 1,
        name: "0001",
        sql: "CREATE TABLE a (id INTEGER);",
    }];
    let store = Store::open(&path, &only_one).await.unwrap();
    store.pool().close().await;

    let newer = [
        only_one[0],
        Migration {
            version: 2,
            name: "0002_future",
            sql: "CREATE TABLE b (id INTEGER);",
        },
    ];
    let store = Store::open(&path, &newer).await.unwrap();
    store.pool().close().await;

    // Now the older build opens it: it must refuse rather than run against a schema it cannot
    // reason about.
    let error = match Store::open(&path, &only_one).await {
        Ok(_) => panic!("a newer schema is refused"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("newer"), "{error}");
}

/* ------------------------------------------------------------------ *
 * the running turn, after a kill
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn resume_closes_a_running_turn_and_keeps_a_clean_tree_active() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("notes.txt"), "before\n").unwrap();

    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("crash"), None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let hash = tree_hash(tmp.path()).unwrap();
    service
        .checkpoint(&session.id, None, &hash, &json!([]))
        .await
        .unwrap();

    // The turn that was running when the process died. Nothing finished it.
    let turn = service
        .append_turn(&agent.id, Some("write the notes"))
        .await
        .unwrap();

    let outcome = resume(&service, tmp.path(), &session.id, false)
        .await
        .unwrap();

    assert!(outcome.clean, "the tree still matches its checkpoint");
    assert_eq!(outcome.closed_turns.len(), 1);
    let closed = service.turn(&turn.id).await.unwrap();
    assert_eq!(closed.status, "cancelled");
    assert_eq!(closed.stop_reason.as_deref(), Some("interrupted"));
    assert_eq!(outcome.session.status, SessionStatus::Active);
    assert!(outcome.patches.is_empty());
    assert!(outcome.reconcile.ok, "{:?}", outcome.reconcile);

    // Resuming again changes nothing: there is no second closure to make.
    let again = resume(&service, tmp.path(), &session.id, false)
        .await
        .unwrap();
    assert!(again.closed_turns.is_empty());
    assert!(again.clean);
}

#[tokio::test]
async fn a_tree_that_moved_after_the_checkpoint_needs_a_decision() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("notes.txt"), "before\n").unwrap();

    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("drift"), None).await.unwrap();
    let hash = tree_hash(tmp.path()).unwrap();
    service
        .checkpoint(&session.id, None, &hash, &json!([]))
        .await
        .unwrap();

    // The kill happened after someone (or some patch) changed the tree.
    std::fs::write(tmp.path().join("notes.txt"), "after\n").unwrap();

    let outcome = resume(&service, tmp.path(), &session.id, false)
        .await
        .unwrap();
    assert!(!outcome.clean);
    assert_eq!(outcome.session.status, SessionStatus::Interrupted);

    // Forcing it is a recorded intervention and rebaselines the tree, so the next resume is
    // clean instead of asking again.
    let forced = resume(&service, tmp.path(), &session.id, true)
        .await
        .unwrap();
    assert_eq!(forced.session.status, SessionStatus::Active);
    let interventions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM interventions WHERE session_id = ? AND kind = 'resume_confirmed'",
    )
    .bind(&session.id)
    .fetch_one(service.pool())
    .await
    .unwrap();
    assert_eq!(interventions, 1);

    let after = resume(&service, tmp.path(), &session.id, false)
        .await
        .unwrap();
    assert!(after.clean);
}

/* ------------------------------------------------------------------ *
 * an install that never finished
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn an_install_interrupted_after_its_row_stays_visible_and_unfinished() {
    let data = tempfile::tempdir().unwrap();
    let service = ToolService::open(data.path()).await.unwrap();

    // The state the process leaves if it dies between writing the ledger row and updating it:
    // a decision, a command, and no execution record.
    sqlx::query(
        "INSERT INTO install_ledger
           (id, name, package_id, command, trust_score, decision, host, verification)
         VALUES ('interrupted', 'ripgrep', 'BurntSushi.ripgrep.MSVC',
                 'winget install --id BurntSushi.ripgrep.MSVC -e', 98.0,
                 'user-approved', 'windows', 'winget hashes the installer')",
    )
    .execute(service.store().pool())
    .await
    .unwrap();

    let rows = service.ledger(10).await.unwrap();
    let row = rows.iter().find(|row| row.id == "interrupted").unwrap();
    assert_eq!(row.decision, "user-approved");
    assert!(row.executed_at.is_none());
    assert!(row.exit_code.is_none());
    assert!(row.duration_ms.is_none());

    // The command is still there to re-run, and the ledger shows it is the only row.
    assert!(row.command.contains("winget install"));
    assert_eq!(rows.len(), 1);
}

/* ------------------------------------------------------------------ *
 * a kill during a patch apply, through resume
 * ------------------------------------------------------------------ */

#[tokio::test]
async fn a_patch_killed_after_the_journal_is_still_pending_after_resume() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("answer.txt"), "one\n").unwrap();

    let service = SessionService::open(tmp.path()).await.unwrap();
    let session = service.start(Some("patch"), None).await.unwrap();
    let agent = service
        .register_agent(&session.id, "native", Some("worker"))
        .await
        .unwrap();
    let pipeline = PatchPipeline::new(tmp.path(), service.pool().clone());
    let (_, base) = pipeline.read("answer.txt").unwrap();

    let error = pipeline
        .propose_with(
            &session.id,
            &agent.id,
            "answer.txt",
            &base,
            "two\n",
            StopAt::AfterJournal,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, rigel_core::PatchError::Stopped(_)));

    let outcome = resume(&service, tmp.path(), &session.id, false)
        .await
        .unwrap();
    assert_eq!(outcome.patches.len(), 1);
    assert_eq!(outcome.patches[0].state, RecoveryState::Pending);
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("answer.txt")).unwrap(),
        "one\n",
        "resume never applies a pending patch by itself"
    );

    // The user applies it, once; a second apply is a no-op and a second resume is empty.
    pipeline.apply(&outcome.patches[0].patch_id).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("answer.txt")).unwrap(),
        "two\n"
    );
    let again = pipeline.apply(&outcome.patches[0].patch_id).await.unwrap();
    assert_eq!(again.status, "applied");
    assert!(pipeline.recover(&session.id).await.unwrap().is_empty());
}

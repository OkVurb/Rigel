//! Schema-level guarantees the rest of the app is allowed to assume.

use std::path::Path;

use rigel_core::{MigrateError, PROJECT_MIGRATIONS, StorageError, Store, apply};
use tempfile::TempDir;

async fn open_project(root: &Path) -> Store {
    Store::open_project(root)
        .await
        .expect("project database opens")
}

async fn open_index(root: &Path) -> Store {
    Store::open_index(root).await.expect("index database opens")
}

async fn table_names(store: &Store) -> Vec<String> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .fetch_all(store.pool())
            .await
            .expect("sqlite_master is readable");
    rows.into_iter().map(|(name,)| name).collect()
}

/// Starting twice at once is the normal case on Windows: the app and the CLI, or two
/// threads of the same process. Both read `schema_migrations` before either writes, so the
/// read and the migration have to be made atomic against each other.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_first_open_applies_each_migration_once() {
    let tmp = TempDir::new().unwrap();
    let opens = (0..4).map(|_| Store::open_project(tmp.path()));
    for opened in futures::future::join_all(opens).await {
        opened.expect("every concurrent open succeeds");
    }

    let store = open_project(tmp.path()).await;
    let versions: Vec<(i64,)> =
        sqlx::query_as("SELECT version FROM schema_migrations ORDER BY version")
            .fetch_all(store.pool())
            .await
            .unwrap();
    let expected: Vec<(i64,)> = PROJECT_MIGRATIONS
        .iter()
        .map(|migration| (migration.version,))
        .collect();
    assert_eq!(versions, expected, "each migration recorded exactly once");
}

#[tokio::test]
async fn project_schema_applies_once_and_reopening_is_a_no_op() {
    let tmp = TempDir::new().unwrap();
    let store = open_project(tmp.path()).await;

    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(
        mode, "wal",
        "WAL is set once for the file, not requested per connection"
    );

    assert_eq!(store.path().file_name().unwrap(), "rigel.db");
    assert!(store.path().parent().unwrap().ends_with(".rigel"));

    let reopened = open_project(tmp.path()).await;
    let versions: Vec<(i64,)> =
        sqlx::query_as("SELECT version FROM schema_migrations ORDER BY version")
            .fetch_all(reopened.pool())
            .await
            .unwrap();
    let expected: Vec<(i64,)> = PROJECT_MIGRATIONS
        .iter()
        .map(|migration| (migration.version,))
        .collect();
    assert_eq!(versions, expected);

    let applied = apply(reopened.pool(), PROJECT_MIGRATIONS).await.unwrap();
    assert!(applied.is_empty(), "an up-to-date database applies nothing");
}

#[tokio::test]
async fn project_schema_has_every_table_the_plan_names() {
    let tmp = TempDir::new().unwrap();
    let store = open_project(tmp.path()).await;
    let tables = table_names(&store).await;

    for expected in [
        "agents",
        "approvals",
        "checkpoints",
        "claim_conflicts",
        "claims",
        "interventions",
        "items",
        "memory_index",
        "patches",
        "schema_migrations",
        "session_skills",
        "sessions",
        "turns",
    ] {
        assert!(
            tables.iter().any(|t| t == expected),
            "missing table {expected}"
        );
    }
}

/// Phase 3's schema: micro-dollar accounting needs its own column on both spenders, and a
/// turn must be able to say how it ended even when that is not `completed`.
async fn column_names(store: &Store, table: &str) -> Vec<String> {
    // PRAGMA table_info is (cid, name, type, notnull, dflt_value, pk).
    let rows: Vec<(i64, String)> = sqlx::query_as(&format!("PRAGMA table_info({table})"))
        .fetch_all(store.pool())
        .await
        .unwrap();
    rows.into_iter().map(|(_, name)| name).collect()
}

#[tokio::test]
async fn spend_is_kept_in_micro_dollars_and_turns_record_their_stop() {
    let tmp = TempDir::new().unwrap();
    let store = open_project(tmp.path()).await;

    assert!(
        column_names(&store, "sessions")
            .await
            .contains(&"spend_micros".to_string())
    );
    assert!(
        column_names(&store, "agents")
            .await
            .contains(&"spend_micros".to_string())
    );
    assert!(
        column_names(&store, "turns")
            .await
            .contains(&"stop_reason".to_string())
    );
    assert!(
        column_names(&store, "turns")
            .await
            .contains(&"parent_turn_id".to_string()),
        "subagent turns must be attributable to a parent"
    );

    // A fresh row is free, not NULL: callers add to it without a COALESCE.
    sqlx::query(
        "INSERT INTO sessions (id, project_root, status, created_at) VALUES (?, ?, 'active', ?)",
    )
    .bind("s1")
    .bind("/tmp/project")
    .bind(1_i64)
    .execute(store.pool())
    .await
    .unwrap();
    let (spend,): (i64,) = sqlx::query_as("SELECT spend_micros FROM sessions WHERE id = 's1'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(spend, 0);
}

#[tokio::test]
async fn project_writes_a_gitignore_that_covers_session_state() {
    let tmp = TempDir::new().unwrap();
    let _store = open_project(tmp.path()).await;

    let body = std::fs::read_to_string(tmp.path().join(".rigel/.gitignore")).unwrap();
    for line in ["rigel.db", "rigel.db-wal", "sessions/"] {
        assert!(body.contains(line), "gitignore should mention {line}");
    }

    // A project that already has one keeps it.
    std::fs::write(tmp.path().join(".rigel/.gitignore"), "custom\n").unwrap();
    let _ = open_project(tmp.path()).await;
    assert_eq!(
        std::fs::read_to_string(tmp.path().join(".rigel/.gitignore")).unwrap(),
        "custom\n"
    );
}

#[tokio::test]
async fn foreign_keys_are_on_and_agents_cascade_with_their_session() {
    let tmp = TempDir::new().unwrap();
    let store = open_project(tmp.path()).await;

    sqlx::query(
        "INSERT INTO sessions (id, project_root, status, created_at) VALUES (?, ?, 'active', ?)",
    )
    .bind("s1")
    .bind("/tmp/project")
    .bind(1_i64)
    .execute(store.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO agents (id, session_id, harness, status, started_at) VALUES (?, ?, 'codex', 'idle', ?)",
    )
    .bind("a1")
    .bind("s1")
    .bind(1_i64)
    .execute(store.pool())
    .await
    .unwrap();

    let orphan = sqlx::query(
        "INSERT INTO agents (id, session_id, harness, status, started_at) VALUES (?, ?, 'codex', 'idle', ?)",
    )
    .bind("a2")
    .bind("does-not-exist")
    .bind(1_i64)
    .execute(store.pool())
    .await;
    assert!(orphan.is_err(), "foreign keys must reject an orphan agent");

    sqlx::query("DELETE FROM sessions WHERE id = 's1'")
        .execute(store.pool())
        .await
        .unwrap();

    let remaining: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agents")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(remaining.0, 0, "deleting a session removes its agents");
}

#[tokio::test]
async fn index_schema_has_fts5_and_stays_in_sync_with_triggers() {
    let tmp = TempDir::new().unwrap();
    let store = open_index(tmp.path()).await;

    sqlx::query(
        "INSERT INTO skills (id, name, description, source, last_seen_at) VALUES (?, ?, ?, 'github', ?)",
    )
    .bind("github:acme/ocr")
    .bind("ocr-reader")
    .bind("Extracts text from scanned documents")
    .bind(1_i64)
    .execute(store.pool())
    .await
    .unwrap();

    let hits = |term: &'static str| {
        let pool = store.pool().clone();
        async move {
            let rows: Vec<(String,)> = sqlx::query_as(
                "SELECT skills.id FROM skills_fts JOIN skills ON skills.rowid = skills_fts.rowid
                 WHERE skills_fts MATCH ? ORDER BY rank",
            )
            .bind(term)
            .fetch_all(&pool)
            .await
            .unwrap();
            rows.into_iter().map(|(id,)| id).collect::<Vec<_>>()
        }
    };

    assert_eq!(hits("ocr").await, vec!["github:acme/ocr".to_string()]);
    assert!(hits("receipts").await.is_empty());

    sqlx::query("UPDATE skills SET description = ? WHERE id = ?")
        .bind("Extracts text from receipts and invoices")
        .bind("github:acme/ocr")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(hits("receipts").await, vec!["github:acme/ocr".to_string()]);

    sqlx::query("DELETE FROM skills WHERE id = ?")
        .bind("github:acme/ocr")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        hits("receipts").await.is_empty(),
        "delete must clear the index"
    );
}

#[tokio::test]
async fn refuses_to_open_a_schema_newer_than_this_build() {
    let tmp = TempDir::new().unwrap();
    let store = open_project(tmp.path()).await;

    sqlx::query(
        "INSERT INTO schema_migrations (version, name, applied_at) VALUES (99, 'future', 0)",
    )
    .execute(store.pool())
    .await
    .unwrap();
    store.pool().close().await;

    match Store::open_project(tmp.path()).await {
        Err(StorageError::Migrate(MigrateError::NewerSchema { found, known })) => {
            assert_eq!(found, 99);
            assert_eq!(
                known,
                PROJECT_MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
            );
        }
        Err(other) => panic!("wrong error: {other}"),
        Ok(_) => panic!("a newer schema must not open silently"),
    }
}

#[tokio::test]
async fn refuses_to_run_when_a_migration_was_renamed() {
    let tmp = TempDir::new().unwrap();
    let store = open_project(tmp.path()).await;

    sqlx::query("UPDATE schema_migrations SET name = 'renamed' WHERE version = 1")
        .execute(store.pool())
        .await
        .unwrap();
    store.pool().close().await;

    match Store::open_project(tmp.path()).await {
        Err(StorageError::Migrate(MigrateError::Drift { version, .. })) => assert_eq!(version, 1),
        Err(other) => panic!("wrong error: {other}"),
        Ok(_) => panic!("renamed migrations must not be ignored"),
    }
}

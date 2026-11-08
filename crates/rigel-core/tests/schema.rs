//! Schema-level guarantees the rest of the app is allowed to assume.

use std::path::Path;

use rigel_core::{INDEX_MIGRATIONS, MigrateError, PROJECT_MIGRATIONS, StorageError, Store, apply};
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
        "conflict_sides",
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

/// 0005 rebuilds `agents` to admit Gemini. SQLite can only widen a CHECK by replacing the
/// table, and that drop would cascade through `turns` and `claims` if the runner left foreign
/// keys on — so this seeds a v4 database, migrates it, and asserts the transcript survived.
#[tokio::test]
async fn widening_the_harness_check_keeps_existing_rows() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join(".rigel").join("rigel.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    let before = &PROJECT_MIGRATIONS[..PROJECT_MIGRATIONS.len() - 1];
    let store = Store::open(&path, before).await.unwrap();
    for statement in [
        "INSERT INTO sessions (id, project_root, status, created_at) VALUES ('s1', '.', 'active', 0)",
        "INSERT INTO agents (id, session_id, harness, status, started_at) VALUES ('a1', 's1', 'native', 'idle', 0)",
        "INSERT INTO turns (id, agent_id, seq, status, started_at) VALUES ('t1', 'a1', 1, 'completed', 0)",
        "INSERT INTO claims (id, path, agent_id, acquired_at, heartbeat_at) VALUES ('c1', 'src/lib.rs', 'a1', 0, 0)",
    ] {
        sqlx::query(statement).execute(store.pool()).await.unwrap();
    }
    store.pool().close().await;

    let store = Store::open(&path, PROJECT_MIGRATIONS).await.unwrap();
    let count = |table: &'static str| {
        let pool = store.pool().clone();
        async move {
            sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert_eq!(
        (
            count("agents").await,
            count("turns").await,
            count("claims").await
        ),
        (1, 1, 1),
        "the rebuild must not cascade into the transcript"
    );

    sqlx::query(
        "INSERT INTO agents (id, session_id, harness, status, started_at) VALUES ('a2', 's1', 'gemini', 'idle', 0)",
    )
    .execute(store.pool())
    .await
    .expect("a gemini agent is accepted after 0005");

    let refused = sqlx::query(
        "INSERT INTO agents (id, session_id, harness, status, started_at) VALUES ('a3', 's1', 'bogus', 'idle', 0)",
    )
    .execute(store.pool())
    .await;
    assert!(
        refused.is_err(),
        "the rebuilt CHECK still guards the harness column"
    );
}

/// 0014 rebuilds `turns` to admit `incomplete`. The drop would cascade into `items` if the
/// runner left foreign keys on, so this seeds a turn and its transcript row, migrates, and
/// asserts both survived and that only the new status was added.
#[tokio::test]
async fn widening_the_turn_status_check_keeps_the_transcript() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join(".rigel").join("rigel.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();

    let before = &PROJECT_MIGRATIONS[..PROJECT_MIGRATIONS.len() - 1];
    let store = Store::open(&path, before).await.unwrap();
    for statement in [
        "INSERT INTO sessions (id, project_root, status, created_at) VALUES ('s1', '.', 'active', 0)",
        "INSERT INTO agents (id, session_id, harness, status, started_at) VALUES ('a1', 's1', 'native', 'idle', 0)",
        "INSERT INTO turns (id, agent_id, seq, status, prompt, started_at, completed_at, usage_json, stop_reason, parent_turn_id) VALUES ('t1', 'a1', 1, 'completed', 'fix it', 0, 1, '{\"cost_micros\":7}', 'completed', NULL)",
        "INSERT INTO items (id, turn_id, seq, kind, status, body_json, raw_json, created_at) VALUES ('i1', 't1', 1, 'message', 'complete', '{}', '{}', 0)",
    ] {
        sqlx::query(statement).execute(store.pool()).await.unwrap();
    }
    store.pool().close().await;

    let store = Store::open(&path, PROJECT_MIGRATIONS).await.unwrap();
    let turn: (String, Option<String>, Option<String>) =
        sqlx::query_as("SELECT status, stop_reason, usage_json FROM turns WHERE id = 't1'")
            .fetch_one(store.pool())
            .await
            .expect("the turn survives the rebuild");
    assert_eq!(turn.0, "completed");
    assert_eq!(turn.1.as_deref(), Some("completed"));
    assert_eq!(turn.2.as_deref(), Some("{\"cost_micros\":7}"));

    let items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM items WHERE turn_id = 't1'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(items, 1, "the drop must not cascade into the transcript");

    sqlx::query(
        "INSERT INTO turns (id, agent_id, seq, status, started_at) VALUES ('t2', 'a1', 2, 'incomplete', 0)",
    )
    .execute(store.pool())
    .await
    .expect("incomplete is accepted after 0014");

    let refused = sqlx::query(
        "INSERT INTO turns (id, agent_id, seq, status, started_at) VALUES ('t3', 'a1', 3, 'done-ish', 0)",
    )
    .execute(store.pool())
    .await;
    assert!(
        refused.is_err(),
        "the rebuilt CHECK still guards the status column"
    );
}

/// Item 2: a database at any historical prefix must upgrade to the current schema with its rows
/// intact. Every prefix is rehearsed, not just the one a hand-written fixture happens to name.
#[tokio::test]
async fn every_project_prefix_upgrades_with_its_rows_intact() {
    let expected: Vec<(i64,)> = PROJECT_MIGRATIONS
        .iter()
        .map(|migration| (migration.version,))
        .collect();

    for boundary in 1..=PROJECT_MIGRATIONS.len() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".rigel").join("rigel.db");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let at = Store::open(&path, &PROJECT_MIGRATIONS[..boundary])
            .await
            .unwrap_or_else(|error| panic!("prefix {boundary} did not open: {error}"));
        for statement in [
            "INSERT INTO sessions (id, project_root, status, created_at) VALUES ('s1', '.', 'active', 0)",
            "INSERT INTO agents (id, session_id, harness, status, started_at) VALUES ('a1', 's1', 'native', 'idle', 0)",
        ] {
            sqlx::query(statement).execute(at.pool()).await.unwrap();
        }
        at.pool().close().await;

        let upgraded = Store::open(&path, PROJECT_MIGRATIONS)
            .await
            .unwrap_or_else(|error| panic!("prefix {boundary} did not upgrade: {error}"));
        let versions: Vec<(i64,)> =
            sqlx::query_as("SELECT version FROM schema_migrations ORDER BY version")
                .fetch_all(upgraded.pool())
                .await
                .unwrap();
        assert_eq!(
            versions, expected,
            "a database at prefix {boundary} did not reach every later migration"
        );
        println!("project prefix {boundary} -> {versions:?}");

        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(upgraded.pool())
            .await
            .unwrap();
        let agents: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents")
            .fetch_one(upgraded.pool())
            .await
            .unwrap();
        assert_eq!(
            (sessions, agents),
            (1, 1),
            "prefix {boundary} lost the rows it started with"
        );

        let again = apply(upgraded.pool(), PROJECT_MIGRATIONS).await.unwrap();
        assert!(
            again.is_empty(),
            "prefix {boundary} left work for a second open"
        );
        upgraded.pool().close().await;
    }
}

/// The same rehearsal for the index database, whose migrations are a separate list.
#[tokio::test]
async fn every_index_prefix_upgrades_to_the_current_schema() {
    let expected: Vec<(i64,)> = INDEX_MIGRATIONS
        .iter()
        .map(|migration| (migration.version,))
        .collect();

    for boundary in 1..=INDEX_MIGRATIONS.len() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("index.db");

        let at = Store::open(&path, &INDEX_MIGRATIONS[..boundary])
            .await
            .unwrap_or_else(|error| panic!("index prefix {boundary} did not open: {error}"));
        at.pool().close().await;

        let upgraded = Store::open(&path, INDEX_MIGRATIONS)
            .await
            .unwrap_or_else(|error| panic!("index prefix {boundary} did not upgrade: {error}"));
        let versions: Vec<(i64,)> =
            sqlx::query_as("SELECT version FROM schema_migrations ORDER BY version")
                .fetch_all(upgraded.pool())
                .await
                .unwrap();
        assert_eq!(
            versions, expected,
            "an index at prefix {boundary} did not reach every later migration"
        );
        println!("index prefix {boundary} -> {versions:?}");
        upgraded.pool().close().await;
    }
}

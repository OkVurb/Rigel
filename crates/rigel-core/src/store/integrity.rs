//! SQLite integrity check (roadmap 182).
//!
//! `PRAGMA integrity_check` and `PRAGMA foreign_key_check` run after an unclean shutdown.
//! A foreign-key problem is reported with its table and row. The repair path copies the
//! database file into a backup directory *before* it rebuilds anything, so a failed repair
//! always has the pre-repair bytes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use crate::migrate::now_ms;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    pub check: String,
    pub table: Option<String>,
    pub row: Option<i64>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub ok: bool,
    pub problems: Vec<Problem>,
    pub checked_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repair {
    pub before: Report,
    pub after: Report,
    /// The copy taken before the repair. `None` means the database was already healthy.
    pub backup: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum IntegrityError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("could not copy the database: {0}")]
    Io(#[from] io::Error),
}

/// Run both checks. An empty problem list is the only healthy report.
pub async fn check(pool: &SqlitePool) -> Result<Report, sqlx::Error> {
    let mut problems = Vec::new();

    let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(pool)
        .await?;
    for line in integrity {
        if line.eq_ignore_ascii_case("ok") {
            continue;
        }
        problems.push(Problem {
            check: "integrity_check".to_string(),
            table: None,
            row: None,
            message: line,
        });
    }

    let foreign_keys = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await?;
    for row in foreign_keys {
        let table: String = row.try_get("table")?;
        let rowid: i64 = row.try_get("rowid")?;
        let parent: String = row.try_get("parent")?;
        problems.push(Problem {
            check: "foreign_key_check".to_string(),
            table: Some(table),
            row: Some(rowid),
            message: format!("row {rowid} references a missing {parent} row"),
        });
    }

    Ok(Report {
        ok: problems.is_empty(),
        problems,
        checked_at_ms: now_ms(),
    })
}

/// Check, and if anything is wrong copy the file to `backup_dir` before rebuilding the
/// indexes and vacuuming. The returned report is the state after the repair; `before` is
/// kept so the caller can show what was found.
pub async fn check_and_repair(db: &Path, backup_dir: &Path) -> Result<Repair, IntegrityError> {
    let pool = connect(db).await?;
    let before = check(&pool).await?;
    if before.ok {
        return Ok(Repair {
            before: before.clone(),
            after: before,
            backup: None,
        });
    }
    pool.close().await;

    fs::create_dir_all(backup_dir)?;
    let backup = backup_dir.join(format!("integrity-{}.sqlite", now_ms()));
    fs::copy(db, &backup)?;

    let pool = connect(db).await?;
    sqlx::query("REINDEX").execute(&pool).await?;
    sqlx::query("VACUUM").execute(&pool).await?;
    let after = check(&pool).await?;
    Ok(Repair {
        before,
        after,
        backup: Some(backup),
    })
}

async fn connect(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .foreign_keys(true);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteSynchronous;

    async fn fixture(path: &Path, foreign_keys: bool) -> SqlitePool {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(foreign_keys)
            .synchronous(SqliteSynchronous::Normal);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE parent (id INTEGER PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE child (
                id INTEGER PRIMARY KEY,
                parent_id INTEGER NOT NULL REFERENCES parent(id)
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[test]
    fn a_healthy_database_reports_ok() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("healthy.sqlite");
            let pool = fixture(&path, true).await;
            sqlx::query("INSERT INTO parent (id) VALUES (1)")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO child (id, parent_id) VALUES (7, 1)")
                .execute(&pool)
                .await
                .unwrap();
            let report = check(&pool).await.unwrap();
            assert!(report.ok, "{:?}", report.problems);
        });
    }

    #[test]
    fn a_foreign_key_orphan_is_reported_with_table_and_row() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("orphan.sqlite");
            let pool = fixture(&path, false).await;
            sqlx::query("INSERT INTO child (id, parent_id) VALUES (7, 42)")
                .execute(&pool)
                .await
                .unwrap();
            let report = check(&pool).await.unwrap();
            assert!(!report.ok);
            let problem = report
                .problems
                .iter()
                .find(|problem| problem.check == "foreign_key_check")
                .expect("a foreign-key problem");
            assert_eq!(problem.table.as_deref(), Some("child"));
            assert_eq!(problem.row, Some(7));
            assert!(problem.message.contains("parent"));
        });
    }

    #[test]
    fn the_repair_path_copies_the_file_before_it_touches_anything() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("repair.sqlite");
            let pool = fixture(&path, false).await;
            sqlx::query("INSERT INTO child (id, parent_id) VALUES (3, 99)")
                .execute(&pool)
                .await
                .unwrap();
            pool.close().await;
            let original = fs::read(&path).unwrap();
            let backups = dir.path().join("backups");
            let repair = check_and_repair(&path, &backups).await.unwrap();
            let backup = repair.backup.expect("a backup before repair");
            assert_eq!(fs::read(&backup).unwrap(), original);
            assert!(!repair.before.ok);
        });
    }
}

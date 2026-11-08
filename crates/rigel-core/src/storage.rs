//! Opening the two SQLite databases and keeping their pragmas honest.
//!
//! * Project database — `<repo>/.rigel/rigel.db`, WAL, foreign keys on, never committed.
//! * Index database — `%APPDATA%\Rigel\index.db`, WAL, shared across projects.
//!
//! Both are opened through [`Store`] so there is exactly one place that decides pragmas,
//! pool size, and what happens when the directory does not exist yet.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteSynchronous};

use crate::migrate::{
    INDEX_MIGRATIONS, MigrateError, Migration, PROJECT_MIGRATIONS, apply, now_ms,
};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("could not create {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error(transparent)]
    Migrate(#[from] MigrateError),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("could not put the database in WAL mode: {0}")]
    Wal(String),
}

/// An open, migrated database. Cloning shares the pool.
#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
    path: PathBuf,
}

impl Store {
    /// Open (creating if needed) the session database for a project root.
    ///
    /// Also writes `.rigel/.gitignore` on first use so a session log can never be
    /// committed by accident (handoff §6.1).
    pub async fn open_project(root: &Path) -> Result<Self, StorageError> {
        let dir = root.join(".rigel");
        create_dir(&dir)?;
        write_gitignore(&dir)?;
        let store = Self::open(dir.join("rigel.db"), PROJECT_MIGRATIONS).await?;
        // Q273: one copy a day, three kept. A backup that cannot be written must not stop the
        // project from opening; the next launch tries again, and diagnostics still reports the
        // database's own health.
        let _ = store.backup_daily().await;
        Ok(store)
    }

    /// Open (creating if needed) the global index database.
    pub async fn open_index(data_dir: &Path) -> Result<Self, StorageError> {
        let dir = data_dir.to_path_buf();
        create_dir(&dir)?;
        Self::open(dir.join("index.db"), INDEX_MIGRATIONS).await
    }

    /// Open a database file and bring its schema up to date.
    pub async fn open(
        path: impl AsRef<Path>,
        migrations: &[Migration],
    ) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();

        // `journal_mode` is deliberately not an option here. sqlx applies requested pragmas
        // on every connection it opens, and switching journal mode takes an exclusive lock
        // that `busy_timeout` cannot wait for: a connection opened while another write is in
        // flight fails immediately with "database is locked", and the attempt can also
        // invalidate a read snapshot underneath a deferred transaction. WAL belongs to the
        // file, so it is set once, below.
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options)
            .await?;

        ensure_wal(&pool).await?;
        apply(&pool, migrations).await?;

        Ok(Self { pool, path })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Q273: a daily copy of the project database, three kept.
    ///
    /// The copy is made by SQLite itself (`VACUUM INTO`), never by copying the file: WAL mode
    /// keeps committed pages in the `-wal` sidecar, and a raw file copy can capture a torn
    /// database. One file per UTC day means a restart loop cannot fill the disk, and pruning
    /// happens only after the new copy exists.
    pub async fn backup_daily(&self) -> Result<Option<PathBuf>, StorageError> {
        self.backup_at(now_ms()).await
    }

    async fn backup_at(&self, now: i64) -> Result<Option<PathBuf>, StorageError> {
        let Some(dir) = self.path.parent() else {
            return Ok(None);
        };
        // Only the project database is precious; the index is rebuildable and lives elsewhere.
        if self.path.file_stem().and_then(|stem| stem.to_str()) != Some("rigel") {
            return Ok(None);
        }
        let backups = dir.join("backups");
        create_dir(&backups)?;
        let day = now.div_euclid(24 * 60 * 60 * 1000);
        let target = backups.join(format!("rigel-{day}.db"));
        if !target.exists() {
            let escaped = target.to_string_lossy().replace('\'', "''");
            sqlx::query(&format!("VACUUM INTO '{escaped}'"))
                .execute(&self.pool)
                .await?;
        }
        prune_backups(&backups, 3)?;
        Ok(Some(target))
    }
}

/// Keep the newest `keep` daily copies, identified by the day encoded in the file name.
fn prune_backups(dir: &Path, keep: usize) -> Result<(), StorageError> {
    let mut days: Vec<(i64, PathBuf)> = fs::read_dir(dir)
        .map_err(|source| StorageError::Io {
            path: dir.to_path_buf(),
            source,
        })?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let day = name
                .strip_prefix("rigel-")?
                .strip_suffix(".db")?
                .parse::<i64>()
                .ok()?;
            Some((day, path))
        })
        .collect();
    days.sort_by_key(|(day, _)| *day);
    while days.len() > keep {
        let (_, path) = days.remove(0);
        if let Err(source) = fs::remove_file(&path) {
            return Err(StorageError::Io { path, source });
        }
    }
    Ok(())
}

/// Put the database in WAL mode if it is not already, with retries.
///
/// The pragma that changes journal mode needs an exclusive lock, and SQLite will not consult
/// the busy handler for it, so two processes opening a brand-new project at once can collide.
/// Once the file is WAL the call is a no-op that returns the current mode.
async fn ensure_wal(pool: &SqlitePool) -> Result<(), StorageError> {
    let mut mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(pool)
        .await?;
    for _ in 0..40 {
        if mode.eq_ignore_ascii_case("wal") {
            return Ok(());
        }
        match sqlx::query_scalar::<_, String>("PRAGMA journal_mode = WAL")
            .fetch_one(pool)
            .await
        {
            Ok(next) => mode = next,
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    if mode.eq_ignore_ascii_case("wal") {
        Ok(())
    } else {
        Err(StorageError::Wal(format!(
            "it stayed in {mode}; another process is holding the file"
        )))
    }
}

/// `%APPDATA%\Rigel` on Windows, `~/.local/share/rigel` elsewhere.
pub fn default_index_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("Rigel");
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("rigel");
    }
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join(".local/share/rigel"),
        None => PathBuf::from(".rigel-index"),
    }
}

fn create_dir(dir: &Path) -> Result<(), StorageError> {
    fs::create_dir_all(dir).map_err(|source| StorageError::Io {
        path: dir.to_path_buf(),
        source,
    })
}

/// Written once; a project that already has its own `.rigel/.gitignore` is left alone.
fn write_gitignore(dir: &Path) -> Result<(), StorageError> {
    let path = dir.join(".gitignore");
    if path.exists() {
        return Ok(());
    }
    let body = "# Written by Rigel. Session logs are local: they contain prompts and machine paths.\n\
                rigel.db\n\
                rigel.db-wal\n\
                rigel.db-shm\n\
                sessions/\n\
                snapshots/\n\
                worktrees/\n";
    fs::write(&path, body).map_err(|source| StorageError::Io { path, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn daily_backups_keep_three_and_survive_a_reopen() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".rigel").join("rigel.db");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let store = Store::open(&path, PROJECT_MIGRATIONS).await.unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, project_root, status, created_at) VALUES ('s1', '.', 'active', 0)",
        )
        .execute(store.pool())
        .await
        .unwrap();

        for day in 1..=4 {
            store.backup_at(day * 86_400_000).await.unwrap();
        }
        let mut names: Vec<String> = std::fs::read_dir(tmp.path().join(".rigel/backups"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["rigel-2.db", "rigel-3.db", "rigel-4.db"],
            "the oldest copy is pruned once there are four"
        );

        let backup = Store::open(
            tmp.path().join(".rigel/backups/rigel-4.db"),
            PROJECT_MIGRATIONS,
        )
        .await
        .unwrap();
        let sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(backup.pool())
            .await
            .unwrap();
        assert_eq!(sessions, 1, "the copy is a readable database with the row");
        backup.pool().close().await;
    }
}

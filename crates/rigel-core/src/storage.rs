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
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

use crate::migrate::{INDEX_MIGRATIONS, MigrateError, Migration, PROJECT_MIGRATIONS, apply};

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
}

/// An open, migrated database.
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
        Self::open(dir.join("rigel.db"), PROJECT_MIGRATIONS).await
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

        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options)
            .await?;

        apply(&pool, migrations).await?;

        Ok(Self { pool, path })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn path(&self) -> &Path {
        &self.path
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
                worktrees/\n";
    fs::write(&path, body).map_err(|source| StorageError::Io { path, source })
}

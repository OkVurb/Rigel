//! Numbered SQL migrations, applied one transaction at a time.
//!
//! There is no ORM and no migration DSL: a migration is a `.sql` file compiled into the
//! binary, applied inside a transaction together with its `schema_migrations` row. The
//! runner refuses to start against a schema that is newer than the build, and refuses to
//! run when an applied version has been renamed — both are silent-corruption bugs otherwise.

use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::SqlitePool;

/// One numbered migration. `sql` may contain multiple statements.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

/// `<repo>/.rigel/rigel.db`
pub const PROJECT_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "0001_init",
    sql: include_str!("../../../migrations/project/0001_init.sql"),
}];

/// `%APPDATA%\Rigel\index.db`
pub const INDEX_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "0001_init",
    sql: include_str!("../../../migrations/index/0001_init.sql"),
}];

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("schema version {found} is newer than this build knows about (newest: {known})")]
    NewerSchema { found: i64, known: i64 },

    #[error("migration {version} was applied as {applied:?} but this build calls it {expected:?}")]
    Drift {
        version: i64,
        applied: String,
        expected: &'static str,
    },
}

/// Apply every migration not yet recorded, in order.
///
/// Returns the versions applied by this call; an up-to-date database returns an empty vec.
pub async fn apply(pool: &SqlitePool, migrations: &[Migration]) -> Result<Vec<i64>, MigrateError> {
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           version    INTEGER PRIMARY KEY,
           name       TEXT NOT NULL,
           applied_at INTEGER NOT NULL
         );",
    )
    .execute(pool)
    .await?;

    let applied: Vec<(i64, String)> =
        sqlx::query_as("SELECT version, name FROM schema_migrations ORDER BY version")
            .fetch_all(pool)
            .await?;

    let known_max = migrations.iter().map(|m| m.version).max().unwrap_or(0);
    if let Some((found, _)) = applied
        .iter()
        .rev()
        .find(|(version, _)| *version > known_max)
    {
        return Err(MigrateError::NewerSchema {
            found: *found,
            known: known_max,
        });
    }

    for (version, name) in &applied {
        match migrations.iter().find(|m| m.version == *version) {
            Some(m) if m.name == name => {}
            Some(m) => {
                return Err(MigrateError::Drift {
                    version: *version,
                    applied: name.clone(),
                    expected: m.name,
                });
            }
            None => {
                return Err(MigrateError::Drift {
                    version: *version,
                    applied: name.clone(),
                    expected: "<no migration file in this build>",
                });
            }
        }
    }

    let mut applied_now = Vec::new();
    for migration in migrations {
        if applied
            .iter()
            .any(|(version, _)| *version == migration.version)
        {
            continue;
        }

        let mut tx = pool.begin().await?;
        sqlx::raw_sql(migration.sql).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO schema_migrations (version, name, applied_at) VALUES (?, ?, ?)")
            .bind(migration.version)
            .bind(migration.name)
            .bind(now_ms())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        applied_now.push(migration.version);
    }

    Ok(applied_now)
}

/// Milliseconds since the Unix epoch. Every timestamp column in Rigel uses this.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

//! Rigel's core: storage, migrations, and (from later phases) sessions, claims,
//! patches and budgets.
//!
//! The core owns all application state. Nothing in the renderer or in an adapter is
//! allowed to hold a session, a claim or a budget as authoritative; see
//! `docs/handoff.md` §3.2 (S2).

pub mod migrate;
pub mod session;
pub mod storage;

pub use migrate::{INDEX_MIGRATIONS, MigrateError, Migration, PROJECT_MIGRATIONS, apply, now_ms};
pub use session::{
    Agent, Checkpoint, Item, Session, SessionError, SessionService, SessionStatus, Turn,
};
pub use storage::{StorageError, Store, default_index_dir};

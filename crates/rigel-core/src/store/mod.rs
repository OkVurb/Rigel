//! Small durable stores that sit beside the session log.
//!
//! `paused` is the only member today: a paused turn is a real row in the project database so a
//! closed window can reopen the same messages and remaining budget without a sidecar file.

pub mod integrity;
pub mod paused;

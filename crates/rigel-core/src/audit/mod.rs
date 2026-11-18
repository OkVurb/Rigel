//! Tamper-evident approvals and command provenance (roadmap 65 and 84).
//!
//! The session database records what happened. This module adds the two things a reviewer
//! needs after the fact: an append-only approval ledger whose lines are chained by SHA-256,
//! and a read-only projection that links one command item back to its turn, model step, tool
//! call, approval and snapshot.

pub mod chain;
pub mod provenance;

pub use chain::{
    ApprovalInput, ApprovalRecord, BrokenLink, ChainError, VerifyReport, append, ledger_path,
    verify, verify_file,
};
pub use provenance::{
    ApprovalLink, CommandProvenance, Link, ProvenanceError, ToolCallLink, command,
};

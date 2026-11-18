//! The append-only approval ledger (roadmap 65).
//!
//! Each line is one approval as JSON. A line commits to the previous line's hash, so removing,
//! reordering or editing a line breaks the chain at that point. The hash is plain SHA-256: this
//! is evidence of accidental or local tampering, not a signature by a remote authority.
//!
//! The ledger is written under `.rigel/audit/approvals.jsonl`, which is ignored by git. The
//! database `approvals` row is written first by `SessionService::record_approval`; this module
//! only knows how to seal and verify the file, so it can also verify a copied export offline.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Project-relative location of the approval ledger.
pub const APPROVALS_LOG: &str = ".rigel/audit/approvals.jsonl";
/// The previous hash of the first record. A full-width zero hash cannot be confused with a
/// real digest by accident.
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
/// Versioned so a future format can be refused instead of silently interpreted as this one.
pub const SCHEMA: &str = "rigel.audit.approval/1";

/// One approval, exactly as it appears on a ledger line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub schema: String,
    pub seq: u64,
    pub approval_id: String,
    pub session_id: String,
    pub agent_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub kind: String,
    pub request: Value,
    pub decision: String,
    #[serde(default)]
    pub scope: Option<String>,
    pub requested_at: i64,
    #[serde(default)]
    pub decided_at: Option<i64>,
    pub prev_hash: String,
    pub hash: String,
}

/// The caller-supplied part of a record. `append` assigns the sequence, previous hash and
/// digest; a caller cannot invent a position in the chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalInput {
    pub approval_id: String,
    pub session_id: String,
    pub agent_id: String,
    pub turn_id: Option<String>,
    pub kind: String,
    pub request: Value,
    pub decision: String,
    pub scope: Option<String>,
    pub requested_at: i64,
    pub decided_at: Option<i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("audit ledger io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the audit ledger could not be locked")]
    Lock,
    #[error("the audit ledger is broken at line {line}: {reason}")]
    Broken { line: usize, reason: String },
    #[error("could not encode an audit record: {0}")]
    Encode(String),
}

/// One broken link. `line` is 1-based, the same numbering a text editor and `rigel audit
/// verify` use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrokenLink {
    pub line: usize,
    pub seq: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerifyReport {
    pub path: PathBuf,
    pub records: usize,
    pub valid: bool,
    pub first_bad: Option<BrokenLink>,
}

/// A process-wide lock is enough for the desktop app's single writer. Cross-process copies are
/// verified independently; a second process appending concurrently is not part of the product's
/// writer model.
static APPEND_LOCK: Mutex<()> = Mutex::new(());

pub fn ledger_path(root: &Path) -> PathBuf {
    root.join(APPROVALS_LOG)
}

#[derive(Serialize)]
struct HashBody<'a> {
    schema: &'a str,
    seq: u64,
    approval_id: &'a str,
    session_id: &'a str,
    agent_id: &'a str,
    turn_id: &'a Option<String>,
    kind: &'a str,
    request: &'a Value,
    decision: &'a str,
    scope: &'a Option<String>,
    requested_at: i64,
    decided_at: Option<i64>,
    prev_hash: &'a str,
}

fn digest(record: &ApprovalRecord) -> String {
    let body = HashBody {
        schema: &record.schema,
        seq: record.seq,
        approval_id: &record.approval_id,
        session_id: &record.session_id,
        agent_id: &record.agent_id,
        turn_id: &record.turn_id,
        kind: &record.kind,
        request: &record.request,
        decision: &record.decision,
        scope: &record.scope,
        requested_at: record.requested_at,
        decided_at: record.decided_at,
        prev_hash: &record.prev_hash,
    };
    let bytes = serde_json::to_vec(&body).expect("audit hash body is serializable");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Seal one new record onto the ledger. The existing chain is verified first: appending to a
/// broken ledger would make the new record look authoritative when it is not.
pub fn append(root: &Path, input: ApprovalInput) -> Result<ApprovalRecord, ChainError> {
    let _guard = APPEND_LOCK.lock().map_err(|_| ChainError::Lock)?;
    let path = ledger_path(root);
    let records = read_records(&path)?;
    let report = check_records(&path, &records);
    if let Some(bad) = report.first_bad {
        return Err(ChainError::Broken {
            line: bad.line,
            reason: bad.reason,
        });
    }

    let seq = records.last().map(|record| record.seq + 1).unwrap_or(1);
    let prev_hash = records
        .last()
        .map(|record| record.hash.clone())
        .unwrap_or_else(|| GENESIS_HASH.to_string());
    let mut record = ApprovalRecord {
        schema: SCHEMA.to_string(),
        seq,
        approval_id: input.approval_id,
        session_id: input.session_id,
        agent_id: input.agent_id,
        turn_id: input.turn_id,
        kind: input.kind,
        request: input.request,
        decision: input.decision,
        scope: input.scope,
        requested_at: input.requested_at,
        decided_at: input.decided_at,
        prev_hash,
        hash: String::new(),
    };
    record.hash = digest(&record);

    let line =
        serde_json::to_string(&record).map_err(|error| ChainError::Encode(error.to_string()))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ChainError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| ChainError::Io {
            path: path.clone(),
            source,
        })?;
    writeln!(file, "{line}").map_err(|source| ChainError::Io {
        path: path.clone(),
        source,
    })?;
    file.sync_all().map_err(|source| ChainError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(record)
}

/// Read every record without verifying the chain. A malformed line is itself a broken link.
pub fn read_records(path: &Path) -> Result<Vec<ApprovalRecord>, ChainError> {
    let mut text = String::new();
    match fs::File::open(path) {
        Ok(mut file) => {
            file.read_to_string(&mut text)
                .map_err(|source| ChainError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(ChainError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    }

    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            return Err(ChainError::Broken {
                line: index + 1,
                reason: "the line is blank".to_string(),
            });
        }
        let record =
            serde_json::from_str::<ApprovalRecord>(line).map_err(|error| ChainError::Broken {
                line: index + 1,
                reason: format!("not a valid approval record: {error}"),
            })?;
        records.push(record);
    }
    Ok(records)
}

fn check_records(path: &Path, records: &[ApprovalRecord]) -> VerifyReport {
    let mut previous = GENESIS_HASH.to_string();
    for (index, record) in records.iter().enumerate() {
        let line = index + 1;
        let expected_seq = index as u64 + 1;
        let bad = if record.schema != SCHEMA {
            Some(format!(
                "schema is {:?}; expected {SCHEMA:?}",
                record.schema
            ))
        } else if record.seq != expected_seq {
            Some(format!(
                "sequence is {}; expected {expected_seq}",
                record.seq
            ))
        } else if record.prev_hash != previous {
            Some(format!(
                "previous hash is {}; expected {previous}",
                record.prev_hash
            ))
        } else if digest(record) != record.hash {
            Some("hash does not match the record contents".to_string())
        } else {
            None
        };
        if let Some(reason) = bad {
            return VerifyReport {
                path: path.to_path_buf(),
                records: records.len(),
                valid: false,
                first_bad: Some(BrokenLink {
                    line,
                    seq: Some(record.seq),
                    reason,
                }),
            };
        }
        previous = record.hash.clone();
    }
    VerifyReport {
        path: path.to_path_buf(),
        records: records.len(),
        valid: true,
        first_bad: None,
    }
}

/// Verify a ledger at `root/.rigel/audit/approvals.jsonl`.
pub fn verify(root: &Path) -> Result<VerifyReport, ChainError> {
    verify_file(&ledger_path(root))
}

/// Verify any copied ledger. An absent file is an empty, intact ledger.
pub fn verify_file(path: &Path) -> Result<VerifyReport, ChainError> {
    let records = match read_records(path) {
        Ok(records) => records,
        Err(ChainError::Broken { line, reason }) => {
            return Ok(VerifyReport {
                path: path.to_path_buf(),
                records: 0,
                valid: false,
                first_bad: Some(BrokenLink {
                    line,
                    seq: None,
                    reason,
                }),
            });
        }
        Err(error) => return Err(error),
    };
    Ok(check_records(path, &records))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(id: &str, decision: &str) -> ApprovalInput {
        ApprovalInput {
            approval_id: id.to_string(),
            session_id: "session-1".to_string(),
            agent_id: "agent-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            kind: "command".to_string(),
            request: serde_json::json!({ "command": "echo audit" }),
            decision: decision.to_string(),
            scope: Some("once".to_string()),
            requested_at: 1_700_000_000_000,
            decided_at: Some(1_700_000_000_100),
        }
    }

    #[test]
    fn a_chain_seals_and_verifies() {
        let root = tempfile::tempdir().unwrap();
        let first = append(root.path(), input("a", "allowed")).unwrap();
        let second = append(root.path(), input("b", "denied")).unwrap();
        assert_eq!(first.seq, 1);
        assert_eq!(first.prev_hash, GENESIS_HASH);
        assert_eq!(second.seq, 2);
        assert_eq!(second.prev_hash, first.hash);

        let report = verify(root.path()).unwrap();
        assert!(report.valid, "{report:?}");
        assert_eq!(report.records, 2);
        assert!(report.first_bad.is_none());
    }

    #[test]
    fn a_mutated_line_names_the_first_broken_link_and_refuses_further_appends() {
        let root = tempfile::tempdir().unwrap();
        append(root.path(), input("a", "allowed")).unwrap();
        append(root.path(), input("b", "denied")).unwrap();
        let path = ledger_path(root.path());
        let mut lines: Vec<String> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(lines.len(), 2);
        lines[1] = lines[1].replacen("\"decision\":\"denied\"", "\"decision\":\"allowed\"", 1);
        fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();

        let report = verify(root.path()).unwrap();
        assert!(!report.valid);
        let bad = report.first_bad.expect("tampering is reported");
        assert_eq!(bad.line, 2);
        assert_eq!(bad.seq, Some(2));
        assert!(bad.reason.contains("hash does not match"), "{bad:?}");

        let error = append(root.path(), input("c", "allowed")).unwrap_err();
        assert!(matches!(error, ChainError::Broken { line: 2, .. }));
    }

    #[test]
    fn a_missing_ledger_is_empty_not_a_failure() {
        let root = tempfile::tempdir().unwrap();
        let report = verify(root.path()).unwrap();
        assert!(report.valid);
        assert_eq!(report.records, 0);
    }
}

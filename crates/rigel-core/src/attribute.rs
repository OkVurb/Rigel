//! Observed changes and attribution (P5.3, §8.2).
//!
//! Reported changes (a patch in the journal, a `file_change` item) are the highest-confidence
//! source. This module is the second: a poll of the project tree, debounced, hashing only files
//! whose size or modified time moved. It exists because harnesses write files themselves, and a
//! write Rigel did not perform is exactly the write worth attributing.
//!
//! Attribution is honest about doubt: exactly one running agent gets the change, several running
//! agents all get it and the change is marked `ambiguous`. Nothing here ever blocks a write.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::claims::{ClaimConflict, ClaimService};
use crate::patches::sha256_hex;
use crate::session::Agent;

/// The debounce in §8.2. One poll interval is also the worst-case detection latency.
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);
/// A tree larger than this is not watched: rigel says so rather than pretending.
pub const MAX_WATCHED_FILES: usize = 5_000;
/// A file larger than this is not hashed; its size and mtime still mark it as changed.
pub const MAX_HASH_BYTES: u64 = 1024 * 1024;

const SKIP_DIRS: &[&str] = &[
    ".git",
    ".rigel",
    "target",
    "node_modules",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".turbo",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    size: u64,
}

/// One change the watcher saw, with the hash it saw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observed {
    pub path: String,
    pub sha256: Option<String>,
}

/// A file changed, and who (if anyone) was running when it did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attributed {
    pub path: String,
    pub sha256: Option<String>,
    /// Every agent that was mid-turn. Empty means the change is unattributed.
    pub agents: Vec<String>,
    /// More than one candidate writer: the UI must not pick one.
    pub ambiguous: bool,
}

/// A poll-based tree watcher. Cheap when nothing moves: it stats files and only hashes a file
/// whose size or mtime changed since the previous tick.
pub struct Watcher {
    root: PathBuf,
    stamps: BTreeMap<String, Stamp>,
    primed: bool,
    /// Set when the tree is too large to watch; the caller reports it instead of watching.
    pub overflowed: bool,
}

impl Watcher {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            stamps: BTreeMap::new(),
            primed: false,
            overflowed: false,
        }
    }

    /// Look once. The first call primes the baseline and reports nothing: files that already
    /// existed before Rigel started watching are not "changes".
    pub fn tick(&mut self) -> Vec<Observed> {
        let mut current: BTreeMap<String, Stamp> = BTreeMap::new();
        let mut count = 0usize;
        collect(&self.root, &self.root, &mut current, &mut count);
        if count > MAX_WATCHED_FILES {
            self.overflowed = true;
            return Vec::new();
        }

        let mut changes = Vec::new();
        for (path, stamp) in &current {
            let changed = match self.stamps.get(path) {
                Some(previous) => previous != stamp,
                None => self.primed,
            };
            if !changed {
                continue;
            }
            let sha256 = if stamp.size <= MAX_HASH_BYTES {
                std::fs::read(self.root.join(path))
                    .ok()
                    .map(|bytes| sha256_hex(&bytes))
            } else {
                None
            };
            changes.push(Observed {
                path: path.clone(),
                sha256,
            });
        }
        // A deleted file has no bytes to hash; it is still a change.
        for path in self.stamps.keys() {
            if !current.contains_key(path) {
                changes.push(Observed {
                    path: path.clone(),
                    sha256: None,
                });
            }
        }
        self.stamps = current;
        self.primed = true;
        changes
    }

    /// Attribute one batch. One running agent is named; several make it ambiguous; none leaves
    /// it unattributed rather than guessing.
    pub fn attribute(observed: &[Observed], running: &[Agent]) -> Vec<Attributed> {
        observed
            .iter()
            .map(|change| Attributed {
                path: change.path.clone(),
                sha256: change.sha256.clone(),
                agents: running.iter().map(|agent| agent.id.clone()).collect(),
                ambiguous: running.len() > 1,
            })
            .collect()
    }
}

fn collect(root: &Path, dir: &Path, into: &mut BTreeMap<String, Stamp>, count: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *count > MAX_WATCHED_FILES {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            collect(root, &path, into, count);
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        *count += 1;
        into.insert(
            relative,
            Stamp {
                modified: metadata.modified().ok(),
                size: metadata.len(),
            },
        );
    }
}

/// The running-turn watcher: every tick, record a conflict when a claimed path changed and the
/// only agent that could have written it is not the claimant.
pub struct ConflictWatcher {
    watcher: Watcher,
}

impl ConflictWatcher {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            watcher: Watcher::new(root),
        }
    }

    /// One poll. Returns the conflicts it recorded, if any.
    pub async fn step(
        &mut self,
        claims: &ClaimService,
        running: &[Agent],
    ) -> Result<Vec<ClaimConflict>, crate::claims::ClaimError> {
        let observed = self.watcher.tick();
        if observed.is_empty() || running.is_empty() {
            return Ok(Vec::new());
        }
        let attributed = Watcher::attribute(&observed, running);
        let mut recorded = Vec::new();
        for change in attributed {
            let held = claims.held(Some(&change.path)).await?;
            for claim in held {
                if change.agents.contains(&claim.agent_id) {
                    // The claimant was running: that is the reported case, not a violation.
                    continue;
                }
                if claim.base_hash.as_deref() == change.sha256.as_deref() {
                    continue;
                }
                let writer = if change.agents.len() == 1 {
                    Some(change.agents[0].as_str())
                } else {
                    None
                };
                recorded.push(
                    claims
                        .record_conflict(
                            &change.path,
                            Some(&claim.agent_id),
                            writer,
                            claim.base_hash.clone(),
                            change.sha256.clone(),
                        )
                        .await?,
                );
            }
        }
        Ok(recorded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClaimService, SessionService};

    async fn services() -> (
        tempfile::TempDir,
        ClaimService,
        SessionService,
        String,
        String,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("shared.txt"), "one\n").unwrap();
        let sessions = SessionService::open(tmp.path()).await.unwrap();
        let session = sessions
            .start(Some("attribution"), Some(100))
            .await
            .unwrap();
        let agent = sessions
            .register_agent(&session.id, "native", Some("writer"))
            .await
            .unwrap();
        let claims = ClaimService::new(sessions.pool().clone(), tmp.path());
        (tmp, claims, sessions, agent.id, session.id)
    }

    #[tokio::test]
    async fn the_first_tick_primes_and_later_ticks_report_the_change() {
        let (tmp, _claims, _sessions, _agent, _session) = services().await;
        let mut watcher = Watcher::new(tmp.path());
        assert!(watcher.tick().is_empty(), "the baseline is not a change");
        std::fs::write(tmp.path().join("shared.txt"), "two\n").unwrap();
        let changes = watcher.tick();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "shared.txt");
        assert_eq!(
            changes[0].sha256.as_deref(),
            Some(sha256_hex(b"two\n").as_str())
        );
        assert!(watcher.tick().is_empty(), "no change, no report");
    }

    #[tokio::test]
    async fn a_deleted_file_is_a_change_and_an_ignored_directory_is_not_watched() {
        let (tmp, _claims, _sessions, _agent, _session) = services().await;
        std::fs::create_dir_all(tmp.path().join("target")).unwrap();
        std::fs::write(tmp.path().join("target/junk"), "x").unwrap();
        let mut watcher = Watcher::new(tmp.path());
        watcher.tick();
        std::fs::write(tmp.path().join("target/junk"), "y").unwrap();
        assert!(watcher.tick().is_empty(), "target/ is not watched");
        std::fs::remove_file(tmp.path().join("shared.txt")).unwrap();
        let changes = watcher.tick();
        assert_eq!(changes[0].path, "shared.txt");
        assert!(changes[0].sha256.is_none());
    }

    #[tokio::test]
    async fn attribution_names_one_running_agent_and_flags_several() {
        let (_tmp, _claims, _sessions, agent, _session) = services().await;
        let observed = vec![Observed {
            path: "shared.txt".to_string(),
            sha256: Some("abc".to_string()),
        }];
        let made = Agent {
            id: agent.clone(),
            ..test_agent("writer")
        };
        let one = Watcher::attribute(&observed, std::slice::from_ref(&made));
        assert_eq!(one[0].agents, vec![agent.clone()]);
        assert!(!one[0].ambiguous);

        let two = Watcher::attribute(&observed, &[made, test_agent("other")]);
        assert_eq!(two[0].agents.len(), 2);
        assert!(two[0].ambiguous);

        let none = Watcher::attribute(&observed, &[]);
        assert!(none[0].agents.is_empty());
        assert!(!none[0].ambiguous);
    }

    #[tokio::test]
    async fn a_write_by_another_agent_becomes_a_conflict_row() {
        let (tmp, claims, sessions, agent, session) = services().await;
        claims
            .acquire(&agent, &["shared.txt".to_string()], "edit it")
            .await
            .unwrap();
        let other = sessions
            .register_agent(&session, "codex", Some("other"))
            .await
            .unwrap();

        let mut watcher = ConflictWatcher::new(tmp.path());
        watcher.step(&claims, &[]).await.unwrap();
        std::fs::write(tmp.path().join("shared.txt"), "written by someone else\n").unwrap();

        let running = vec![other.clone()];
        let recorded = watcher.step(&claims, &running).await.unwrap();
        assert_eq!(recorded.len(), 1, "{recorded:?}");
        assert_eq!(recorded[0].path, "shared.txt");
        assert_eq!(recorded[0].claimant.as_deref(), Some(agent.as_str()));
        assert_eq!(recorded[0].writer.as_deref(), Some(other.id.as_str()));
        // A second tick does not double-report an unchanged file.
        assert!(watcher.step(&claims, &running).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_conflict_surfaces_within_two_seconds_of_the_write() {
        let (tmp, claims, sessions, agent, session) = services().await;
        claims
            .acquire(&agent, &["shared.txt".to_string()], "edit it")
            .await
            .unwrap();
        let other = sessions
            .register_agent(&session, "codex", Some("other"))
            .await
            .unwrap();
        let running = vec![other.clone()];

        let mut watcher = ConflictWatcher::new(tmp.path());
        watcher.step(&claims, &[]).await.unwrap();
        let started = std::time::Instant::now();
        let writer = tokio::spawn({
            let root = tmp.path().to_path_buf();
            async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                std::fs::write(
                    root.join("shared.txt"),
                    "somebody else wrote this
",
                )
                .unwrap();
            }
        });

        let mut seen = None;
        while started.elapsed() < Duration::from_secs(2) {
            let recorded = watcher.step(&claims, &running).await.unwrap();
            if let Some(conflict) = recorded.into_iter().next() {
                seen = Some((conflict, started.elapsed()));
                break;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        writer.await.unwrap();

        let (conflict, elapsed) = seen.expect("a conflict within two seconds");
        assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
        assert_eq!(conflict.claimant.as_deref(), Some(agent.as_str()));
        assert_eq!(conflict.writer.as_deref(), Some(other.id.as_str()));
        assert!(conflict.base_hash != conflict.observed_hash);
    }

    fn test_agent(role: &str) -> Agent {
        Agent {
            id: format!("agent-{role}"),
            session_id: "session".to_string(),
            harness: "native".to_string(),
            role: Some(role.to_string()),
            model: None,
            worktree: None,
            status: "running".to_string(),
            started_at: 0,
            spend_cents: 0,
            spend_micros: 0,
            tokens_in: 0,
            tokens_out: 0,
        }
    }
}

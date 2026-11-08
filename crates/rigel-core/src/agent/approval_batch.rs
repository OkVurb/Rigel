//! Approval batching and blast radius (roadmap 64).
//!
//! The approval card groups related requests, but grouping is presentation only: every
//! request keeps its own scope, and an answer for one row cannot widen or approve another.
//! A row whose blast radius cannot be determined is always rendered on its own and is never
//! folded into a group.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum ScopeFacts {
    /// The resolver named every path and how many it would write.
    Known { paths: Vec<String>, writes: usize },
    /// The resolver could not determine the effect. This row is never collapsed.
    CannotDetermine { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub title: String,
    /// The exact scope token this request was approved under. Two requests about the same
    /// path with different scopes remain separate rows.
    pub scope: String,
    pub facts: ScopeFacts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub id: String,
    pub title: String,
    pub scope: String,
    pub paths: Vec<String>,
    pub writes: Option<usize>,
    pub cannot_determine: bool,
    pub note: Option<String>,
    pub approved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub scope: String,
    pub rows: Vec<Row>,
    pub resolved_paths: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub groups: Vec<Group>,
    pub rows: usize,
    pub cannot_determine: usize,
    pub resolved_paths: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    #[error("approval request ids must be unique; {0} appears twice")]
    DuplicateId(String),

    #[error("cannot approve {id}: it was issued under scope {expected:?}, not {given:?}")]
    ScopeWidened {
        id: String,
        expected: String,
        given: String,
    },

    #[error("no approval request with id {0}")]
    Unknown(String),

    #[error("a cannot-determine row cannot be approved through a batch; answer it on its own")]
    CannotDetermine(String),
}

#[derive(Debug, Clone)]
pub struct Batch {
    requests: Vec<Request>,
    approved: BTreeSet<String>,
}

impl Batch {
    pub fn new(requests: Vec<Request>) -> Result<Self, BatchError> {
        let mut seen = BTreeSet::new();
        for request in &requests {
            if !seen.insert(request.id.clone()) {
                return Err(BatchError::DuplicateId(request.id.clone()));
            }
        }
        Ok(Self {
            requests,
            approved: BTreeSet::new(),
        })
    }

    pub fn requests(&self) -> &[Request] {
        &self.requests
    }

    /// Group by scope without merging rows. A cannot-determine row is its own group even
    /// when another row shares its scope.
    pub fn card(&self) -> Card {
        let mut groups: BTreeMap<String, Group> = BTreeMap::new();
        let mut paths = BTreeSet::new();
        let mut cannot_determine = 0;
        for request in &self.requests {
            let (row_paths, writes, cannot, note, group_scope) = match &request.facts {
                ScopeFacts::Known { paths, writes } => (
                    paths.clone(),
                    Some(*writes),
                    false,
                    None,
                    request.scope.clone(),
                ),
                ScopeFacts::CannotDetermine { reason } => {
                    cannot_determine += 1;
                    (
                        Vec::new(),
                        None,
                        true,
                        Some(reason.clone()),
                        format!("{}:cannot-determine:{}", request.scope, request.id),
                    )
                }
            };
            for path in &row_paths {
                paths.insert(path.clone());
            }
            let row = Row {
                id: request.id.clone(),
                title: request.title.clone(),
                scope: request.scope.clone(),
                paths: row_paths,
                writes,
                cannot_determine: cannot,
                note,
                approved: self.approved.contains(&request.id),
            };
            groups
                .entry(group_scope.clone())
                .or_insert_with(|| Group {
                    scope: group_scope,
                    rows: Vec::new(),
                    resolved_paths: 0,
                })
                .rows
                .push(row);
        }
        for group in groups.values_mut() {
            let group_paths: BTreeSet<&String> =
                group.rows.iter().flat_map(|row| row.paths.iter()).collect();
            group.resolved_paths = group_paths.len();
        }
        Card {
            rows: self.requests.len(),
            cannot_determine,
            resolved_paths: paths.len(),
            groups: groups.into_values().collect(),
        }
    }

    pub fn is_approved(&self, id: &str) -> bool {
        self.approved.contains(id)
    }

    /// Approve one request. The scope token must match the one the request was issued
    /// under, so a broad "allow this turn" answer cannot approve a narrower or different
    /// row. A cannot-determine row must be answered on its own surface.
    pub fn approve(&mut self, id: &str, scope: &str) -> Result<(), BatchError> {
        let request = self
            .requests
            .iter()
            .find(|request| request.id == id)
            .ok_or_else(|| BatchError::Unknown(id.to_string()))?;
        if matches!(request.facts, ScopeFacts::CannotDetermine { .. }) {
            return Err(BatchError::CannotDetermine(id.to_string()));
        }
        if request.scope != scope {
            return Err(BatchError::ScopeWidened {
                id: id.to_string(),
                expected: request.scope.clone(),
                given: scope.to_string(),
            });
        }
        self.approved.insert(id.to_string());
        Ok(())
    }

    pub fn deny(&mut self, id: &str) -> Result<(), BatchError> {
        if !self.requests.iter().any(|request| request.id == id) {
            return Err(BatchError::Unknown(id.to_string()));
        }
        self.approved.remove(id);
        Ok(())
    }

    /// The exact scopes the user approved, for the transcript row.
    pub fn approved_scopes(&self) -> Vec<(String, String)> {
        self.requests
            .iter()
            .filter(|request| self.approved.contains(&request.id))
            .map(|request| (request.id.clone(), request.scope.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(id: &str, scope: &str, paths: &[&str], writes: usize) -> Request {
        Request {
            id: id.to_string(),
            title: format!("write {id}"),
            scope: scope.to_string(),
            facts: ScopeFacts::Known {
                paths: paths.iter().map(|path| path.to_string()).collect(),
                writes,
            },
        }
    }

    fn unknown(id: &str, scope: &str) -> Request {
        Request {
            id: id.to_string(),
            title: format!("run {id}"),
            scope: scope.to_string(),
            facts: ScopeFacts::CannotDetermine {
                reason: "the command builds its own paths".to_string(),
            },
        }
    }

    #[test]
    fn two_scopes_over_the_same_path_stay_separate() {
        let batch = Batch::new(vec![
            known("a", "once:src/lib.rs", &["src/lib.rs"], 1),
            known("b", "turn:src/lib.rs", &["src/lib.rs"], 1),
        ])
        .unwrap();
        let card = batch.card();
        assert_eq!(card.groups.len(), 2);
        assert_eq!(card.resolved_paths, 1);
        assert_eq!(card.rows, 2);
    }

    #[test]
    fn a_cannot_determine_row_is_never_collapsed() {
        let batch = Batch::new(vec![
            known("a", "turn", &["src/lib.rs"], 1),
            unknown("b", "turn"),
        ])
        .unwrap();
        let card = batch.card();
        assert_eq!(card.cannot_determine, 1);
        let row = card
            .groups
            .iter()
            .flat_map(|group| &group.rows)
            .find(|row| row.id == "b")
            .unwrap();
        assert!(row.cannot_determine);
        assert_eq!(
            row.note.as_deref(),
            Some("the command builds its own paths")
        );
        assert_eq!(row.writes, None);
    }

    #[test]
    fn one_approval_cannot_approve_another_scope() {
        let mut batch = Batch::new(vec![
            known("a", "once:src/lib.rs", &["src/lib.rs"], 1),
            known("b", "turn", &["src/other.rs"], 1),
        ])
        .unwrap();
        batch.approve("a", "once:src/lib.rs").unwrap();
        assert!(batch.is_approved("a"));
        assert!(!batch.is_approved("b"));
        assert!(matches!(
            batch.approve("b", "once:src/lib.rs"),
            Err(BatchError::ScopeWidened { .. })
        ));
        assert_eq!(
            batch.approved_scopes(),
            vec![("a".to_string(), "once:src/lib.rs".to_string())]
        );
    }

    #[test]
    fn a_cannot_determine_row_needs_its_own_surface() {
        let mut batch = Batch::new(vec![unknown("x", "turn")]).unwrap();
        assert!(matches!(
            batch.approve("x", "turn"),
            Err(BatchError::CannotDetermine(_))
        ));
        assert!(!batch.is_approved("x"));
    }

    #[test]
    fn duplicate_ids_are_refused_before_any_card_is_shown() {
        let requests = vec![known("a", "turn", &["a"], 1), known("a", "turn", &["b"], 1)];
        assert!(matches!(
            Batch::new(requests),
            Err(BatchError::DuplicateId(id)) if id == "a"
        ));
    }
}

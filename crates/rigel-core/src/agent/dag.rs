//! Subagent dependency graph (roadmap 42).
//!
//! `spawn_agents` can name each child and the siblings it waits for. A node starts only after
//! every prerequisite reached `done`; a failed, blocked or cancelled prerequisite blocks the
//! dependents instead of leaving a thread waiting forever. The graph is small (at most four
//! children per call), so it lives in memory for the turn and is mirrored into
//! `subagent_nodes` for the session view.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    Pending,
    Running,
    Done,
    Failed,
    Blocked,
    Cancelled,
}

impl NodeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "pending" => Self::Pending,
            "running" => Self::Running,
            "done" => Self::Done,
            "failed" => Self::Failed,
            "blocked" => Self::Blocked,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Done | Self::Failed | Self::Blocked | Self::Cancelled
        )
    }

    /// A dependency in one of these states can never become `done`.
    pub fn is_broken(self) -> bool {
        matches!(self, Self::Failed | Self::Blocked | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNode {
    pub id: String,
    pub role: String,
    pub depends_on: Vec<String>,
    pub state: NodeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DagEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DagError {
    Empty,
    Duplicate(String),
    Unknown { node: String, dependency: String },
    SelfDependency(String),
    Cycle(Vec<String>),
}

impl fmt::Display for DagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the graph has no nodes"),
            Self::Duplicate(id) => write!(f, "node id {id} is declared twice"),
            Self::Unknown { node, dependency } => {
                write!(
                    f,
                    "{node} depends on {dependency}, which is not in this call"
                )
            }
            Self::SelfDependency(id) => write!(f, "{id} cannot depend on itself"),
            Self::Cycle(ids) => write!(f, "the dependency graph has a cycle: {}", ids.join(" -> ")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitError {
    Blocked {
        dependency: String,
        state: NodeState,
        detail: Option<String>,
    },
    Cancelled,
}

impl fmt::Display for WaitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blocked {
                dependency,
                state,
                detail,
            } => write!(
                f,
                "dependency {dependency} ended {}; this node was not started{}",
                state_name(*state),
                detail
                    .as_deref()
                    .map(|detail| format!(": {detail}"))
                    .unwrap_or_default()
            ),
            Self::Cancelled => f.write_str("the parent cancelled before this node started"),
        }
    }
}

fn state_name(state: NodeState) -> &'static str {
    match state {
        NodeState::Pending => "pending",
        NodeState::Running => "running",
        NodeState::Done => "done",
        NodeState::Failed => "failed",
        NodeState::Blocked => "blocked",
        NodeState::Cancelled => "cancelled",
    }
}

#[derive(Debug)]
pub struct Dag {
    nodes: Mutex<Vec<DagNode>>,
    wake: Notify,
}

impl Dag {
    /// Validate and build a graph. A cycle, an unknown dependency or a duplicate id is refused
    /// before any child is registered, so a bad plan costs nothing.
    pub fn new(nodes: Vec<DagNode>) -> Result<Self, DagError> {
        if nodes.is_empty() {
            return Err(DagError::Empty);
        }
        let mut ids = HashSet::new();
        for node in &nodes {
            if !ids.insert(node.id.clone()) {
                return Err(DagError::Duplicate(node.id.clone()));
            }
        }
        for node in &nodes {
            for dependency in &node.depends_on {
                if dependency == &node.id {
                    return Err(DagError::SelfDependency(node.id.clone()));
                }
                if !ids.contains(dependency) {
                    return Err(DagError::Unknown {
                        node: node.id.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }
        // Kahn's algorithm: peel nodes with no remaining prerequisites. Whatever cannot be
        // peeled is in a cycle (or downstream of one).
        let mut indegree: HashMap<&str, usize> = nodes
            .iter()
            .map(|node| (node.id.as_str(), node.depends_on.len()))
            .collect();
        let mut queue: Vec<&str> = nodes
            .iter()
            .filter(|node| node.depends_on.is_empty())
            .map(|node| node.id.as_str())
            .collect();
        let mut visited = 0usize;
        while let Some(id) = queue.pop() {
            visited += 1;
            for node in &nodes {
                if node.depends_on.iter().any(|dependency| dependency == id)
                    && let Some(remaining) = indegree.get_mut(node.id.as_str())
                {
                    *remaining = remaining.saturating_sub(1);
                    if *remaining == 0 {
                        queue.push(node.id.as_str());
                    }
                }
            }
        }
        if visited != nodes.len() {
            let cycle = nodes
                .iter()
                .filter(|node| indegree.get(node.id.as_str()).copied().unwrap_or(0) > 0)
                .map(|node| node.id.clone())
                .collect();
            return Err(DagError::Cycle(cycle));
        }
        Ok(Self {
            nodes: Mutex::new(nodes),
            wake: Notify::new(),
        })
    }

    pub fn snapshot(&self) -> Vec<DagNode> {
        self.nodes
            .lock()
            .map(|nodes| nodes.clone())
            .unwrap_or_default()
    }

    pub fn edges(&self) -> Vec<DagEdge> {
        self.snapshot()
            .into_iter()
            .flat_map(|node| {
                node.depends_on.into_iter().map(move |from| DagEdge {
                    from,
                    to: node.id.clone(),
                })
            })
            .collect()
    }

    /// Dependencies before dependents. Stable within a level by declaration order.
    pub fn order(&self) -> Vec<String> {
        let nodes = self.snapshot();
        let mut remaining = nodes.clone();
        let mut ordered = Vec::new();
        while !remaining.is_empty() {
            let mut ready: Vec<DagNode> = remaining
                .iter()
                .filter(|node| {
                    node.depends_on
                        .iter()
                        .all(|dependency| ordered.contains(dependency))
                })
                .cloned()
                .collect();
            if ready.is_empty() {
                break;
            }
            ready.sort_by_key(|node| {
                nodes
                    .iter()
                    .position(|candidate| candidate.id == node.id)
                    .unwrap_or(usize::MAX)
            });
            for node in &ready {
                ordered.push(node.id.clone());
            }
            remaining.retain(|node| !ready.iter().any(|ready| ready.id == node.id));
        }
        ordered
    }

    pub fn set_running(&self, id: &str) {
        if let Ok(mut nodes) = self.nodes.lock()
            && let Some(node) = nodes.iter_mut().find(|node| node.id == id)
        {
            node.state = NodeState::Running;
        }
        self.wake.notify_waiters();
    }

    /// Record a terminal outcome. `ok` maps to `done`/`failed`; a failure wakes every dependent
    /// so it can block instead of waiting forever.
    pub fn finish(&self, id: &str, ok: bool, detail: Option<String>) {
        if let Ok(mut nodes) = self.nodes.lock()
            && let Some(node) = nodes.iter_mut().find(|node| node.id == id)
        {
            node.state = if ok {
                NodeState::Done
            } else {
                NodeState::Failed
            };
            node.detail = detail;
        }
        self.wake.notify_waiters();
    }

    /// Mark a dependent that never started because a prerequisite broke.
    pub fn block(&self, id: &str, detail: impl Into<String>) {
        if let Ok(mut nodes) = self.nodes.lock()
            && let Some(node) = nodes.iter_mut().find(|node| node.id == id)
        {
            node.state = NodeState::Blocked;
            node.detail = Some(detail.into());
        }
        self.wake.notify_waiters();
    }

    /// Mark every still-pending node cancelled. Used when the parent turn stops.
    pub fn cancel_pending(&self, detail: impl Into<String>) {
        let detail = detail.into();
        if let Ok(mut nodes) = self.nodes.lock() {
            for node in nodes
                .iter_mut()
                .filter(|node| matches!(node.state, NodeState::Pending))
            {
                node.state = NodeState::Cancelled;
                node.detail = Some(detail.clone());
            }
        }
        self.wake.notify_waiters();
    }

    /// Wait until every named prerequisite is `done`. A broken prerequisite is returned with the
    /// sibling name and its state, which is what the dependent reports to the parent.
    pub async fn wait_for(&self, id: &str, token: &CancellationToken) -> Result<(), WaitError> {
        loop {
            if token.is_cancelled() {
                return Err(WaitError::Cancelled);
            }
            {
                let nodes = match self.nodes.lock() {
                    Ok(nodes) => nodes,
                    Err(_) => return Err(WaitError::Cancelled),
                };
                let Some(node) = nodes.iter().find(|node| node.id == id) else {
                    return Err(WaitError::Blocked {
                        dependency: id.to_string(),
                        state: NodeState::Blocked,
                        detail: Some("the node is missing from its own graph".to_string()),
                    });
                };
                let mut all_done = true;
                for dependency in &node.depends_on {
                    let Some(prerequisite) = nodes.iter().find(|node| &node.id == dependency)
                    else {
                        return Err(WaitError::Blocked {
                            dependency: dependency.clone(),
                            state: NodeState::Blocked,
                            detail: Some("the prerequisite is missing from the graph".to_string()),
                        });
                    };
                    if prerequisite.state.is_broken() {
                        return Err(WaitError::Blocked {
                            dependency: dependency.clone(),
                            state: prerequisite.state,
                            detail: prerequisite.detail.clone(),
                        });
                    }
                    if prerequisite.state != NodeState::Done {
                        all_done = false;
                    }
                }
                if all_done {
                    return Ok(());
                }
            }
            tokio::select! {
                _ = self.wake.notified() => {}
                _ = token.cancelled() => return Err(WaitError::Cancelled),
                _ = tokio::time::sleep(Duration::from_millis(200)) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn node(id: &str, depends_on: &[&str]) -> DagNode {
        DagNode {
            id: id.to_string(),
            role: id.to_string(),
            depends_on: depends_on.iter().map(|id| id.to_string()).collect(),
            state: NodeState::Pending,
            detail: None,
        }
    }

    #[test]
    fn dependencies_come_before_dependents_and_cycles_are_refused() {
        let dag = Dag::new(vec![
            node("publish", &["review", "test"]),
            node("review", &["draft"]),
            node("test", &["draft"]),
            node("draft", &[]),
        ])
        .unwrap();
        assert_eq!(dag.order(), vec!["draft", "review", "test", "publish"]);
        assert_eq!(dag.edges().len(), 4);

        let cycle = Dag::new(vec![node("a", &["b"]), node("b", &["a"])]).unwrap_err();
        assert!(matches!(cycle, DagError::Cycle(_)));
        let unknown = Dag::new(vec![node("a", &["missing"])]).unwrap_err();
        assert!(matches!(unknown, DagError::Unknown { .. }));
        let duplicate = Dag::new(vec![node("a", &[]), node("a", &[])]).unwrap_err();
        assert!(matches!(duplicate, DagError::Duplicate(_)));
    }

    #[tokio::test]
    async fn a_child_waits_for_its_named_sibling_and_a_failure_blocks_dependents() {
        let dag = Arc::new(
            Dag::new(vec![
                node("first", &[]),
                node("second", &["first"]),
                node("third", &["second"]),
            ])
            .unwrap(),
        );
        let token = CancellationToken::new();

        let waiting = {
            let dag = dag.clone();
            let token = token.clone();
            tokio::spawn(async move { dag.wait_for("second", &token).await })
        };
        // Not ready while the prerequisite is pending, and still not ready when it only starts.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiting.is_finished());
        dag.set_running("first");
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiting.is_finished());

        dag.finish("first", true, None);
        assert_eq!(waiting.await.unwrap(), Ok(()));

        // A failure is named, and it blocks the transitive dependent as well.
        let third = {
            let dag = dag.clone();
            let token = token.clone();
            tokio::spawn(async move { dag.wait_for("third", &token).await })
        };
        dag.finish("second", false, Some("the review found a bug".to_string()));
        let error = third.await.unwrap().unwrap_err();
        match error {
            WaitError::Blocked {
                dependency,
                state,
                detail,
            } => {
                assert_eq!(dependency, "second");
                assert_eq!(state, NodeState::Failed);
                assert_eq!(detail.as_deref(), Some("the review found a bug"));
            }
            other => panic!("unexpected wait error: {other:?}"),
        }

        // Cancellation releases a waiter instead of leaving the thread parked.
        let pending = Dag::new(vec![node("alone", &[])]).unwrap();
        token.cancel();
        assert_eq!(
            pending.wait_for("alone", &token).await,
            Err(WaitError::Cancelled)
        );
    }
}

-- Roadmap 42: the parent's view of a subagent DAG. Rows are replaced whenever the graph moves,
-- so the session shows each node's prerequisites and terminal state without a sidecar file.
CREATE TABLE IF NOT EXISTS subagent_nodes (
  session_id     TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  parent_turn_id TEXT NOT NULL,
  node_id        TEXT NOT NULL,
  role           TEXT NOT NULL,
  depends_on_json TEXT NOT NULL,
  state          TEXT NOT NULL,
  detail         TEXT,
  updated_at     INTEGER NOT NULL,
  PRIMARY KEY (session_id, parent_turn_id, node_id)
);
CREATE INDEX IF NOT EXISTS subagent_nodes_by_session
  ON subagent_nodes (session_id, parent_turn_id);

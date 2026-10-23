-- The team board: which agent was given which files, by whom, and how far it got.
--
-- The coordinator (the "big boss") splits a job into file-scoped assignments. Native children
-- get an `agents` row; agents in the session chat room get a row here keyed by their room name
-- without an `agents` row, so `agent_key` is deliberately not a foreign key. `files_json` is a
-- JSON array of project-relative paths; the board is read as a whole, never queried by file.
CREATE TABLE IF NOT EXISTS team_agents (
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  agent_key  TEXT NOT NULL,
  role       TEXT,
  kind       TEXT NOT NULL CHECK (kind IN ('native','room')),
  files_json TEXT NOT NULL,
  task       TEXT NOT NULL,
  status     TEXT NOT NULL CHECK (status IN ('assigned','working','done','failed')),
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (session_id, agent_key)
);

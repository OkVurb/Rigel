-- D92: a task may carry its own cap below the session cap. Keyed by the agent that runs the
-- task, because a spawned child is the unit a task is assigned to; removing the agent drops it.
CREATE TABLE IF NOT EXISTS task_caps (
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  agent_id   TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  cap_micros INTEGER NOT NULL CHECK (cap_micros > 0),
  set_at     INTEGER NOT NULL,
  PRIMARY KEY (session_id, agent_id)
);

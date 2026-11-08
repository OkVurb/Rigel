-- Roadmap 37/40: recovery is a database decision, not a sidecar file.
--
-- A tombstone is authoritative: a discarded turn stays in the transcript for forensics, but
-- its id can never be resumed or treated as live. A paused turn carries the exact messages the
-- coordinator had built and the budget/step remainder so reopening the window continues the
-- same request instead of rebuilding a different one.
CREATE TABLE turn_tombstones (
  turn_id      TEXT PRIMARY KEY REFERENCES turns(id) ON DELETE CASCADE,
  session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  reason       TEXT NOT NULL,
  discarded_at INTEGER NOT NULL
);
CREATE INDEX turn_tombstones_session ON turn_tombstones(session_id, discarded_at);

CREATE TABLE paused_turns (
  turn_id           TEXT PRIMARY KEY REFERENCES turns(id) ON DELETE CASCADE,
  session_id        TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  agent_id          TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  messages_json     TEXT NOT NULL,
  remaining_steps   INTEGER NOT NULL,
  remaining_micros  INTEGER,
  paused_at         INTEGER NOT NULL,
  resumed_at        INTEGER
);
CREATE INDEX paused_turns_session ON paused_turns(session_id, paused_at);

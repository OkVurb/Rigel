-- D98: a session can be in plan mode. The phase is per session, not per turn, so a reload
-- does not silently turn the guard off. `approved_at` records the explicit hand-off to work.
CREATE TABLE IF NOT EXISTS session_plan (
  session_id  TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
  mode        INTEGER NOT NULL DEFAULT 0 CHECK (mode IN (0, 1)),
  approved_at INTEGER,
  updated_at  INTEGER NOT NULL
);

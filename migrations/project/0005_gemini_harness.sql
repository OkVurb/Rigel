-- Gemini CLI joins the roster as a fifth external harness.
--
-- SQLite cannot alter a CHECK constraint in place, so `agents` is rebuilt with the documented
-- table-rebuild procedure: create the new shape, copy every row, drop the old table, rename,
-- recreate the index. `migrate::apply` turns foreign-key enforcement off on the migration
-- connection for the duration of the run, because `DROP TABLE agents` would otherwise cascade
-- into `turns` and `claims` through their `ON DELETE CASCADE` edges and empty the transcript.
--
-- The column list is explicit: `INSERT ... SELECT *` would silently shift once a future
-- migration appends another column, and this file has to keep working after that happens.
CREATE TABLE agents_new (
  id           TEXT PRIMARY KEY,
  session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  harness      TEXT NOT NULL CHECK (harness IN ('native','codex','claude','cursor','cline','gemini')),
  parent_id    TEXT REFERENCES agents(id),
  role         TEXT,
  model        TEXT,
  pid          INTEGER,
  worktree     TEXT,
  status       TEXT NOT NULL,
  started_at   INTEGER NOT NULL,
  ended_at     INTEGER,
  spend_cents  INTEGER NOT NULL DEFAULT 0,
  tokens_in    INTEGER NOT NULL DEFAULT 0,
  tokens_out   INTEGER NOT NULL DEFAULT 0,
  spend_micros INTEGER NOT NULL DEFAULT 0
);

INSERT INTO agents_new (
  id, session_id, harness, parent_id, role, model, pid, worktree, status,
  started_at, ended_at, spend_cents, tokens_in, tokens_out, spend_micros
)
SELECT
  id, session_id, harness, parent_id, role, model, pid, worktree, status,
  started_at, ended_at, spend_cents, tokens_in, tokens_out, spend_micros
FROM agents;

DROP TABLE agents;
ALTER TABLE agents_new RENAME TO agents;
CREATE INDEX agents_session ON agents(session_id, started_at);

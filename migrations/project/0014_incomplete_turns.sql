-- Q248: a turn whose provider stream died after delivering output is neither a completed
-- answer nor a failure with nothing behind it. It is closed with status `incomplete` and the
-- tokens that arrived are billed.
--
-- SQLite cannot alter a CHECK constraint in place, so `turns` is rebuilt with the same
-- procedure as 0005: create the new shape, copy every row, drop the old table, rename, and
-- recreate the index the dropped table owned. `migrate::apply` runs the connection with
-- foreign-key enforcement off, so `DROP TABLE turns` does not cascade into `items`; the
-- `foreign_key_check` before commit proves nothing was orphaned.
--
-- The column list is explicit so a later column cannot silently shift the copy.
CREATE TABLE turns_new (
  id             TEXT PRIMARY KEY,
  agent_id       TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  seq            INTEGER NOT NULL,
  status         TEXT NOT NULL CHECK (status IN ('running','completed','failed','cancelled','incomplete')),
  prompt         TEXT,
  started_at     INTEGER NOT NULL,
  completed_at   INTEGER,
  usage_json     TEXT,
  stop_reason    TEXT,
  parent_turn_id TEXT,
  UNIQUE (agent_id, seq)
);

INSERT INTO turns_new (
  id, agent_id, seq, status, prompt, started_at, completed_at, usage_json,
  stop_reason, parent_turn_id
)
SELECT
  id, agent_id, seq, status, prompt, started_at, completed_at, usage_json,
  stop_reason, parent_turn_id
FROM turns;

DROP TABLE turns;
ALTER TABLE turns_new RENAME TO turns;
CREATE INDEX IF NOT EXISTS idx_turns_parent ON turns(parent_turn_id);

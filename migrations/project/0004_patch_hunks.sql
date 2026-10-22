-- Per-hunk attribution (P5.6).
--
-- A patch already names its agent; a *hunk* has to as well, because several agents touch one
-- file and "who wrote these lines" is the question the merged diff exists to answer. The diff
-- text stays the record; this table is its index: one row per `@@` header, in the order the
-- header appears, with the agent that proposed it.
--
-- Rows are written in the same step as the `patches` row, so a patch that exists has its
-- hunks. A patch reverted later keeps them: attribution is history, not current ownership.
CREATE TABLE patch_hunks (
  id         TEXT PRIMARY KEY,
  patch_id   TEXT NOT NULL REFERENCES patches(id) ON DELETE CASCADE,
  path       TEXT NOT NULL,
  ordinal    INTEGER NOT NULL,
  agent_id   TEXT NOT NULL,
  header     TEXT NOT NULL,
  old_start  INTEGER NOT NULL,
  old_lines  INTEGER NOT NULL,
  new_start  INTEGER NOT NULL,
  new_lines  INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  UNIQUE (patch_id, ordinal)
);
CREATE INDEX patch_hunks_path ON patch_hunks(path, created_at);

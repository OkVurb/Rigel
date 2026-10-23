-- Hunk comments (D88).
--
-- A comment attaches to one `patch_hunks` row, names the agent that wrote that hunk, and is
-- delivered to that agent as a turn. The patch stays read-only: a comment never edits a diff,
-- it sends a sentence back to the author and records that it was sent.
--
-- `delivered_turn_id` is the proof of delivery: a comment without one is pending, and the
-- pending rows are what the roster shows next to the agent that still owes an answer.
CREATE TABLE hunk_comments (
  id               TEXT PRIMARY KEY,
  hunk_id          TEXT NOT NULL REFERENCES patch_hunks(id) ON DELETE CASCADE,
  patch_id         TEXT NOT NULL,
  path             TEXT NOT NULL,
  header           TEXT NOT NULL,
  -- The hunk's author: who the comment is for.
  agent_id         TEXT NOT NULL,
  -- Who wrote the comment: `user`, or an agent id. Kept separate from `agent_id` so a
  -- reviewer agent's note is not mistaken for the author's own.
  author           TEXT NOT NULL,
  body             TEXT NOT NULL,
  created_at       INTEGER NOT NULL,
  delivered_turn_id TEXT,
  delivered_at     INTEGER
);
CREATE INDEX hunk_comments_hunk ON hunk_comments(hunk_id, created_at);
CREATE INDEX hunk_comments_pending ON hunk_comments(agent_id, delivered_at);

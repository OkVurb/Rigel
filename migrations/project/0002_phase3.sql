-- Phase 3 — the bill, session pins and crash recovery.
--
-- Why micro-dollars: `usage.cost` from a provider is a float in USD, and one cheap turn can
-- cost less than a cent. Rounding every turn to `spend_cents` would make a $0.25 cap
-- meaningless (ten turns at $0.004 each would read as $0). `spend_micros` is the accounting
-- column (1 micro-dollar = $0.000001); `spend_cents` stays for display and is derived, not
-- separately incremented.
ALTER TABLE sessions ADD COLUMN spend_micros INTEGER NOT NULL DEFAULT 0;
ALTER TABLE agents ADD COLUMN spend_micros INTEGER NOT NULL DEFAULT 0;

-- A session binds each skill it loads to the body hash it saw first (§8.7). Loading a skill
-- again returns that version even after the index updates; changing the pin is an explicit
-- user action, not something a re-index can do silently.
CREATE TABLE session_skills (
  session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  skill_id    TEXT NOT NULL,
  body_sha    TEXT NOT NULL,
  pinned_at   INTEGER NOT NULL,
  PRIMARY KEY (session_id, skill_id)
);

-- The patch journal lives on disk because it must survive a process kill; this table only
-- records what was decided. `stop_reason` is the turn's own account of how it ended, which
-- the replay pane shows next to `usage`.
ALTER TABLE turns ADD COLUMN stop_reason TEXT;

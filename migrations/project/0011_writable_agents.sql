-- D80: a spawned subagent may write, in its own worktree, and its work only lands after a
-- review. D113's cap, D114's plan gate and D116's daily cap need their own small tables.
--
-- `agents.writable` is the durable half of the concurrency cap: a writable child is counted
-- while it runs, so two fan-outs in the same session cannot each start three. `plan_json` is
-- the plan the user approved for this agent; `plan_note` records a skipped gate (who skipped
-- it and why), so a later reader can tell approval from waiver.
ALTER TABLE agents ADD COLUMN writable INTEGER NOT NULL DEFAULT 0;
ALTER TABLE agents ADD COLUMN plan_json TEXT;
ALTER TABLE agents ADD COLUMN plan_note TEXT;

-- D115: one row per attempt to merge a writable agent, whatever the verdict. The diff hash is
-- the identity of what was reviewed: a file that changed after the review cannot ride on it.
CREATE TABLE IF NOT EXISTS merge_reviews (
  id             TEXT PRIMARY KEY,
  session_id     TEXT NOT NULL,
  agent_id       TEXT NOT NULL,
  reviewer_model TEXT NOT NULL,
  verdict        TEXT NOT NULL CHECK (verdict IN ('approved','blocked','unavailable')),
  reason         TEXT,
  diff_sha       TEXT NOT NULL,
  files_json     TEXT NOT NULL,
  override_note  TEXT,
  merged         INTEGER NOT NULL DEFAULT 0,
  created_at     INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS merge_reviews_agent ON merge_reviews(agent_id, created_at);

-- D116: spend per UTC day, incremented in the same transaction as `sessions.spend_micros`.
-- The ledger is the project database, so "global" means every session in this project.
CREATE TABLE IF NOT EXISTS daily_spend (
  day    TEXT PRIMARY KEY,
  micros INTEGER NOT NULL DEFAULT 0
);

-- One row of defaults. A NULL `daily_cap_micros` turns the cap off; the default is $20.
CREATE TABLE IF NOT EXISTS safety_settings (
  id              INTEGER PRIMARY KEY CHECK (id = 1),
  writable_agents INTEGER NOT NULL DEFAULT 3,
  daily_cap_micros INTEGER
);
INSERT INTO safety_settings (id, writable_agents, daily_cap_micros)
VALUES (1, 3, 20000000);

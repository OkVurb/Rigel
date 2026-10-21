-- Subagents — a parent turn can own child turns.
--
-- A subagent's spend must be attributable: `budget::reconcile` sums `turns.usage_json` per
-- agent, so a child needs its own agent row and its own turn. `parent_turn_id` is what keeps
-- those turns out of the user's top-level transcript without losing them from replay.
ALTER TABLE turns ADD COLUMN parent_turn_id TEXT;

CREATE INDEX IF NOT EXISTS idx_turns_parent ON turns(parent_turn_id);

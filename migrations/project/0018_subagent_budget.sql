-- Roadmap 44: the session tree shows each child's depth and what is left of the allowance it
-- inherited. Existing rows default to depth 1 with an unknown (NULL) allowance.
ALTER TABLE subagent_nodes ADD COLUMN depth INTEGER NOT NULL DEFAULT 1;
ALTER TABLE subagent_nodes ADD COLUMN budget_micros INTEGER;
ALTER TABLE subagent_nodes ADD COLUMN spent_micros INTEGER NOT NULL DEFAULT 0;
ALTER TABLE subagent_nodes ADD COLUMN remaining_micros INTEGER;

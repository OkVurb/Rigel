-- Roadmap 41: an approved plan names the tools the agent may use without asking again.
-- The proposal is kept after approval so the session can show what was agreed; turning plan
-- mode back on clears it, and an outside tool clears `approved_at` while keeping the proposal
-- so the window can ask for a fresh approval.
ALTER TABLE session_plan ADD COLUMN plan_text TEXT;
ALTER TABLE session_plan ADD COLUMN tools_json TEXT;
ALTER TABLE session_plan ADD COLUMN proposed_at INTEGER;

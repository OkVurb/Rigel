-- 0003: what Phase 6's detection, installs and hash pins need.
--
-- 0001 gave `tool_status` a version and a timestamp, which was all a one-off probe needed.
-- A cached answer now also has to say *where* the binary is and *which* bytes it was, so a
-- later probe can notice that the file underneath changed (P6.7). `install_ledger` records
-- the host it ran on, how long it took and whether it started at all; a row with `decision`
-- and no `executed_at` is a queue entry, which is what unattended mode leaves behind (D13).

ALTER TABLE tool_status ADD COLUMN path TEXT;
ALTER TABLE tool_status ADD COLUMN sha256 TEXT;
ALTER TABLE tool_status ADD COLUMN present INTEGER NOT NULL DEFAULT 0;

ALTER TABLE install_ledger ADD COLUMN host TEXT;
ALTER TABLE install_ledger ADD COLUMN duration_ms INTEGER;
ALTER TABLE install_ledger ADD COLUMN verification TEXT;
ALTER TABLE install_ledger ADD COLUMN error TEXT;

CREATE INDEX IF NOT EXISTS install_ledger_decision ON install_ledger(decision);

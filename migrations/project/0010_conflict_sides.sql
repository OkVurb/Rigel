-- The three sides of a claim conflict (D79).
--
-- P5.4 recorded two hashes, and a hash cannot be read: the Diff view could say that two versions
-- differed but not how. This migration keeps the text behind each hash.
--
-- `claims.base_text` is captured when the claim is taken, because that is the one moment the file
-- on disk *is* the version the claimant started from. `conflict_sides` then stores the three
-- versions as they were at detection. Only `theirs` is read from disk after the fact; `base` comes
-- from the claim, and `ours` is rebuilt by walking the claimant's own applied patches forward from
-- that base. A rebuilt side is kept only when its bytes hash to the hash the ledger recorded; a
-- side that cannot be rebuilt stays NULL and `note` says which one and why — an empty pane with an
-- explanation, never an invented one.
--
-- `merged_patch_id` is the proof that a resolution wrote a file: it points at the patch that
-- carried the merged text through the same compare-and-swap path as any agent write.
ALTER TABLE claims ADD COLUMN base_text TEXT;

CREATE TABLE conflict_sides (
  conflict_id     TEXT PRIMARY KEY REFERENCES claim_conflicts(id) ON DELETE CASCADE,
  base_text       TEXT,
  base_hash       TEXT,
  ours_text       TEXT,
  ours_hash       TEXT,
  theirs_text     TEXT,
  theirs_hash     TEXT,
  -- Why a side is missing, when one is.
  note            TEXT,
  merged_patch_id TEXT,
  merged_hash     TEXT,
  captured_at     INTEGER NOT NULL
);

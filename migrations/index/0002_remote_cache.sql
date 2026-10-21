-- 0002: everything Phase 2's remote sources need that §6.3's 0001 does not carry.
--
-- Three changes, each with a reason:
--
-- 1. `blobs.bytes` is dropped. §6.1 puts skill bodies on disk at `blobs\<sha256>` and P2.3
--    forbids storing them in SQLite, so the table becomes a manifest of what is on disk
--    (sha, size, created_at). `rigel gc` walks the directory and reconciles against it.
-- 2. `http_cache` records one row per URL fetched: the ETag/Last-Modified to revalidate
--    with, the blob holding the last body, and when it was last checked. This is what makes
--    a re-index cheaper than a first index and what lets the shelf work offline.
-- 3. `skills.capabilities_json` and `skills.simhash` hold the static-review inventory and the
--    64-bit content fingerprint. §6.3 has a column for neither, and both have to survive a
--    restart: the grade alone cannot show evidence, and clustering cannot re-fetch 300 bodies
--    just to decide which rows are near-duplicates.

ALTER TABLE blobs DROP COLUMN bytes;
ALTER TABLE blobs ADD COLUMN size INTEGER NOT NULL DEFAULT 0;

CREATE TABLE http_cache (
  url           TEXT PRIMARY KEY,
  etag          TEXT,
  last_modified TEXT,
  sha256        TEXT,                -- blob holding the last successful body
  status        INTEGER,             -- last HTTP status seen
  fetched_at    INTEGER NOT NULL,    -- when the body was last replaced
  checked_at    INTEGER NOT NULL     -- when we last confirmed it is current
);

CREATE INDEX http_cache_checked ON http_cache(checked_at);

ALTER TABLE skills ADD COLUMN capabilities_json TEXT;
ALTER TABLE skills ADD COLUMN simhash TEXT;        -- 16 hex digits, NULL until fingerprinted

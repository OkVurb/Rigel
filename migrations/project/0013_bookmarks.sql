-- D150: a star on a transcript item or a patch hunk, kept per session. The label is a snapshot
-- so the inspector can list a bookmark without joining a turn, an item and a patch on every
-- read; the target id is what a jump uses. One row per target: starring twice clears the star.
CREATE TABLE IF NOT EXISTS bookmarks (
  id         TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL CHECK (kind IN ('item', 'hunk')),
  target_id  TEXT NOT NULL,
  label      TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  UNIQUE (session_id, kind, target_id)
);

CREATE INDEX IF NOT EXISTS bookmarks_session ON bookmarks(session_id, created_at);

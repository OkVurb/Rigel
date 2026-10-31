-- D148: the window had no session list, so a week of work was reachable only by its newest
-- session. The rows already existed; these two nullable columns are what the list needs to be
-- managed without deleting anything.
--
-- `pinned_at` moves a session to the top of the list and keeps it there. `archived_at` hides a
-- session from the default list; the row, its transcript and its patches are untouched, so an
-- archived session is still searchable and still resumable. `title` already existed and stays
-- nullable: no title means the transcript's opening prompt is the name.
ALTER TABLE sessions ADD COLUMN pinned_at INTEGER;
ALTER TABLE sessions ADD COLUMN archived_at INTEGER;

-- The list reads pinned sessions first and then the newest, so the pin column carries an index.
-- SQLite cannot index an expression without repeating it in every query, and the list is small
-- enough that the created_at sort stays a scan; only the pin lookup is worth a structure.
CREATE INDEX IF NOT EXISTS sessions_pinned ON sessions(pinned_at);

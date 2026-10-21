-- Global skill index. Lives in %APPDATA%\Rigel\index.db and never enters a project.
-- Bodies are content-addressed blobs on disk; only metadata and the search index live here.

CREATE TABLE sources (
  id           TEXT PRIMARY KEY,     -- 'github' | 'skillsmp' | 'skillhub' | 'local'
  base_url     TEXT,
  last_sync_at INTEGER,
  status       TEXT,                 -- 'ok' | 'stale' | 'broken'
  error        TEXT
);

CREATE TABLE skills (
  id            TEXT PRIMARY KEY,    -- source:repo:path  |  source:slug
  name          TEXT NOT NULL,
  description   TEXT,
  source        TEXT NOT NULL,
  repo          TEXT,
  path          TEXT,
  url           TEXT,
  body_sha      TEXT,
  grade         TEXT,                -- A..F, NULL = unscanned
  grade_scope   TEXT,                -- 'frontmatter+body' | 'file-only'
  license       TEXT,
  stars         INTEGER,
  installs      INTEGER,
  tokens        INTEGER,
  cluster_id    TEXT,
  first_seen_at INTEGER,
  last_seen_at  INTEGER
);
CREATE INDEX skills_cluster ON skills(cluster_id);
CREATE INDEX skills_source  ON skills(source, last_seen_at);

CREATE VIRTUAL TABLE skills_fts USING fts5(
  name, description, content='skills', content_rowid='rowid'
);

-- External-content FTS5 needs explicit synchronisation.
CREATE TRIGGER skills_ai AFTER INSERT ON skills BEGIN
  INSERT INTO skills_fts(rowid, name, description)
  VALUES (new.rowid, new.name, new.description);
END;

CREATE TRIGGER skills_ad AFTER DELETE ON skills BEGIN
  INSERT INTO skills_fts(skills_fts, rowid, name, description)
  VALUES ('delete', old.rowid, old.name, old.description);
END;

CREATE TRIGGER skills_au AFTER UPDATE ON skills BEGIN
  INSERT INTO skills_fts(skills_fts, rowid, name, description)
  VALUES ('delete', old.rowid, old.name, old.description);
  INSERT INTO skills_fts(rowid, name, description)
  VALUES (new.rowid, new.name, new.description);
END;

CREATE TABLE skill_versions (
  skill_id   TEXT NOT NULL,
  body_sha   TEXT NOT NULL,
  fetched_at INTEGER NOT NULL,
  PRIMARY KEY (skill_id, body_sha)
);

CREATE TABLE blobs (
  sha256     TEXT PRIMARY KEY,
  bytes      BLOB NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE tools (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  category      TEXT,
  provides_json TEXT,
  commands_json TEXT,
  docs          TEXT,
  size_hint     TEXT
);

CREATE TABLE tool_status (
  tool_id    TEXT NOT NULL,
  host       TEXT NOT NULL,           -- 'windows' | 'wsl:<distro>' | 'darwin' | 'linux'
  version    TEXT,
  checked_at INTEGER,
  PRIMARY KEY (tool_id, host)
);

CREATE TABLE install_ledger (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  package_id    TEXT,
  command       TEXT NOT NULL,
  trust_score   REAL,
  evidence_json TEXT,
  decision      TEXT NOT NULL,       -- 'auto' | 'user-approved' | 'denied' | 'queued'
  executed_at   INTEGER,
  exit_code     INTEGER
);

CREATE TABLE evidence (
  skill_id    TEXT NOT NULL,
  metric      TEXT NOT NULL,
  measured    REAL,
  advertised  REAL,
  source      TEXT,
  source_url  TEXT,
  observed_at INTEGER,
  PRIMARY KEY (skill_id, metric, source)
);

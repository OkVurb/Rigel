-- Project session database. Lives in <repo>/.rigel/rigel.db and is never committed.
-- The migration runner creates schema_migrations before applying anything; IF NOT EXISTS
-- keeps this file runnable on its own (for inspection with sqlite3).

CREATE TABLE IF NOT EXISTS schema_migrations (
  version    INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  applied_at INTEGER NOT NULL
);

CREATE TABLE sessions (
  id             TEXT PRIMARY KEY,
  project_root   TEXT NOT NULL,
  title          TEXT,
  status         TEXT NOT NULL CHECK (status IN ('active','idle','interrupted','ended')),
  budget_cents   INTEGER,
  spend_cents    INTEGER NOT NULL DEFAULT 0,
  interventions  INTEGER NOT NULL DEFAULT 0,
  tasks_done     INTEGER NOT NULL DEFAULT 0,
  created_at     INTEGER NOT NULL,
  ended_at       INTEGER
);

CREATE TABLE agents (
  id           TEXT PRIMARY KEY,
  session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  harness      TEXT NOT NULL CHECK (harness IN ('native','codex','claude','cursor','cline')),
  parent_id    TEXT REFERENCES agents(id),
  role         TEXT,
  model        TEXT,
  pid          INTEGER,
  worktree     TEXT,
  status       TEXT NOT NULL,
  started_at   INTEGER NOT NULL,
  ended_at     INTEGER,
  spend_cents  INTEGER NOT NULL DEFAULT 0,
  tokens_in    INTEGER NOT NULL DEFAULT 0,
  tokens_out   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX agents_session ON agents(session_id, started_at);

CREATE TABLE turns (
  id           TEXT PRIMARY KEY,
  agent_id     TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  seq          INTEGER NOT NULL,
  status       TEXT NOT NULL CHECK (status IN ('running','completed','failed','cancelled')),
  prompt       TEXT,
  started_at   INTEGER NOT NULL,
  completed_at INTEGER,
  usage_json   TEXT,
  UNIQUE (agent_id, seq)
);

CREATE TABLE items (
  id         TEXT PRIMARY KEY,
  turn_id    TEXT NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
  seq        INTEGER NOT NULL,
  kind       TEXT NOT NULL,
  status     TEXT NOT NULL,
  body_json  TEXT NOT NULL,
  raw_json   TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  UNIQUE (turn_id, seq)
);
CREATE INDEX items_kind ON items(kind);

CREATE TABLE claims (
  id             TEXT PRIMARY KEY,
  path           TEXT NOT NULL,
  agent_id       TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  base_hash      TEXT,
  acquired_at    INTEGER NOT NULL,
  heartbeat_at   INTEGER NOT NULL,
  released_at    INTEGER,
  release_reason TEXT
);
CREATE UNIQUE INDEX claims_active ON claims(path) WHERE released_at IS NULL;

CREATE TABLE claim_conflicts (
  id               TEXT PRIMARY KEY,
  path             TEXT NOT NULL,
  claimant_id      TEXT,
  writer_id        TEXT,
  base_hash        TEXT,
  observed_hash    TEXT,
  detected_at      INTEGER NOT NULL,
  resolved_at      INTEGER,
  resolution       TEXT
);

CREATE TABLE patches (
  id          TEXT PRIMARY KEY,
  session_id  TEXT NOT NULL,
  agent_id    TEXT NOT NULL,
  path        TEXT NOT NULL,
  base_hash   TEXT NOT NULL,
  result_hash TEXT NOT NULL,
  diff        TEXT NOT NULL,
  status      TEXT NOT NULL CHECK (status IN ('proposed','applied','conflicted','reverted')),
  created_at  INTEGER NOT NULL,
  applied_at  INTEGER,
  reverted_at INTEGER
);

CREATE TABLE approvals (
  id           TEXT PRIMARY KEY,
  agent_id     TEXT NOT NULL,
  turn_id      TEXT,
  kind         TEXT NOT NULL,
  request_json TEXT NOT NULL,
  decision     TEXT,
  scope        TEXT,
  requested_at INTEGER NOT NULL,
  decided_at   INTEGER
);

CREATE TABLE interventions (
  id          TEXT PRIMARY KEY,
  session_id  TEXT NOT NULL,
  kind        TEXT NOT NULL,
  detail_json TEXT,
  at          INTEGER NOT NULL
);

CREATE TABLE checkpoints (
  id           TEXT PRIMARY KEY,
  session_id   TEXT NOT NULL,
  turn_id      TEXT,
  tree_hash    TEXT NOT NULL,
  journal_json TEXT NOT NULL,
  created_at   INTEGER NOT NULL
);

CREATE TABLE memory_index (
  id           TEXT PRIMARY KEY,
  path         TEXT NOT NULL,
  sha256       TEXT NOT NULL,
  updated_at   INTEGER NOT NULL,
  words        INTEGER NOT NULL
);

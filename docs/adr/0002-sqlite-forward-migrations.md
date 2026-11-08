# 0002: SQLite with forward-only migrations

Status: accepted. Date: 2026-11-06.

## Context

Rigel is a local-first desktop app. Sessions, transcripts, spend, approvals, patches and the
skill index must survive an upgrade on a machine with no database administrator and no network.

## Decision

The local store is SQLite in WAL mode, with numbered migrations applied in order at startup and
a schema test that replays every prefix. Migrations are forward-only: a build refuses to open a
database newer than the schema it knows rather than guessing at a downgrade. Full-text search uses
SQLite's FTS5 tables, and a migration rehearsal upgrades every prefix to the current schema.

## Alternatives considered

- PostgreSQL. Rejected: it adds a server to a desktop install and breaks the local-first promise.
- An embedded key-value store. Rejected: transcripts and spend need joins, indexes and ad-hoc
  inspection that SQLite already provides.
- JSON files per session. Rejected: no transactions, no schema history, and partial writes lose
  a session.

## Consequences

There is one writer per process and no multi-writer server. Upgrades are tested by replaying
prefixes, and a failed migration must be fixed at the source rather than skipped. Backups remain
the user's responsibility, and the database is not encrypted at rest.

## Reopen if

Concurrent writes from multiple devices or processes become a product requirement that local WAL
cannot satisfy, or a hosted database becomes the source of truth for team workspaces.

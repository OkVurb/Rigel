# 0004: Opt-in cloud on the user's Firebase project

Status: accepted. Date: 2026-11-06.

## Context

Phone sync is useful, but a coding transcript can contain proprietary code and prompts. A
multi-tenant hosted backend would make Rigel the data controller and create a breach surface the
team does not operate.

## Decision

Cloud is off by default and runs against a Firebase project the user configures. Only a session
document containing the title, prompts and assistant text is published, and only on **Publish to
phone** or when automatic publishing is explicitly enabled. Tool calls, command output, file
changes and project paths are excluded. Firestore rules scope documents to the authenticated user
id. Credentials live in the OS keychain, and deleting a row writes a tombstone.

## Alternatives considered

- A hosted multi-tenant sync service. Rejected: it centralises every user's transcript and makes
  retention a Rigel obligation.
- No cloud at all. Rejected: phone visibility is a requested workflow, and opt-in keeps the
  default local.
- Object storage with presigned URLs. Rejected: it syncs bytes but not per-user access rules,
  deletion tombstones or account identity.

## Consequences

Rigel sends no telemetry and operates no cloud database. The user owns retention, backups and
account deletion in their Firebase project. Cloud features require the user to create a project,
and team sharing is not available.

## Reopen if

Team workspaces require shared tenancy and an organisation-level retention policy, which cannot
be expressed by per-user Firebase rules.

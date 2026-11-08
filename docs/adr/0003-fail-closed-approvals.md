# 0003: File-backed, fail-closed approvals

Status: accepted. Date: 2026-11-06.

## Context

A command, write, network fetch, mail send or skill script may be initiated by the agent, but a
human must be able to allow or refuse it. The window that shows the request can crash, lose its
connection or be replaced while the request is pending.

## Decision

Approvals travel over a file-backed bus with typed requests and decisions. A request carries the
exact command text, working directory and timeout (or the exact path and content hash for a
write). Scopes are once, session, project and always. A timeout, an unanswered request or a
surface error is a refusal; only an explicit affirmative decision allows the action. The
coordinator records the decision with the turn so the transcript shows what was approved.

## Alternatives considered

- An in-process channel between the coordinator and the UI. Rejected: a reload or a second window
  drops the request and leaves the agent waiting forever.
- An allowlist of "safe" commands that never asks. Rejected: the allowlist is a policy bypass
  that is hard to audit and easy to widen.
- Treat an unanswered request as approval after a grace period. Rejected: silence must never
  authorise a side effect.

## Consequences

Any surface that can read the bus can answer, and a crashed surface denies rather than approves.
The request shape is part of the protocol, so a new tool with a side effect must add one. A
remote approval surface needs an authenticated transport before it can reuse this bus.

## Reopen if

Approvals must cross machines or accounts, which requires a signed transport and a different
trust boundary than the local filesystem.

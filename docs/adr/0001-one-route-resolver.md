# 0001: One route resolver for every surface

Status: accepted. Date: 2026-11-06.

## Context

The session composer, the CLI, the route inspector and the coordinator all need to answer the
same question: which provider and model will serve this turn? When each surface resolves a saved
pair or a bare model id itself, the picker can show one answer while the turn sends another.

## Decision

`rigel-core` owns one resolver. It reads the provider table, the saved route and the requested
model, repairs a bare id only when the repair is unambiguous, and labels that repair
`bare-id-repair`. The CLI's `route explain --json` serialises the resolver's four fields
(provider, model, source and reason), and the UI reads the same payload. A turn records the
resolved pair with the message so a later provider or model change cannot rewrite history.

## Alternatives considered

- Resolve in each surface and compare strings. Rejected: the picker and the send path can drift.
- Fall back silently to a default provider when the pair is wrong. Rejected: it hides a
  configuration error and can send data to a provider the user did not choose.
- Let the provider choose a model alias. Rejected: spend and transcript attribution become
  provider-dependent.

## Consequences

A new surface must call the resolver rather than reimplementing lookup. Adding a provider updates
one table and its shape checks. A wrong pair is a named error with a remedy, not a fallback.

## Reopen if

A provider requires per-request server-side routing that cannot be represented by a static
provider/model pair, or a surface needs to resolve a route without the core crate.

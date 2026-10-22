# Architecture

One desktop window, one local database per project, four harness protocols, and a core that owns
every fact the window shows. This document is the shape; [docs/handoff.md](docs/handoff.md) is the
reasoning and the checklist.

## The pieces

```
ui/ (React)  ──IPC──▶  crates/rigel-app (Tauri)  ──▶  crates/rigel-core
                                                          │
                    crates/rigel-adapters ◀───────────────┤  (Codex, Claude, ACP, native)
                    crates/rigel-mcp     ◀───────────────┤  (tools injected into an agent)
                    crates/rigel-cli     ◀───────────────┘  (every command, `--json`)
```

`rigel-core` has no Tauri dependency and no HTTP handler. The window and the CLI are two clients
of the same service layer, which is what makes the Phase 1 exit test meaningful: two processes,
one database, one session.

## Two databases

| File | Scope | Holds |
|---|---|---|
| `%APPDATA%\Rigel\index.db` (`~/.local/share/rigel` elsewhere) | every project on the machine | skills, sources, HTTP cache, blobs manifest, `tool_status`, `install_ledger`, update settings |
| `<project>/.rigel/rigel.db` | one project | sessions, agents, turns, items, claims, conflicts, patches, approvals, interventions, checkpoints, memory index |

Both are SQLite in WAL mode with foreign keys on. Migrations are numbered `.sql` files compiled
into the binary and applied one transaction at a time (`migrate.rs`); the runner refuses a database
newer than the build and refuses a renamed migration. `.rigel/.gitignore` is written on first use so
a session log cannot be committed by accident.

Skill bodies are **not** in the database: they are files at `<data dir>/blobs/<sha256>`, written
before their row so a crash leaves an orphan (collected by `rigel skills gc`) rather than a row
pointing at nothing.

## The write path

Every project write goes through `PatchPipeline` (`patches.rs`), in this order:

1. read the file, compare `base_hash` — compare-and-swap; a stale base writes nothing;
2. insert the `patches` row as `proposed`;
3. write `.rigel/journals/<id>.json` and fsync it;
4. write a temp file beside the target, fsync, rename;
5. mark the row `applied`, delete the journal.

`recover` reconciles the two crash windows: a journal with an untouched target is `Pending` and is
never applied automatically; a target already at `result_hash` is marked applied without a second
write. `StopAt` is the seam the crash tests use to stop at exactly those two points.

## A turn

`Coordinator::run_turn` reserves one cent from the session budget, builds the request (system
prompt, tool schemas, the session's pinned skills), streams the model's reply into `items`, runs
tool calls through `Tools`, and accounts the provider's usage as each step lands. The cap stops the
turn before its next step and interrupts every running agent in the session; the step that crossed
it is still billed, because that is what happened.

Tools are small and explicit: read, list, search, write, edit, run a command, fetch a URL, load a
skill, spawn read-only children. A write goes through the patch pipeline. A command is shown and
approved before it runs, and a destructive one is classified first, snapshotted second, argued over
by three advisors third, and still refused unless the user says yes.

## Several agents

Claims (`claims.rs`) are advisory and never block. A path with a live claim held by another agent
comes back as `held_by` plus a conflict row, and `release` is a tombstone. `attribute.rs` polls the
tree every 250 ms, hashes only files whose size or mtime moved, and names the writer — one agent, or
all of them with `ambiguous` set. A parse of declared tool use and a body scan (`toolchain.rs`)
say what a skill needs, with the line that said so.

Budgets are per session with per-agent attribution. Spend is micro-dollars; the cap is enforced
locally within one turn, and `rigel doctor` reconciles the ledger against the summed turns.

## Trust and installs

`toolchain.rs` keeps a curated registry (twelve tools), probes it per host (`windows`,
`wsl:<distro>`, `darwin`, `linux`), and caches the answer in `tool_status` with the binary's path
and SHA-256. A trust score is arithmetic over five weighted factors, printed in full by
`rigel trust explain`; it gates an install below 50 but never triggers one. An install prints its
exact command, runs only after approval, records decision, exit code and duration in
`install_ledger`, re-probes the host, and pins the installed binary's hash. Unattended mode queues;
it never runs a network install. Actions that need root are printed and refused, because Rigel never
runs an elevation tool.

## Updates

`update.rs` is the only update code. The switch defaults to off. A manifest is Ed25519-verified over
`version\nurl\nsha256` against a key in `update-pubkey.txt` or `RIGEL_UPDATE_PUBKEY`, and the
artifact's SHA-256 must match before it is offered. Nothing installs itself.

## Testing

- Unit tests next to the code for parsers, arithmetic and state machines.
- Integration tests in `crates/rigel-core/tests` over real temporary files, real SQLite and real
  git repositories.
- Adapter tests replay recorded fixture streams, so a protocol change is a failing test rather than
  a surprise.
- One exit script per phase in `scripts/`, each asserting the phase's promise end to end and
  printing every check.

The six non-negotiable guarantees in the handoff (§12) have named tests; a change that makes one of
them pass by weakening it is a regression in the product, not a fix.

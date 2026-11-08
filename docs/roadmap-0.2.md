# Roadmap 0.2 — what 0.2.0 closed

Round 9 accepted more work than a public release should carry. Under **Q330**, the substantial
items were moved here with their decisions intact. 0.2.0 shipped the release gates below; the
work that is not a gate is listed under [not 0.2.0 gates](#not-020-gates--moved-to-03) and does
not hold the version. Each closed line names the code and the test that proves it.

**0.2.0 shipped 6 Nov 2026.** `rigel --version` and the window report `0.2.0`.

## 0.2.0 gates — closed

- [x] **Provider routing: the pick travels with its door.** **Landed 6 Nov 2026:** the renderer
  sends the provider with the model on `turn_send` and `turn_estimate`, and one core resolver
  (`crates/rigel-core/src/agent/routing.rs`) decides the pair. A saved pair, a live-catalogue
  guard, an agent row, a preset, Free mode and swarm all pass through it; a model the named door
  cannot send is an error, never a quiet default. A model id can never *select* Vertex — its
  namespace `google/gemini…` is also OpenRouter's spelling — so a Vertex request happens only
  when Vertex is the selected provider. `guard_live_model` still refuses a saved id absent from
  the provider's live roster, now against the resolved door. Tests:
  `agent::routing` — including `a_broken_vertex_door_is_never_chosen_for_another_providers_model`
  (a credentialed, broken Vertex row present while every other pick resolves elsewhere),
  `a_picker_pair_beats_a_stale_saved_pair`,
  `a_model_the_named_door_cannot_send_is_an_error_not_a_default`, and
  `free_mode_and_swarm_pairs_survive_the_rule`.
- [x] **Vertex credential guard.** **Landed 6 Nov 2026:** `vertex::resolve_token` refuses an API
  key (`AIza…`), a service-account JSON (`{…`) and an ID token (`eyJ….….…`) before any request,
  naming the exact remedy (switch to `gemini` for an AI Studio key, or store
  `gcloud auth print-access-token`). Tests: `agent::vertex` — the API-key test and
  `an_id_token_or_service_account_file_is_also_refused_before_sending`.
- [x] **Version 0.2.0.** **Landed 6 Nov 2026:** `Cargo.toml` workspace version, `ui/package.json`
  and `crates/rigel-app/tauri.conf.json`; `rigel --version` prints `rigel 0.2.0` from
  `CARGO_PKG_VERSION`, so the CLI and the window cannot disagree.
- [x] **Destructive-command blast-radius preview (Q239).** **Landed 6 Nov 2026:** the deterministic classifier's findings are turned into `BlastRadius` before the question; every approval surface receives the exact command segments, resolved paths, file/byte counts where enumerable, and an explicit `cannot_determine` row for a remote, registry write or unresolvable target. The preview never executes anything and never guesses a count; the existing snapshot/restore path still runs after approval, so an approved deletion remains recoverable.
- [x] **Per-session network kill switch and project allowlist (Q240).** **Landed 6 Nov 2026:** `NetworkPolicy` is stored in the project's `.rigel/network.json` and checked before the per-origin prompt and before any socket. `enabled: false` refuses every request; a non-empty allowlist refuses every origin not on it; `disabled_sessions` refuses just the named session, while the per-origin once/session/always prompt remains the default gate. The window exposes `network_get`/`network_set`; `rigel agent run --no-network` and repeated `--allow-origin` use the same policy.
- [x] **Per-project MCP server approval with a tool list (Q242).** **Landed 6 Nov 2026:** `rigel_approve` recognises `mcp__server__tool` names and reads the project's `.mcp.json`. The first use of a declared server is refused until the card answers, and the card carries that server's **real** `tools/list`, read by launching only the declared stdio command through a bounded `initialize` → `tools/list` handshake that is killed when it ends. A server grant is not a tool grant: each tool asks on its own first use, with its description and input schema. Every decision is recorded — an `approval` row (`gate: "Q242"`) on the turn, or an `mcp_approval` intervention when a harness turn has no row. Tests: `crates/rigel-mcp/tests/project_mcp.rs` (refused, then approved and recorded) against the local stdio fixture in `tests/fixtures/fake_mcp_server.rs`.
- [x] **Skill-script approval and untrusted-output marking (Q243).** **Landed 6 Nov 2026:** `Tools::skill_load` records the script paths a loaded body names and `run_command` carries them as `skill_scripts`; the command path refuses a script that has not been approved even when the command itself was granted at project scope, and a command grant never counts as a script grant. The window card and the CLI question print each exact path with the skill that contributed it. Tests: `crates/rigel-core/tests/skill_scripts.rs` (blocked despite a pre-granted command, then permitted and run).
- [x] Provider-key validation on save (Q244). **Landed 5 Nov 2026:** a chat provider key is checked before the keychain write; `/models` with the candidate is the default, OpenRouter uses its authenticated `/key` record because its roster is public, and providers with no roster are reported unchecked rather than valid. A rejection leaves the previous key in place; `rigel keys set` uses the same core path.
- [x] **Per-subagent caps (Q245).** **Landed 6 Nov 2026:** each child coordinator is built with `SubagentPolicy::max_steps`, separate from the parent's bound and from the per-turn child count; `a_subagent_stops_at_its_own_step_cap` proves two children stop at `step_limit` while the parent completes.
- [x] **Approval before memory writes (Q251).** **Landed 6 Nov 2026:** `MemoryService::new` defaults to `DenyMemory`; a caller must install a `MemoryApprover` with `with_approval`. The window puts the exact path, mode and content on the approval card, the CLI prints them and reads `y/yes` (EOF denies), and the MCP write uses the same bus; validation still runs before the question. `memory_write_is_closed_without_an_approver` proves the default refusal.
- [x] **Pause and resume at a step boundary (Q254).** **Landed 6 Nov 2026:** `Cancels::pause_turn`/`resume_turn` set a per-turn pause checked only at the coordinator's step boundary; the same turn, message list and remaining budget continue, and `paused`/`resumed` rows are written to the transcript. The window exposes `turn_pause`/`turn_resume` and Pause/Resume controls; the CLI has no long-lived turn process to signal, so its pause surface remains the window.
- [x] **Free mode and launch freshness.** **Landed 6 Nov 2026:** opening the window forces every credentialed roster and re-syncs skill sources; the picker cache is 60 s and `models_refresh` re-reads on demand. Free mode ranks tool-capable free chat rows by a checked-in benchmark snapshot, probes up to six with a real 8-token message, and saves the best-ranked provider/model through the same settings path a manual pick uses; the native fan-out policy and the Chat room's managed `@rigel-free` agent run on that same pair. Tests: `agent::freemode`, `agent::agentchat`.

## Not 0.2.0 gates — moved to 0.3

These were accepted in round 9 and deferred here by Q330. None of them is a 0.2.0 gate, and none
is partly implemented behind a stub. The reason is per group, because the groups fail for
different reasons.

### Agent trust

- Context compaction at 80% of the window, with a visible compaction record (Q237).
- Provider retry plus an explicit fallback offer (Q238).
- Secret redaction across output, logs, and diagnostics (Q241).
- Approval countdown and Retry (Q246).
- 200-second command timeout (Q247).
- Incomplete-turn billing marker (Q248).
- Continue-with-a-fresh-step-budget (Q249).
- Per-step cost and timing details (Q250).
- On-demand file and symbol map (Q252).
- First-class local-model support (Q253).
- One writer per file; no side-by-side agent race (Q255).

**Why not 0.2.0:** each changes what a running turn does — its budget, its retries, its
transcript rows or its approval surface — and needs its own coordinator test. The routing repair
had to land first because every one of them starts by sending a turn through the door that is
now resolved correctly.

### Sessions and the workspace

- Fork from a turn with lineage (Q256, Q261).
- Per-project model, effort, and step defaults (Q257).
- Full Claude Code and Codex history import and resume (Q258).
- Redacted Markdown/JSON session export (Q260).
- Cheap-model session titles (Q262).
- Crash recovery offer (Q263).
- Per-agent model picker (Q265).
- Handoff prompt to another agent (Q266).
- Claim counts in the roster (Q267).
- Run-tests action (Q268).
- `AGENTS.md` precedence over `CLAUDE.md` (Q269).
- Skill pins and template inclusion (Q270).
- Uncapped inline vision images, after a memory/abuse review (Q271).
- Worktree auto-prune (Q272).
- Rolling database backups (Q273).
- Permanent session deletion (Q274).
- Taskbar running/attention badge (Q275).

**Why not 0.2.0:** this group needs schema migrations plus a window pass; a migration that lands
without its restore path is worse than no migration, and Q270's pin behaviour and Q265's picker
need UI review that this environment cannot do.

### The window

- First-run setup (Q276).
- Follow-system theme (Q277).
- Rebindable shortcuts (Q278).
- Transcript filter (Q279).
- Markdown and syntax highlighting (Q280).
- Steering a running turn (Q281).
- Full slash-command language (Q282).
- Notification routing and quiet hours (Q283).
- Tray control surface (Q284).
- Crash reporter (Q285).
- Contextual empty states (Q286).
- Model favourites and cost hints (Q287).
- Richer inspector (Q288).
- Focus mode (Q289).
- Explicit high-contrast toggle (Q290).
- Full internationalization (Q291).
- Drag-and-drop in-place attachments (Q292).
- Word-level diffs (Q293).
- Settings search deep links (Q294).
- About dialog (Q295).

**Why not 0.2.0:** visible work that must be checked in a running Tauri window, including the
keyboard-only and contrast passes; compiling it would not verify it, and 0.2.0 does not claim it.

### Cloud, data, and integrations

- Hosted Cloud default and its privacy review (Q296).
- Account export/delete (Q297).
- Versioned sync conflicts (Q298).
- Opt-in tool output and diffs in published sessions (Q299).
- Mandatory App Check (Q300).
- Sponsored cloud runs (Q301).
- Remote phone prompts and approvals (Q302).
- Full GitHub PR/check sync (Q303).
- Generic forge interface (Q304).
- Linear connector (Q306).
- SQLCipher database encryption (Q308).
- System proxy support (Q311).
- Google and GitHub OAuth (Q313).
- Full Terms of Service and privacy policy with legal review (Q315).

**Why not 0.2.0:** these are separate product and legal decisions, as the original order of work
said. They need accounts, credentials, provider projects and a privacy/legal review this
repository does not own.

### Test and release infrastructure

- Native Tauri-window driver (Q317).
- Fuzz/property tests for parsers, paths, and migrations (Q319).
- Database backup and migration rehearsal as a mandatory gate (Q320).
- Crash-log upload with explicit consent and a backend (Q322).

**Why not 0.2.0:** they need CI hosts, long-running fuzz budgets and a crash-log backend. They
are the first work of 0.3, because the next session and window batch is what they protect.

## Order of work after 0.2.0

1. Native-driver smoke, migration rehearsal, fuzz targets, crash-log consent design.
2. The cheap agent-trust items: 200-second timeout, approval countdown, secret redaction, step
   metrics.
3. Session lineage and project defaults, because the rest of the session work builds on them.
4. The window pass in one batch: theme, shortcuts, filter, Markdown, focus, About.
5. Hosted Cloud, OAuth, SQLCipher, generic forge, and remote approvals as separate decisions.

Back to [round 9](grilling-round-9.md) · [handoff](handoff.md) · **Next:** [roadmap-0.3.md](roadmap-0.3.md)

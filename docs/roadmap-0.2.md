# Roadmap 0.2 — the deferred half of round 9

Round 9 accepted more work than a public 0.1.7 should carry. Under **Q330**, the substantial items
were moved here with their decisions intact. This is not a wish list: each line points at the
answer that accepted it in [grilling-round-9.md](grilling-round-9.md).

## 1. Agent trust and recovery

- Context compaction at 80% of the window, with a visible compaction record (Q237).
- Provider retry plus an explicit fallback offer (Q238).
- Destructive-command blast-radius preview (Q239).
- Per-session network kill switch and project allowlist (Q240).
- Secret redaction across output, logs, and diagnostics (Q241).
- Per-project MCP server approval with a tool list (Q242).
- Skill-script approval and untrusted-output marking (Q243).
- Provider-key validation on save (Q244).
- Per-subagent caps (Q245).
- Approval countdown and Retry (Q246).
- 200-second command timeout (Q247).
- Incomplete-turn billing marker (Q248).
- Continue-with-a-fresh-step-budget (Q249).
- Per-step cost and timing details (Q250).
- Approval before memory writes (Q251).
- On-demand file and symbol map (Q252).
- Pause and resume at a step boundary (Q254).

## 2. Sessions and the workspace

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

## 3. The window

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

## 4. Cloud, data, and integrations

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

## 5. Test and release infrastructure

- Native Tauri-window driver (Q317).
- Fuzz/property tests for parsers, paths, and migrations (Q319).
- Database backup and migration rehearsal as a mandatory gate (Q320).
- Crash-log upload with explicit consent and a backend (Q322).

## Order of work

1. Finish the release gate: native-driver smoke, migration rehearsal, fuzz targets, crash-log
   consent design.
2. Land the agent trust items that are cheap and independent: 200-second timeout, approval
   countdown, secret redaction, step metrics.
3. Land session lineage and project defaults, because the rest of the session work builds on them.
4. Take the UI pass in one batch: theme, shortcuts, filter, Markdown, focus, About.
5. Treat hosted Cloud, OAuth, SQLCipher, generic forge, and remote approvals as separate product
   decisions, not a single 0.2 checkbox.

Back to [round 9](grilling-round-9.md) · [handoff](handoff.md).

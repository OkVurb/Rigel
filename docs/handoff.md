# Rigel — implementation handoff

**Audience:** a coding agent implementing this, with a human reviewing. Every claim about an external
CLI carries a source URL; every decision carries an ID that later sections cite. Where this document
and an older doc disagree, this document wins — see [§3.3 Superseded](#33-superseded).

**Last reviewed:** 20 Oct 2026.

---

## How to use this file

This is a **living checklist**, not a spec you read once.

- Every deliverable and every exit test has a stable ID (`P1.3`, `P4.2`) and a checkbox.
- **Update rule:** the commit that finishes an item ticks its box **and** appends the short commit
  hash after the line, e.g. `- [x] **P1.1** Cargo workspace… — \`a1b2c3d\``. Do not tick an item in a
  different commit from the work.
- `node scripts/handoff-status.mjs` prints completion per phase, so nobody has to count boxes.
- An exit test is only ticked when it has been run **on a clean checkout**, not when it "should" pass.
- If an item is dropped, mark it `- [ ] ~~P4.7 …~~ **dropped:** reason` rather than deleting it.
  The history of what was cut is part of the handoff.

**Progress:** run the script. Phase 0 is complete; Phases 1–7 are unstarted as of this review.

---

## 0. Non-negotiables

These are product constraints, not preferences. A change that violates one is a bug even if tests pass.

| # | Rule | Why |
|---|---|---|
| N1 | The UI never says a file is **locked**. It says *claimed by X, unverified*. | Claims are advisory (D5). Pretending otherwise is the one lie this product cannot afford. |
| N2 | Every approval shows the **exact command** and the **scope** (once / session / project / always). | Inherited from the design brief; the difference between an instrument and a slot machine. |
| N3 | Cost and token count are always visible, in tabular figures, in the status bar. | The teardown found cost opacity to be the most emotional complaint about every competitor. |
| N4 | Never hide duplicates, never silently collapse variants. | The index's original promise. |
| N5 | No telemetry, no analytics, no phone-home. Updates are a signed manifest fetch you can disable. | Open source means the binary is inspectable; behaviour must match. |
| N6 | No install runs without a shown command. No unattended install of anything with network access. | D12, D13. |
| N7 | Nothing machine-specific enters a committed file. Memory is the only committed artifact. | D10. |
| N8 | The design brief holds: 13px body, 28px rows, amber `#E9A23B` as the only accent, no gradients, no glow, no emoji icons, nothing below 11px. | The product's visual identity is "instrument". |

---

## 1. What Rigel is

One desktop window where several coding agents — Codex, Claude Code, Cursor, Cline, and Rigel's own
coordinator — work on the same project. It keeps the session, the memory, the diff and the bill in one
place, and it is honest about what it can and cannot enforce.

The product's centre of gravity, stated plainly, because the earlier docs say something different:

> Rigel is not an index of skills with an agent bolted on. It is an agent workspace whose capability
> shelf happens to be excellent.

Skills remain the differentiator: they are how a workspace's capability is made explicit, versioned and
auditable. But the session — agents, claims, diffs, budget, memory — is the product.

**What it is not:** a hosted service, a marketplace with accounts, an IDE, a replacement for the
harness CLIs, or a sandbox vendor. It wraps agent processes; it does not reimplement them.

---

## 2. Current state (verified 20 Oct 2026)

### 2.1 Repository

```
<repo>/
  README.md                    product + status
  docs/
    design-prompt.md           visual brief (still authoritative, see §3.3)
    grilling-50.md             66 pre-pivot questions
    interview-answers.md       60 answered questions + seven collisions (authoritative for intent)
    phase-0-teardown.md        competitor analysis
    phase-1-plan.md            the OLD plan — superseded by §11
    skill-intelligence.md      discovery, fingerprinting, safety, ranking
    handoff.md                 this document
  prototype/                   the working UI prototype (React 19 + TS + Vite)
    index.html                 title "Rigel", favicon /rigel.svg
    public/rigel.svg           the mark: star chain, amber on #0b0c0e
    src/App.tsx                3 views: packages / discover / targets
    src/components/            PackageList, PackageDetail, DiscoverView, DepsPanel,
                               TargetsView, CommandPalette, StatusBar, ImportDialog, ui.tsx
    src/db.ts                  localStorage stand-in for the future index
    src/discover.ts            live GitHub / SkillsMP / SkillHub fetching
    src/data.ts                mock catalogue (337 lines)
    src/deps.ts                20 real tools with winget/brew commands
    src/targets.ts             measured per-harness state on this machine
    src/styles.css             677 lines, implements the design brief
    vite.config.ts             dev-only proxy `/__hub` → skillsmp.com, skills.browseract.com
  scripts/                     export-installed-skills.mjs, verify-ecc.mjs, fix-cursor-hooks.mjs
  screenshots/                 captured states
```

### 2.2 What actually works

| Surface | State | Evidence |
|---|---|---|
| Package list / detail / variants / evidence table | mock data, real interactions | `src/data.ts`, preview capture |
| Discover → GitHub | live: repo tree, frontmatter parse, static scan, download | `src/discover.ts` |
| Discover → SkillsMP | live via dev proxy, 50 anonymous requests/day | same |
| Discover → SkillHub | live via proxy, HTML parsed | same |
| Discover → All | merges sources, marks on-disk duplicates | 446 rows, 293 not on disk |
| Dependencies | 20 tools, real commands, generated install script | `src/deps.ts` |
| Targets | measured harness state | `src/targets.ts`, `scripts/verify-ecc.mjs` |
| Local index | `localStorage` metadata only, survives reload | `src/db.ts` |
| Agent surface | `⤓ .md` + **Copy brief** per package | `src/components/ui.tsx` |

### 2.3 What is fake, and must not be mistaken for real

- Install buttons in the prototype: `setTimeout(900)`, then a set insert. No filesystem write.
- Safety grades: static scan of one file, mock for non-GitHub rows.
- `src/data.ts` catalogue: hand-written, with realistic numbers but no provenance.
- The prototype has **no agent**, no terminal, no session, no database beyond localStorage.

### 2.4 Build and run

```bash
cd prototype
npm install
npm run dev        # http://localhost:5173
npm run build      # tsc -b && vite build  — currently exits 0
```

`Ctrl+1` packages, `Ctrl+2` discover, `Ctrl+3` targets, `Ctrl+K` palette, `/` focus search.

There is **no test suite** in the repository. `npm run build` and browser checks are the entire
verification surface today. Phase 1 changes that (§12).

### 2.5 Measured environment

- Windows, PowerShell 7, Git Bash, WSL 2 installed.
- 2,737 skill directories across harnesses; 446 unique catalogue rows; 293 absent from disk.
- Codex: 5/5 modules installed, hooks resolve. Cursor: hook paths project-relative, degraded.
- Present: node, npm, python, git, rg, pwsh, wsl. Absent: docker, `GROQ_API_KEY`, `crawl4ai`.
- GitHub unauthenticated quota: 60 requests/hour.Do not commit absolute user paths (`C:\Users\<name>\…`) into docs or fixtures. Use `%APPDATA%`,
`~`, and project-relative paths.

### 2.6 Implemented since §2.1–2.5 (Phases 1–2)

The prototype still exists as the visual reference; the app is now the Rust workspace, and these
replace the localStorage notes above.

| Surface | Where | Evidence |
|---|---|---|
| Session log: sessions, agents, turns, items, checkpoints | `crates/rigel-core/src/session.rs` | `tests/sessions.rs` (5), restart persistence in `docs/qa` for P1.10 |
| Desktop shell over IPC | `crates/rigel-app/src/lib.rs` | `project.open`, `session.*`, `settings.*`, `skills.*` |
| Renderer | `ui/src/App.tsx` | shelf wired to the index; browser preview uses sample rows |
| Global index | `crates/rigel-core/src/skills/index.rs`, `<data dir>/index.db` | FTS5, clusters, evidence; 12 integration tests in `tests/skills.rs` |
| Skill bodies | `<data dir>/blobs/<sha256>` | never in SQLite (P2.3); `rigel skills gc` reconciles |
| Sources | `skills/github.rs`, `skills/hub.rs` | GitHub (tree + raw bodies + ETag), SkillsMP, SkillHub |
| CLI | `crates/rigel-cli/src/main.rs` | `skills search\|get\|index\|status\|sync\|stats\|import\|gc`, all `--json` |

### 2.7 Implemented in Phase 4 so far

| Item | Where | Evidence |
|---|---|---|
| **P4.5** Codex adapter | `crates/rigel-adapters/src/codex.rs` | app-server primary, `exec --json` fallback that never re-runs a turn which already produced tokens, per-turn usage from `thread/tokenUsage/updated`; `tests/codex.rs` (4) against `tests/fixtures/fake_codex.rs`; real turn 21 Oct — 155,894 in / 5 out — `7b3c2fb` |
| `rigel codex` CLI | `crates/rigel-cli/src/harness.rs` | `probe --json` reports 0.162.1 and an absolute path; `run --json` returns text, transport, fallback flag and usage |

---

## 3. Decisions of record

### 3.1 Product

| ID | Decision | Source |
|---|---|---|
| D1 | The unit is a **project-level session**; the UI shows windows onto a log. **Amended by D35:** a session idle for 24 h auto-ends, and resuming starts a new session. | Q17, Q83 |
| D2 | Memory is **per agent and portable**; it lives in the repo as text. | Q18, Q47 |
| D3 | The index is **local-only** and machine-specific. | Q56 |
| D4 | Uploaded files are **context only**, never project content. | Q19 |
| D5 | File claims are **advisory**: an MCP tool agents may call, plus violation detection. No locking. | Q14→Q31→Q51 |
| D6 | Only agents Rigel **spawns** are forced to register. Third-party subagents are observed best-effort. | Q53 + research |
| D7 | Spawned agents run in **isolated worktrees**; the main session runs in the live tree. | Q52 |
| D8 | Writes Rigel performs are **atomic and journalled**; direct harness writes are not. | Q33 |
| D9 | Skills are invoked as **tools**, and skill versions are **pinned per session**. | Q38, Q39 |
| D10 | Budget caps are mandatory and stop agents automatically. | Q25 |
| D11 | Sessions must **resume** after sleep or network loss, and be replayable from messages, tool calls and diffs. | Q45, Q46 |
| D12 | Installs auto-proceed only above tunable **trust thresholds**, and only while the user is present. | Q41, Q54 |
| D13 | Unattended mode bans **network installs and schema migrations**. | Q55 |
| D14 | Credentials live in the **OS keychain**, per agent and per provider. | Q48 |
| D15 | Skills are untrusted; **capabilities are gated** at the agent's tool layer. | Q49 |
| D16 | One merged diff with **agent attribution**, not side-by-side panes. | Q24 |
| D17 | Success metric is **interventions per completed task**, trending down. | Q50 |
| D18 | Rigel calls model APIs **directly** for its native agent; the local OmniRoute router first, OpenRouter as the hosted alternative. | user, 20 Oct |
| D19 | ~~The native agent **coordinates and patches**; it has no general bash loop of its own.~~ **Superseded by D30** — the native agent gets a shell, one approval per command. | user, 20 Oct |
| D20 | Windows first; macOS and Linux adapters later. | user, 20 Oct |
| D21 | **Every harness authenticates with its own API key.** No subscription, SSO or browser login is ever reused, and a harness that is only subscription-authenticated refuses to spawn. | user, 20 Oct |
| D22 | Launch **resumes the newest open session** for the last project; Shelf is one click away. | Q67 |
| D23 | Diff is a **read-only unified diff per patch** with revert, opened from a patch or a session. Not an editor. | Q68 |
| D24 | **One window**, with an in-app project switcher that restores the last project. No tabs, no per-project windows. | Q69 |
| D25 | A **command palette (Ctrl/Cmd+K)** covers sessions, projects, providers, settings, and stop/start. | Q70 |
| D26 | **Providers are plural and concurrent.** Each agent is bound to a provider + model when its session starts, so several run side by side on different upstreams. OmniRoute is the front door because it already multiplexes seven upstream connections behind one OpenAI-compatible surface; **per-agent OmniRoute API keys** give isolation, policy, budget and `cost --group-by api-key` attribution. OpenRouter stays as a direct fallback. | Q71 |
| D27 | The **router starts with the app**, in the background. A turn that finds it down starts it, retries once, then fails with a fix-it panel rather than silently switching provider. | Q72, Q75 |
| D28 | **Model picker with a saved default**, plus a **separate reasoning-effort setting** sent per request — effort is not an id suffix. | Q73, Q74 |
| D29 | **Cost honesty.** The local ledger stays the hard cap; router/provider usage is reconciled and displayed, never the enforcement source (remote usage lags). Turns the router prices are labelled `estimated`, always with token counts, and prices sync from router metadata. | Q76, Q81 |
| D30 | The native agent gets a **real shell tool**; every command is approved with its exact text and scope (N2). Supersedes D19. | Q77 |
| D31 | Writes stay **auto-applied inside the project**, journaled and revertible per patch. No per-write prompt. | Q78 |
| D32 | **Destructive operations need three things**: a recorded deliberation (agents argue pro/con), a **snapshot of uncommitted work** (D38), and an explicit approval with the exact command. Nothing destructive runs on a single model's say-so. | Q79 |
| D33 | **Network fetch with per-origin approval**; search and fetch never run unapproved, and fetched text is labelled untrusted. | Q80 |
| D34 | Subagents **pool under the session cap**; there is no per-child ceiling. | Q82 |
| D35 | Sessions **auto-end after 24 h idle**; resuming creates a new session. Amends D1. | Q83 |
| D36 | **Codex is the first external harness** — `codex exec --json` primary, app-server behind the trait — with `rigel-mcp` wired in from day one. Claude Code is deferred: no subscription on this machine, and the router's own Claude connection is marked *banned*. | Q84, Q85 |
| D37 | **1.0 is a signed MSI** with opt-in signed auto-update, **MIT OR Apache-2.0**, and zero telemetry. | Q86 |
| D38 | A pre-destructive snapshot is a **git object plus a file copy, not a restore point**: `git stash create` captures tracked changes without touching the index or the worktree, untracked non-ignored files are copied, and `rigel snapshot restore <id>` rebuilds the tree. A Windows restore point is rejected — it rolls back system files and registry and explicitly leaves personal files alone, so it would protect everything except the work at risk. | §15.5 |
| D39 | Codex's upstream is the **router**, confirmed by probe on 21 Oct (both `exec --json` and app-server passed with `wire_api="responses"`). Rigel writes the `-c` provider overrides itself — never `omniroute run`/`launch-codex`, which is broken here — and `codex.upstream` (`router` or `openai`) is a per-agent setting with **no silent fallback**: a down upstream refuses with a fix-it message (D27). | §15.5 |
| D40 | Agent visibility is a **roster plus one split pane**: the roster lists every agent with spend and claims, and at most two transcripts sit side by side. No grids, no tabs. | §15.5 |
| D41 | **SQLite locking is explicit.** WAL is set **once per file** when the store opens, never as a per-connection pragma: sqlx re-applies requested pragmas on every connection, and the journal-mode pragma takes an exclusive lock that `busy_timeout` cannot wait for — so a second opener fails at connect, and the attempt can invalidate a read snapshot mid-transaction. Any transaction that **reads before it writes** opens with `BEGIN IMMEDIATE`; a write-first transaction may stay deferred. | 21 Oct, storage audit |

### 3.2 Stack

| ID | Decision | Rationale |
|---|---|---|
| S1 | **Tauri 2 shell + Rust core + React 19 UI.** Rust is a required part of the build, not an optional extra. | User requirement; smallest binary; the existing UI already fits. |
| S2 | **The Rust core owns all state.** No application state lives in JavaScript beyond the renderer's cache. | One owner for sessions, claims, patches, budgets; the UI can crash without losing anything. |
| S3 | **Adapters are Rust by default.** ACP, Codex app-server and Claude's NDJSON are all stdio protocols a Rust client can speak. | No second runtime for the happy path; one process tree; one logger. |
| S4 | **A bundled Node sidecar is permitted where a vendor SDK is the only complete surface** — primarily Cline's `requestToolApproval`, and as a fallback if Codex app-server drifts. | Piggybacking on vendor SDKs beats reimplementing their edge cases; it stays optional and off the critical path. |
| S5 | **ACP-first** for Cursor and Cline; Codex via app-server with `codex exec --json` fallback; Claude via `stream-json` + an MCP permission tool. | Richest observable surface per harness, with a documented degraded path. |
| S6 | SQLite via `sqlx` (async, compile-time-checked queries), WAL, numbered SQL migrations, no ORM. | Inspectable with `sqlite3`, which is the debugging story that matters. |
| S7 | Rust MCP server using the `rmcp` crate; agents connect to Rigel, never the reverse. | Codex removed its MCP-server mode; Rigel must be the server. |
| S8 | Frontend state: `zustand` + a single event router; TanStack Query for Tauri reads; no Redux. | Small surface, React 19. |

**Why the sidecar exists at all.** The four vendor SDKs are Node packages, but they wrap the same stdio
protocols. Rust covers Codex, Claude, Cursor and Cline's ACP paths without it. The sidecar earns its
~50 MB when a capability exists *only* in an SDK — today that is Cline's approval callback and parts of
Cline's event taxonomy. It is started lazily, one process per SDK-backed agent, and it streams the same
normalised events. If it crashes, the Rust core restarts that adapter; the session log is unaffected
because Rust owns it. The sidecar never touches SQLite, claims, patches or budgets.

### 3.3 Superseded

| Old statement | Where | Now |
|---|---|---|
| "This indexes and explains; it does not own installation." | `phase-1-plan.md` risk 4 | False. D12/D13 make Rigel own install execution with a trust ledger and consent. |
| "Any always-on background service" is out of Phase 1 | `phase-1-plan.md` | Still true — nothing runs when the window is closed. |
| Three screens: browser, session, palette | `design-prompt.md` | Four: shelf, session, diff, settings. |
| `~/.local/share/skill-index/index.db` | `phase-1-plan.md` | Windows-first: `%APPDATA%\Rigel\index.db`. |
| "Skills plus one agent chat pane" as the week-one slice | Q26 | The slice now includes budget caps and the claim ledger's read side; a chat without a bill is not shippable. |
| Storage keys `skilldb:` | `README.md` | Migrated by `rigel skills import` (`docs/migrating-prototype-index.md`); nothing in `crates/` or `ui/` reads or writes them. |
| Rust-only adapters | this doc, first draft | S3/S4: Rust by default, sidecar where an SDK is the only complete surface. |
| "The native agent has no bash loop" (D19) | §3.1, §7.1, P3.3 | D30: a shell exists, gated per command. P3.3's registry is extended, not rewritten. |
| "A session never cleanly ends" (D1) | §3.1, §5 | D35: 24 h idle auto-end; resume starts fresh. |
| Claude Code as the first harness | §15 draft recommendation | D36: Codex first; Claude deferred. |

---

## 4. Architecture

### 4.1 Process model

```
┌──────────────────────────────────────────────────────────────────────┐
│ Tauri window (WebView2)                                              │
│   React 19 + TS  ──invoke/emit──┐                                    │
└─────────────────────────────────┼────────────────────────────────────┘
                                  │ Tauri IPC (commands + event channels)
┌─────────────────────────────────▼────────────────────────────────────┐
│ rigel-core (Rust, tokio)                                             │
│  ├ SessionService   turns, items, checkpoints, resume                │
│  ├ ClaimLedger      claims, heartbeats, conflict detection           │
│  ├ PatchPipeline    atomic apply, journal, attribution               │
│  ├ BudgetService    per-agent/turn accounting, hard stop             │
│  ├ SkillService     index queries, version pinning, tool wrapping    │
│  ├ MemoryService    committed memory files, schema + redaction       │
│  ├ TrustService     install policy, ledger                           │
│  └ EventBus         normalized AgentEvent fan-out                    │
└───────┬───────────────────┬────────────────────┬─────────────────────┘
        │                   │                    │
   ┌────▼─────┐      ┌──────▼──────┐      ┌──────▼────────┐
   │ Adapters │      │ MCP server  │      │ Model client  │
   │ ACP      │      │ (stdio,     │      │ OmniRoute /   │
   │ Codex    │      │  rigel_*)   │      │ (native agent)│
   │ Claude   │      └─────────────┘      └───────────────┘
   │ Native   │
   └────┬─────┘
        │ spawn + stdio JSON-RPC / NDJSON
   ┌────▼────────────────────────────────────────────────┐
   │ codex app-server │ claude -p │ agent acp │ cline     │
   └─────────────────────────────────────────────────────┘
        │ (only when an SDK-only capability is needed)
   ┌────▼─────────────────────┐
   │ node sidecar/index.mjs   │  ← bundled Node 22, lazily started
   └──────────────────────────┘
```

### 4.2 Harness capability matrix

Facts from vendor docs fetched 20 Oct 2026. **Verify against the installed binary before coding each
adapter** — all four auto-update, and Codex's protocol is explicitly version-dependent.

| | Codex | Claude Code | Cursor | Cline |
|---|---|---|---|---|
| Stable headless | `codex exec --json` (JSONL) | `claude -p --output-format stream-json --verbose` | `agent -p --output-format stream-json` | `cline --json` |
| Embedding protocol | `codex app-server` (JSON-RPC, **experimental, not supported for production**) | MCP permission tool + NDJSON | `agent acp` (JSON-RPC) | `cline --acp` (JSON-RPC) |
| Programmatic approval | app-server `…/requestApproval` → `accept`/`acceptForSession`/`decline`/`cancel` | `--permission-prompt-tool mcp__rigel__approve`, or `PermissionRequest` hook | ACP `session/request_permission` | SDK `requestToolApproval`; ACP permissions to client |
| Nested agents visible | item stream, not guaranteed for nested | yes: `parent_tool_use_id`, `--forward-subagent-text` (v2.1.219+) | ACP child sessions if the client advertises the capability | `team_event` exists; nested schema undocumented |
| Worktree | `--worktree` (exec only; not with resume) | host-managed | `--worktree [name]` under `~/.cursor/worktrees` | `--worktree` under `~/.cline/worktrees` |
| Working dir | `--cd` / `-C`, app-server `cwd` | `--add-dir`, `cwd` in settings | `--workspace` | `--cwd` |
| Skills dir | `.agents/skills`, `$HOME/.agents/skills`, `/etc/codex/skills`; app-server `skills/extraRoots/set` | `~/.claude/skills`, `.claude/skills`, `--add-dir` | `.cursor/skills`, `.agents/skills`, `~/.cursor/skills` | `.cline/skills`, `.agents/skills`-compatible, `~/.cline/skills` |
| MCP config | `~/.codex/config.toml` `[mcp_servers.*]` | `claude mcp add`, `~/.claude.json`, `.mcp.json` | `.cursor/mcp.json`, `~/.cursor/mcp.json` | `~/.cline/data/settings/cline_mcp_settings.json` |
| Sandbox / perms | `--sandbox read-only\|workspace-write\|danger-full-access`, `--ask-for-approval on-request\|never` | `--permission-mode default\|acceptEdits\|plan\|auto\|dontAsk\|bypassPermissions` | `--force` (applies changes without confirmation) | auto-approval defaults true in CLI, false in ACP |

Sources: [Codex non-interactive](https://developers.openai.com/codex/non-interactive-mode) ·
[Codex app-server](https://developers.openai.com/codex/app-server) · [Codex MCP](https://developers.openai.com/codex/mcp) ·
[Codex approvals](https://developers.openai.com/codex/agent-approvals-security) ·
[Claude headless](https://code.claude.com/docs/en/headless) · [Claude hooks](https://code.claude.com/docs/en/hooks-guide) ·
[Claude SDK](https://code.claude.com/docs/en/agent-sdk/overview) · [Claude skills](https://code.claude.com/docs/en/skills) ·
[Cursor ACP](https://cursor.com/docs/cli/acp) · [Cursor output format](https://cursor.com/docs/cli/reference/output-format) ·
[Cursor skills](https://cursor.com/docs/skills) · [Cline CLI](https://docs.cline.bot/cli/cli-reference) ·
[Cline SDK](https://docs.cline.bot/sdk/clinecore) · [Cline ACP](https://docs.cline.bot/usage/acp)

Versions observed: Codex CLI **0.162.1** (21 Oct probe; 0.163.0 was recorded on 15 Oct, so it moves);
Claude Code **2.1.251** (21 Oct `omniroute status`); Cursor **3.24.12**; Cline CLI **3.0.73**
(15 Oct, unverified since). Record `--version` output in `adapters.json` at first run and pin fixtures
to it.

**Probed on this machine, 21 Oct 2026 (Codex CLI 0.162.1).** Both Codex transports survive the router
with `wire_api = "responses"` and **no API key**:

```bash
codex exec --json -c 'model_provider="omniroute"' \
  -c 'model_providers.omniroute.base_url="http://127.0.0.1:20128/v1"' \
  -c 'model_providers.omniroute.wire_api="responses"' \
  -c 'model_providers.omniroute.requires_openai_auth=false' \
  -m gpt-5.6-sol-medium "reply with exactly: pong"
```

- `exec --json`: `thread.started` → `agent_message` `pong` → `turn.completed`, exit 0.
- `app-server`: `initialize` → `thread/start` → `turn/start` → `item/agentMessage/delta` `pong` →
  `turn/completed` with status `completed`, 7.3 s end to end.
- **Cost warning:** the exec turn reported **155,895 input tokens** for a one-word answer, because Codex
  injects this machine's 2,378 skills into every request. If input is priced at $3/M that is ~$0.47 per
  turn — the number that matters for D29 is input, not output.
- **The OmniRoute launchers are broken here.** `omniroute run codex` and `omniroute launch-codex` both
  pass `^^^"model_provider=\^^^"omniroute…` as argv, which Codex rejects, and the CLI's default context
  is `http://localhost:0`. Rigel must write the `-c` overrides itself.
- **Auth gap:** `/v1/chat/completions` and `/v1/responses` answer without a key from localhost (only
  `/v1/models` returns 401). Rigel still sends its key, but the router is not an auth boundary —
  anything local can spend (§10).
- **Reproduce it:** `CODEX_EXE=… node scripts/codex-router-probe.mjs` runs both transports and prints
  `exec --json PASS` / `app-server PASS` plus the token counts. It is the first thing P4.5 should run.
- **Implemented:** `crates/rigel-adapters/src/codex.rs` writes exactly these overrides, streams
  `item/agentMessage/delta`, reads per-turn usage from `thread/tokenUsage/updated`, and falls back to
  `exec` only when app-server produced no tokens. `rigel codex probe` and `rigel codex run` are the CLI
  surface; the adapter tests use `tests/fixtures/fake_codex.rs`, so they pass without Codex installed.

**Order (D36).** Codex first, because the probe above works: its router connection is `active`, while
Claude's is marked `banned` and there is no subscription to fall back on (D21). `omniroute setup-codex` / `launch-codex` already generate Codex profiles
pointed at the router, so a harness can share the native agent's bill instead of opening a second one.
Claude Code is deferred, not dropped — the adapter trait and fixtures are written so it slots in when an
`ANTHROPIC_API_KEY` exists.

### 4.3 Constraints the research imposes

1. **Codex app-server is experimental.** Primary path, but `codex exec --json` stays a tested fallback
   behind the same trait. Do not build a feature only app-server can support until the fallback has
   parity, or mark it Codex-only in the UI.
2. **Codex is not an MCP server.** Rigel hosts; agents connect. Anything Rigel needs from an agent goes
   through the adapter's protocol, not MCP.
3. **Every harness runs on its own API key** (D21). This is both a product rule and a legal one:
   Anthropic's terms forbid offering Claude.ai login or subscription rate limits in third-party
   products without approval, so Claude Code runs with `ANTHROPIC_API_KEY` and its own config. The same
   rule applies to Codex (`OPENAI_API_KEY`, not a ChatGPT login), Cursor (`CURSOR_API_KEY`) and Cline
   (provider keys in a per-agent config). See §4.4.
4. **Cursor's binary is `agent`**, not `cursor-agent`. Probe both, prefer the documented name.
5. **Claude's subagent visibility needs v2.1.219+** and a flag. Probe; if lower, mark subagents
   "not observable" rather than showing an empty list.
6. **The sidecar is a leaf.** It may not open the database, hold claims, or decide approvals. If a
   capability lands there, its events must still be normalised by the Rust adapter that owns the agent.

### 4.4 Authentication: API keys only (D21)

| Harness | Mechanism | Keychain entry | Probe must reject |
|---|---|---|---|
| Codex | `OPENAI_API_KEY` in the agent's env | `rigel/openai` | ChatGPT/SSO login |
| Claude Code | `ANTHROPIC_API_KEY` in the agent's env | `rigel/anthropic` | subscription login |
| Cursor | `CURSOR_API_KEY` | `rigel/cursor` | browser login |
| Cline | provider keys written to a **per-agent** config, never the global one | `rigel/cline.<provider>` | unauthenticated / credit-only mode |
| Native agent | OmniRoute key (local router) or OpenRouter key | `rigel/omniroute` / `rigel/openrouter` | — |

Rules:

- `probe()` reports `auth_mode: ApiKey | Subscription | None` and the UI shows **Needs API key** for the
  last two. Rigel never falls back to a subscription quietly.
- Keys are injected per spawn from the keychain and never written to a config file that outlives the
  process, except Cline's per-agent config, which lives under `%APPDATA%\Rigel\agents\<id>\`.
- A harness that starts in subscription mode anyway is killed at first event and reported as a
  configuration error, not a crash.
- Do not log env values, ever — the adapter's `Debug` impl redacts every `*_API_KEY` and `*_TOKEN`.

---

## 5. Domain model

Entities and the invariants that make them trustworthy. Field lists are the contract; §6 is the
storage realisation.

```rust
// crates/rigel-core/src/domain.rs
pub struct Project { pub id: ProjectId, pub root: PathBuf, pub name: String, pub created_at: i64 }
pub struct Session { pub id: SessionId, pub project: ProjectId, pub title: Option<String>,
                     pub status: SessionStatus, pub budget_cents: Option<u32>,
                     pub spend_cents: u32, pub started_at: i64, pub ended_at: Option<i64>,
                     pub interventions: u32, pub tasks_completed: u32 }
pub enum SessionStatus { Active, Idle, Interrupted, Ended }

pub struct Agent { pub id: AgentId, pub session: SessionId, pub harness: Harness,
                   pub parent: Option<AgentId>, pub role: Option<String>, pub model: Option<String>,
                   pub pid: Option<u32>, pub worktree: Option<PathBuf>, pub status: AgentStatus,
                   pub usage: Usage }
pub enum Harness { Native, Codex, Claude, Cursor, Cline }
pub enum AgentStatus { Starting, Idle, Running, WaitingApproval, Interrupted, Failed, Exited }

pub struct Turn { pub id: TurnId, pub agent: AgentId, pub seq: u32, pub status: TurnStatus,
                  pub started_at: i64, pub completed_at: Option<i64>, pub usage: Option<Usage> }

pub struct Item { pub id: ItemId, pub turn: TurnId, pub seq: u32, pub kind: ItemKind,
                  pub status: ItemStatus, pub body: serde_json::Value, pub raw: serde_json::Value }

pub struct Usage { pub input_tokens: u64, pub cached_input_tokens: u64, pub output_tokens: u64,
                   pub reasoning_tokens: u64, pub cost_cents: u32 }

pub struct Claim { pub id: ClaimId, pub path: RelPath, pub agent: AgentId,
                   pub base_hash: Option<Sha256>, pub acquired_at: i64,
                   pub heartbeat_at: i64, pub released_at: Option<i64>,
                   pub release_reason: Option<ReleaseReason> }

pub struct Patch { pub id: PatchId, pub agent: AgentId, pub path: RelPath,
                   pub base_hash: Sha256, pub result_hash: Sha256, pub diff: String,
                   pub status: PatchStatus, pub applied_at: Option<i64> }
```

### 5.1 Invariants

| # | Invariant | Enforced by |
|---|---|---|
| I1 | A turn ends in exactly one of `completed`, `failed`, `cancelled`. | `SessionService::finish_turn` is the only writer. |
| I2 | An item's `raw` is the untouched harness JSON. Normalisation is additive. | adapters write both; a test asserts `raw` parses. |
| I3 | A claim is never deleted, only released. History is the audit trail. | `claims.released_at`, no `DELETE`. |
| I4 | A patch applies only if `sha256(current file) == base_hash`. | `PatchPipeline::apply`, CAS check. |
| I5 | A patch row exists before the file changes; the journal is write-ahead. | insert row → fsync → rename. |
| I6 | `agents.spend_cents` equals the sum of its turns' costs; a mismatch fails a startup check. | `BudgetService::reconcile` on boot. |
| I7 | No absolute path appears in a committed file. | `MemoryService::validate` rejects drive letters and home prefixes. |
| I8 | A session resumes only from a checkpoint whose tree hash still matches; otherwise it is `Interrupted` and the user is prompted. | `SessionService::resume`. |

### 5.2 Item kinds

`message` · `reasoning` · `command` · `file_change` · `mcp_call` · `web_search` · `plan` · `approval` ·
`usage` · `error`.

Every adapter maps into these ten. Anything unmappable becomes `error` with `raw` preserved and
`body.unmapped = true` — visible in a debug pane, never silently dropped.

---

## 6. Storage

### 6.1 Layout

```
%APPDATA%\Rigel\                      (Linux: ~/.local/share/rigel, macOS: ~/Library/Application Support/Rigel)
  index.db                            global skill index + trust ledger (D3)
  adapters.json                       probed harness versions + capabilities
  settings.toml                       user settings, no secrets
  logs\rigel.log                      rotating, 10 MB × 3
  worktrees\                          spawned-agent worktrees
  blobs\<sha256>                      content-addressed skill bodies
  sidecar\                            bundled node runtime + sidecar/index.mjs (S4)

<repo>\.rigel\
  rigel.db                            session log (gitignored)
  rigel.db-wal
  memory\
    <agent-id>\
      MEMORY.md                       committed (D2)
      topics\<slug>.md                committed
  sessions\<session-id>\checkpoints\  gitignored
  .gitignore                          contains: rigel.db*  sessions/  worktrees/
```

**Why sessions are local but memory is committed:** sessions contain prompts, machine paths and
possibly secrets; committing them would leak by default and make every turn a diff. Memory is the
distilled, reviewed artifact that should travel. This is the interpretation of Q47 that Q56's split
requires.

### 6.2 Project database (DDL, migration 0001)

**Locking (D41).** WAL is a property of the file, not of a connection. `Store::open` sets it once
and retries if another process is holding the file; it is never requested through the connect
options, because sqlx applies requested pragmas on every connection it opens and that pragma
takes a lock `busy_timeout` cannot wait for. Migrations and `BudgetService::account` — both of
which read before they write — open with `BEGIN IMMEDIATE`, so a competing writer waits instead
of invalidating their snapshot.

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

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
```

### 6.3 Index database (DDL, migration 0001)

```sql
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
```

### 6.4 Rules

- Migrations: `migrations/0001_init.sql`, `0002_*.sql`; applied inside a transaction with the version
  row; `rigel doctor` reports drift. **Never migrate while unattended** (D13).
- Blobs are content-addressed and written before their row; orphans are collected by `rigel gc`.
- FTS5 is populated by triggers in the same migration (insert/update/delete on `skills`).
- Every query is parameterised; no string-built SQL anywhere.
- Backups: `rigel export` writes a single `.rigelpack` (tar.zst of index.db + blobs + memory), never
  the session DB with absolute paths.

---

## 7. Interfaces

### 7.1 MCP server Rigel hosts

One stdio server per spawned agent, injected at spawn time:

```toml
# written to the agent's config by the adapter, e.g. an appended table in ~/.codex/config.toml
[mcp_servers.rigel]
command = "rigel"
args = ["mcp", "--session", "<session-id>", "--agent", "<agent-id>"]
```

Tools (JSON Schema 2020-12). All return `{ "ok": true, ... }` or
`{ "ok": false, "error": { "code": string, "message": string } }`.

```jsonc
// rigel_claim — advisory. Never blocks.
{
  "name": "rigel_claim",
  "description": "Register intent to edit files. Advisory: other agents may still write. Returns the current holder if any.",
  "inputSchema": {
    "type": "object",
    "required": ["paths", "reason"],
    "properties": {
      "paths":  { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 50 },
      "reason": { "type": "string", "maxLength": 280 },
      "ttl_seconds": { "type": "integer", "minimum": 60, "maximum": 3600, "default": 900 }
    }
  }
}
// → { "ok": true, "claims": [{ "path": "src/App.tsx", "id": "...", "held": true,
//      "conflict_with": [{ "agent": "codex-2", "since": 1760000000 }] }] }

// rigel_release
{ "name": "rigel_release",
  "inputSchema": { "type": "object", "required": ["paths"],
    "properties": { "paths": { "type": "array", "items": { "type": "string" } },
                    "reason": { "type": "string" } } } }

// rigel_who — who holds what, and what changed without a claim
{ "name": "rigel_who",
  "inputSchema": { "type": "object",
    "properties": { "path": { "type": "string" }, "prefix": { "type": "string" } } } }
// → { "ok": true, "claims": [...], "unverified_changes": [{ "path": "...", "agent": "claude-1" }] }

// rigel_skills_search
{ "name": "rigel_skills_search",
  "inputSchema": { "type": "object", "required": ["query"],
    "properties": { "query": { "type": "string" }, "limit": { "type": "integer", "default": 8 },
                    "installed_only": { "type": "boolean", "default": false } } } }
// → { "ok": true, "results": [{ "id": "...", "name": "...", "grade": "B", "why": "matches 'ocr' in description",
//      "installed": false, "pinned_version": null }] }

// rigel_skill_load — returns the SKILL.md body of the session-pinned version
{ "name": "rigel_skill_load",
  "inputSchema": { "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } } } }
// → { "ok": true, "id": "...", "version": "sha256:...", "body": "---\nname: ...", "truncated": false }

// rigel_patch_propose — the only write path Rigel controls
{ "name": "rigel_patch_propose",
  "inputSchema": { "type": "object", "required": ["path", "base_hash", "new_content"],
    "properties": { "path": { "type": "string" }, "base_hash": { "type": "string" },
                    "new_content": { "type": "string" }, "summary": { "type": "string" } } } }
// → { "ok": true, "patch_id": "...", "applied": true }
// → { "ok": false, "error": { "code": "STALE_BASE", "message": "...", "current_hash": "..." } }

// rigel_budget
{ "name": "rigel_budget", "inputSchema": { "type": "object", "properties": {} } }
// → { "ok": true, "session_cents": 42, "cap_cents": 500, "agent_cents": 12, "remaining_cents": 458 }

// rigel_memory_write — committed, validated, path-redacted
{ "name": "rigel_memory_write",
  "inputSchema": { "type": "object", "required": ["topic", "content"],
    "properties": { "topic": { "type": "string", "pattern": "^[a-z0-9-]{1,48}$" },
                    "content": { "type": "string", "maxLength": 8000 },
                    "mode": { "type": "string", "enum": ["append", "replace"], "default": "append" } } } }
```

Writes are rejected with `PATH_LEAK` if `content` matches `[A-Za-z]:\`, `/Users/`, `/home/`, or an
`ssh-rsa` / `glpat-` / `sk-` prefix.

`run_command` (D30) and `fetch_url` (D33) are **native-agent tools only** and are deliberately absent
from this MCP surface: a harness already has its own shell and its own network, and routing either
through Rigel would turn one approval boundary into two disagreeing ones.

### 7.2 CLI (`rigel`, same binary)

```
rigel open [--project PATH]
rigel skills search "pdf ocr" [--json]
rigel skills get <id> [--to DIR] [--pin]
rigel skills index <repo|url> [--force]
rigel sessions list [--json]
rigel sessions resume <id>
rigel claim list [--path P] [--json]
rigel claim release <path> [--reason R]
rigel deps check [--missing] [--json]
rigel trust explain <package>
rigel doctor [--json]
rigel export [--out FILE]
rigel mcp --session S --agent A        # stdio MCP server, used by adapters
```

Exit codes: `0` ok, `1` user error, `2` precondition failed (stale base, unresolvable conflict),
`3` policy denied, `4` harness unavailable. `--json` is required on every command the UI shells out
to; never parse human output.

### 7.3 Tauri commands

| Command | Payload | Returns |
|---|---|---|
| `project.open` | `{ path }` | `Project` |
| `session.start` | `{ project, title?, budget_cents? }` | `Session` |
| `session.list` | `{ project }` | `Session[]` |
| `session.resume` | `{ session, force?: bool }` | `ResumeOutcome` |
| `session.end` | `{ session }` | `()` |
| `agent.spawn` | `{ session, harness, role?, model?, worktree: bool, prompt? }` | `Agent` |
| `agent.stop` | `{ agent, reason }` | `()` |
| `turn.send` | `{ agent, text, attachments?: FileRef[] }` | `Turn` |
| `turn.cancel` | `{ agent, turn }` | `()` |
| `approval.respond` | `{ approval, decision, scope }` | `()` |
| `claim.list` | `{ session }` | `Claim[]` |
| `claim.release` | `{ claim, reason }` | `()` |
| `conflict.resolve` | `{ conflict, choice }` | `()` |
| `patch.list` | `{ session }` | `Patch[]` |
| `patch.apply` | `{ patch }` | `()` |
| `patch.revert` | `{ patch }` | `()` |
| `skills.search` | `{ query, limit, installed_only }` | `SkillRow[]` |
| `skills.load` | `{ id }` | `{ body, version }` |
| `deps.check` | `{ host? }` | `ToolStatus[]` |
| `deps.install` | `{ tool, consent: 'auto' \| 'user' }` | `InstallRecord` |
| `budget.set` | `{ session, cap_cents }` | `Session` |
| `memory.read` / `memory.write` | `{ agent, topic }` / `{ agent, topic, content, mode }` | file / `()` |
| `settings.get` / `settings.set` | — | `Settings` |

Event channel `rigel://events`: `AgentEvent` (normalised), `SessionUpdated`, `ClaimChanged`,
`ConflictDetected`, `BudgetWarning`, `BudgetExceeded`, `ApprovalRequested`, `ApprovalResolved`,
`HarnessAvailability`. The UI subscribes once and routes by `type`.

### 7.4 Adapter trait

```rust
#[async_trait::async_trait]
pub trait Adapter: Send + Sync {
    fn harness(&self) -> Harness;

    /// Probe the installed binary: version, flags, capabilities. Cached in adapters.json,
    /// re-probed when the binary's mtime changes.
    async fn probe(&self) -> Result<HarnessInfo>;

    async fn start(&mut self, spec: StartSpec) -> Result<AgentHandle>;
    async fn send(&mut self, turn: TurnInput) -> Result<()>;
    async fn respond(&mut self, approval: &ApprovalId, d: Decision) -> Result<()>;
    async fn cancel(&mut self, turn: &TurnId) -> Result<()>;
    async fn interrupt(&mut self, agent: &AgentId) -> Result<()>;   // budget kill
    async fn shutdown(&mut self) -> Result<()>;

    fn events(&self) -> broadcast::Receiver<AgentEvent>;
}

pub struct StartSpec {
    pub agent: AgentId,
    pub cwd: PathBuf,            // live tree or worktree (D7)
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub sandbox: SandboxMode,
    pub approval_policy: ApprovalPolicy,
    pub mcp_config: Option<PathBuf>,
    pub extra_skill_roots: Vec<PathBuf>,   // session-pinned skill versions
    pub env: BTreeMap<String, String>,     // per-agent, from keychain (D14)
}

pub enum SandboxMode { ReadOnly, WorkspaceWrite, FullAccess }
pub enum ApprovalPolicy { OnRequest, Never, Auto }
```

The sidecar, when used, implements the same wire contract from Rust's point of view: the Rust adapter
spawns `node sidecar/index.mjs`, speaks NDJSON `{id, method, params}` / `{id, result}` / `{event}` and
re-emits `AgentEvent`s. No Rust code above the adapter knows the difference.

### 7.5 Native model client (OmniRoute first)

- Default is the local OmniRoute router at `http://127.0.0.1:20128/v1` (override with
  `RIGEL_OMNIROUTE_URL`). If `/api/health` is down, Rigel runs
  `omniroute serve --daemon --no-open` once and waits for health before sending. The default
  model is `gpt-5.6-sol-medium`.
- OpenRouter remains a provider (`--provider openrouter`, `RIGEL_PROVIDER`, or Settings).
- `POST {base}/chat/completions`, `stream: true`, `tools: [...]`,
  `usage: { include: true }` so a provider that reports cost does so.
- Model catalogue from `GET /api/v1/models`; cache 24 h in `index.db`.
- Retries: 3 attempts on 429/5xx with full jitter, honouring `Retry-After`; never retry a stream after
  the first token.
- Cost accounting: `usage.cost` if present, else a local price table keyed by model id; both stored in
  `Usage` so the reconcile check (I6) can flag drift.
- The key lives in the OS keychain under `rigel/openrouter`. Read at spawn, never written to disk.

---

## 8. Behaviours to implement exactly

### 8.1 Claim protocol (advisory)

```
on claim(paths, agent):
  for each path:
    normalize to project-relative, reject outside-root, reject > 200 chars
    if active claim exists for path:
       if claimant == agent: refresh heartbeat, continue
       else: record conflict_with, still grant (advisory) but mark degraded
    insert claim(base_hash = sha256(file) or null if new)
    emit ClaimChanged
  return all claims for the requested paths

on heartbeat(agent): update all active claims owned by agent
on stale scan (every 30 s): claims with heartbeat_at older than ttl → mark stale, emit ClaimChanged
  stale claims are NOT auto-released: the UI asks the user (Q32). Auto-release after 24 h, reason 'expired'.
```

The word **locked** must not appear in the UI, the CLI output, or the MCP tool descriptions.

### 8.2 Attribution and violation detection

Two sources, in priority order:

1. **Reported changes** — items of kind `file_change` (Codex) or `tool_use` with `Edit|Write|MultiEdit`
   (Claude/Cursor/Cline), plus the patch journal. Highest confidence.
2. **Observed changes** — a `notify` watcher on the project root, debounced 250 ms, hashing changed
   files. Attributing an observed change requires an agent whose turn is `running`; exactly one
   running → attribute; several → attribute to all and mark `ambiguous: true`.

A violation is: observed hash ≠ `claim.base_hash` AND writer ≠ claimant. Violations become rows in
`claim_conflicts` and `ConflictDetected` events. The UI shows a **Conflicts** strip above the diff with
claimant, writer, and a three-way diff against the base.

### 8.3 Budgets and the kill switch

```
quote(agent, prompt)                    → estimated first-step cost + headroom; advisory only
reserve(agent, turn, estimated_cents)   → hard fail if session spend + estimate > cap
account(agent, turn, usage)             → write usage, update agent + session spend
on each step's usage: if session spend > cap → end the turn there, interrupt every running agent, emit BudgetExceeded
on hard cap: adapter.interrupt(agent)   → ACP session/cancel, Codex turn interrupt, else SIGTERM
```

- Caps are set per session at start (default from settings, `$5.00`).
- A soft warning fires at 80%; the UI shows an inline approval in the transcript: *Raise cap / Stop*.
- `rigel_budget` is available to agents so a coordinator can plan, not discover.
- Post-hoc accounting only — no provider supports a hard stop mid-stream; the guarantee is "stops
  within one turn".

### 8.4 Patches

```
propose(path, base_hash, new_content):
  lock the path in-process (single mutex per project)
  current = read(path); if sha256(current) != base_hash → STALE_BASE with current_hash
  diff = unified(base, new)
  insert patch(status='proposed', diff, base_hash, result_hash)
  write temp file in the same directory, fsync, rename over path
  fsync directory
  update patch(status='applied', applied_at)
```

Revert applies the inverse only if `sha256(current) == result_hash`; otherwise `conflicted` and the
user resolves. This is the only path by which Rigel itself writes project files (D8, D31). Shell
commands approved under D30 do not go through it — their filesystem effects are observed and attributed
by the Phase 5 watcher, not journalled.

### 8.5 Resume

```
resume(session):
  if latest checkpoint.tree_hash == current tree hash → resume cleanly from turn seq
  else:
     session.status = Interrupted
     for each claim held by an agent whose pid is gone:
        emit prompt: "Session stopped mid-write on <path>. Release claim?"
     never auto-release, never auto-apply a patch (Q57)
```

Recording a checkpoint: after every completed turn, store `tree_hash` (git tree hash of the worktree,
or a Merkle hash of tracked file hashes when git is unavailable) plus the turn's patch ids.

Process liveness: `agents.pid` plus a boot-time process scan; a recycled PID is detected by comparing
process start time against `started_at`.

### 8.6 Unattended mode

Toggled per session. While on: no installs, no migrations, no `git push`, no worktree pruning. Agents
keep running and queue their requests; the UI shows a **Queued (3)** badge over the approval tray.

### 8.7 Skill invocation

A skill is a tool. `rigel_skill_load` returns the pinned SKILL.md body; the agent then acts on those
instructions with its own tools, under its own sandbox. Rigel does not execute skill scripts: the gate
in "capabilities are gated" is the agent's tool policy, not a second sandbox Rigel invents.

Pinning: at session start, every skill the session may use is resolved to a `body_sha`; loading always
returns that version even if the index updates. A mid-session version change is a banner with the diff,
and requires an explicit "update pinned version" to take effect.

### 8.8 Intervention accounting

Increment `sessions.interventions` on: approval answered, conflict resolved, stale claim released,
resume confirmed, install approved, budget raised, unpinned skill accepted. Increment
`sessions.tasks_done` when a turn completes with no error item and the agent reports completion.
The status bar shows `interventions / tasks` — the metric from D17, visible from day one.

---

## 9. UI surfaces

Four, mapped to the existing design tokens. The prototype's three views become windows into one shell.

| Surface | Content | Key states |
|---|---|---|
| **Shelf** (`Ctrl+1`) | search, skill rows with grade + install state, detail pane, evidence table, variants | empty (teaches), loading >400 ms, offline (cached, muted banner), stale source (last-seen date) |
| **Session** (`Ctrl+2`) | agent roster (left), transcript (centre), inspector (right: tools, approvals, budget) | running, waiting-approval, interrupted, budget-exceeded, harness-missing |
| **Diff** (`Ctrl+3`) | one merged diff, attribution gutter per hunk, conflict strip, per-patch revert | conflicted, stale-base, reverted |
| **Settings** (`Ctrl+4`) | providers + keys, harness paths, caps, unattended default, install thresholds, theme | unsaved, key-missing, harness-unavailable |

Roster rows: harness icon, role, model, status dot, spend, claims count. Clicking filters the
transcript. A nested subagent indents one level and is labelled **observed only** when the harness does
not report its internals (§4.3.5).

Status bar (permanent):
`model · branch │ $0.42 · 18.2k tok │ 3 skills │ 2 claims · 1 unverified │ 1/2 interventions/tasks`.
Everything in tabular figures.

Claims render as `claimed by codex-2 · unverified`, amber outline, never a lock icon.

---

## 10. Security model

| Boundary | Rule |
|---|---|
| Credentials | OS keychain (`keyring` crate), one entry per provider and per harness; never in config, never in env dumps, redacted in logs |
| Agent isolation | Per-agent env allowlist; no inherited full environment; per-agent MCP server instance scoped to its `agent_id` |
| Skill trust | Skills are instructions from strangers. Loading one is an action the transcript shows; scripts run under the agent's own sandbox |
| Path safety | All paths normalised, canonicalised, rejected if outside the project root; symlinks resolved before the check |
| Patch safety | CAS on `base_hash`; no writes outside root; binary files refused |
| Memory | Committed, validated, secret-pattern rejected (I7) |
| Sidecar | No DB access, no claim authority, no approvals; its events are re-normalised by the Rust adapter that owns the agent |
| Network | No telemetry. Outbound calls: OmniRoute on loopback (native agent, which fans out to providers), OpenRouter when selected, GitHub/marketplaces (indexing, on demand), update manifest (opt-out) |
| Router auth | OmniRoute answers `/v1/chat/completions` and `/v1/responses` from localhost **without a key** (probed 21 Oct); `/v1/models` returns 401. Rigel sends its key regardless, never treats the router as an auth boundary, and `rigel doctor` warns when router auth is disabled |
| Logs | Redact tokens by pattern; rotate at 10 MB; local only |
| Third-party terms | Rigel never reuses a vendor subscription's auth; each harness authenticates itself (§4.3.3) |

Prompt-injection stance, stated because agents read skill bodies and repo files: Rigel treats repo
content, skill content and agent output as **data**. No agent output is executed as a Rigel command.
Approvals are bound to an `approval_id` issued by the harness, not to text in the transcript.
---

## 11. The plan

**State as of 21 Oct 2026:** Phases 0–3 complete; Phase 4 in progress.
Run `node scripts/handoff-status.mjs` for live counts; this table is the summary.

| Phase | Scope | State | Items |
|---|---|---|---|
| 0 | Prototype | **complete** | 9/9 |
| 1 | Rust skeleton and storage | **complete** | 10/10 |
| 2 | The shelf, real | **complete** | 11/11 |
| 3 | Native agent, chat and the bill | **complete** | 11/11 |
| 4 | Harness adapters | in progress | 1/19 |
| 5 | Claims, attribution, merged diff | not started | 0/8 |
| 6 | Dependencies and trust | not started | 0/8 |
| 7 | Hardening and open-source release | not started | 0/9 |

Phases are sequential. Every item has an ID; tick it in the commit that finishes it (§How to use).

### Phase 0 — Prototype *(complete)*

- [x] **P0.1** Working UI prototype: package browser, discover, targets, command palette, status bar — `a3186eb`
- [x] **P0.2** Live discovery across GitHub, SkillsMP and SkillHub through the dev proxy — `e4f830b`
- [x] **P0.3** Duplicate detection and on-disk diffing (446 rows, 293 absent) — `e4f830b`
- [x] **P0.4** Dependency catalogue: 20 tools with real winget/brew commands — `e4f830b`
- [x] **P0.5** Per-harness measurement plus `scripts/verify-ecc.mjs` — `e4f830b`
- [x] **P0.6** Competitor teardown (`docs/phase-0-teardown.md`) — `e4f830b`
- [x] **P0.7** 60-question grilling with collisions recorded (`docs/interview-answers.md`) — `b88d59f`
- [x] **P0.8** Rename to Rigel: icon, title, favicon, package name, READMEs — `b88d59f`
- [x] **P0.9** `npm run build` clean — `b88d59f`

### Phase 1 — Rust skeleton and storage *(1.5 weeks)*

**Goal:** a Tauri app that opens, creates a project, stores a session and reads it back — with the test
harness every later phase depends on.

- [x] **P1.1** Cargo workspace: `rigel-core`, `rigel-adapters`, `rigel-mcp`, `rigel-cli`, `rigel-app`; empty traits compile — `af50117`
- [x] **P1.2** `sqlx` pool, numbered migration runner, `schema_migrations` written transactionally — `af50117`
- [x] **P1.3** Project database at `0001_init.sql` exactly as §6.2 — `af50117`
- [x] **P1.4** Index database at `0001_init.sql` as §6.3, including FTS5 triggers — `af50117`
- [x] **P1.5** `SessionService`: create, list, end, append turn/item, write checkpoint — `e37493b`
- [x] **P1.6** Tauri commands `project.open`, `session.start`, `session.list`, `settings.get/set` — `e37493b`
- [x] **P1.7** UI shell ported to `ui/`: rail, status bar skeleton, Shelf on `MockSkillSource` — `e37493b`
- [x] **P1.8** CI on a Windows runner: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `npm run build` — `e37493b`
- [x] **P1.9** The six non-negotiable tests from §12 exist as `#[ignore]` stubs with names and intent — `e37493b`
- [x] **P1.10** **Exit test:** open this repo, create a session, restart the app, the session is listed with its original `created_at` — `e37493b`

**Not yet:** agents, adapters, MCP, the index migration.

### Phase 2 — The shelf, real *(2 weeks)*

**Goal:** the index becomes the app's own, offline-capable, and queryable by an agent.

- [x] **P2.1** `trait Source` plus the GitHub source: one tree call, ETag cache, raw bodies — `45bdfec`
- [x] **P2.2** Frontmatter parser and static safety scan ported from `prototype/src/github.ts` — `45bdfec`
- [x] **P2.3** Content-addressed blob store; bodies never in SQLite — `45bdfec`
- [x] **P2.4** FTS5 search with a documented tiebreak (grade → stars → recency) — `45bdfec`
- [x] **P2.5** SkillsMP and SkillHub connectors returning `SourceStatus` including `stale` + `last_seen_at` — `45bdfec`
- [x] **P2.6** SimHash duplicate clustering; "N variants" UI that never collapses rows — `45bdfec`
- [x] **P2.7** Evidence table with measured/advertised/source/date and an explicit "unmeasured" label — `45bdfec`
- [x] **P2.8** CLI `skills search|get|index --json` with the exit codes from §7.2 — `45bdfec`
- [x] **P2.9** One-time migration of the prototype's `skilldb:` localStorage rows, then removal — `45bdfec`
- [x] **P2.10** Shelf wired to real data, including offline and stale states — `45bdfec`
- [x] **P2.11** **Exit test:** with the network off, a search over 40+ matching skills returns in <50 ms from a cold start; re-indexing the same repo makes zero HTTP requests — `45bdfec`, results in `docs/qa/p2.md`

**Not yet:** MinHash and tree-signature clustering (one signal of three), measured evidence
(Phase 6 produces it), installs (Phase 6), skill tool invocation (Phase 3 for the MCP surface).

**Not yet:** installs (Phase 6), skill tool invocation (Phase 3 for the MCP surface).

### Phase 3 — Native agent, chat, and the bill *(2.5 weeks)*

**Goal:** a usable chat pane with tool calls, streaming, cost accounting, budget caps and replay.

- [x] **P3.1** Model client: streaming SSE, tool-call parsing, retry-with-jitter, `Retry-After` — `c23b268`
- [x] **P3.2** Cost accounting from response usage, local price fallback, `BudgetService::reconcile` — `c23b268`
- [x] **P3.3** Native coordinator loop (D19) with the tool registry from §7.1 — `c23b268`
- [x] **P3.4** `rigel-mcp` stdio server, scoped by `--session` and `--agent` — `c23b268`
- [x] **P3.5** Patch pipeline: CAS check, write-ahead journal, fsync, revert — `c23b268`
- [x] **P3.6** Memory service: committed files, path/secret validator (I7), `memory_index` — `c23b268`
- [x] **P3.7** Transcript UI: role gutter, collapsed tool lines, inline approvals, streaming text — `c23b268`
- [x] **P3.8** Budget reserve/account/interrupt, pre-turn quote in the composer, 80% inline prompt, cap editing — `c23b268`
- [x] **P3.9** Replay from `raw_json` with a raw/debug toggle — `c23b268`
- [x] **P3.10** Intervention counter in the status bar (`interventions / tasks`) — `c23b268`
- [x] **P3.11** **Exit test:** $0.25 cap, read three files, patch one; kill mid-turn, relaunch, resume; no patch applied twice and `git diff` is byte-identical — `scripts/p3-exit-test.sh`, results in `docs/qa/p3.md`; `c23b268`

**Not yet:** tool and harness approvals (P4). The native loop's only inline prompt is the 80% cap
one; `approval` items are stored and rendered, but no adapter can answer them until Phase 4.

### Phase 4 — Harness adapters *(3 weeks)*

**Goal:** Codex, Claude Code, Cursor and Cline as first-class agents — each authenticated by **its own
API key**, never a subscription login (D21) — with approvals and normalised events.

**Order (D36):** P4.1 → P4.2 → P4.5 (Codex, with P4.7/P4.8 alongside it) → P4.3 (Cursor ACP) → P4.4 →
P4.10, then P4.6 (Claude) once an `ANTHROPIC_API_KEY` exists. P4.13–P4.17 come before P4.6: they are
what makes the approval surface worth having.

- [ ] **P4.1** `probe()` for all four, writing `adapters.json`; UI lists missing harnesses with an install hint
- [ ] **P4.2** Per-agent keychain injection; probe reports `auth_mode` and refuses `subscription` (D21)
- [ ] **P4.3** ACP client: `agent acp`, session updates, `session/request_permission` → approval UI
- [ ] **P4.4** ACP client: `cline --acp`, same client, separate probe
- [x] **P4.5** Codex adapter: app-server JSON-RPC primary (probe passed 21 Oct), `codex exec --json` fallback behind the trait. Write the `-c` overrides explicitly; do not shell out to `omniroute run`/`launch-codex` — `7b3c2fb`
- [ ] **P4.6** Claude adapter: `claude -p --output-format stream-json --verbose --include-partial-messages --permission-prompt-tool mcp__rigel__approve`
- [ ] **P4.7** Golden fixtures per harness per scenario (start, tool call, approval, error, interrupt) + replay tests
- [ ] **P4.8** Normalisation for all ten item kinds; unmappable events retained as `unmapped`
- [ ] **P4.9** Roster UI (D40): nested subagents, `observed only` labels, per-agent spend and claims; at most two transcripts side by side, no tabs or grids
- [ ] **P4.10** End-to-end approvals across all three protocols, with scope buttons (N2)
- [ ] **P4.11** Sidecar decision recorded: implement it for Cline SDK-only features, or mark `dropped` with the reason
- [ ] **P4.12** **Exit test:** Codex, Cursor and Cline each run `rigel doctor --json`; all three show a real command item and exit code; killing one leaves the others running and marks it `Interrupted`
- [ ] **P4.13** `run_command` tool (D30): exact text and scope shown before execution (N2), cwd jailed to the project, timeout and output cap, exit code recorded as a `command` item
- [ ] **P4.14** Egress tool (D33): `fetch_url` with approval per origin and once/session/always scopes, size and content-type caps, fetched text labelled untrusted in the transcript
- [ ] **P4.15** Destructive classifier (D32): deletes outside the project, force-push/reset, recursive remove, registry writes — each maps to a policy row with its own test
- [ ] **P4.16** Deliberation panel (D32): three read-only agents argue pro and con in one round with no tool access beyond read; the record attaches to the approval item, and the gate stays deterministic — it cannot be voted away
- [ ] **P4.17** Pre-destructive snapshot (D32, D38): `git stash create` for tracked changes, untracked non-ignored files copied to `%APPDATA%\Rigel\snapshots\<turn>\` with a manifest, `rigel snapshot restore <id>`; non-git directories copy the whole tree under a 2 000-file / 200 MB cap and refuse above it. No disk image, no restore point
- [ ] **P4.18** **Exit test:** a destructive command triggers deliberation, snapshot and approval; deny leaves the tree byte-identical; approve records the snapshot id and the command text, and restoring the snapshot reproduces the pre-turn tree
- [ ] **P4.19** Codex context budget (D29, N3): record input tokens per turn, trim or disable the skills-catalogue injection, and surface the per-turn estimate before sending. The 21 Oct probe spent 155,895 input tokens on a one-word answer

**Not yet:** claims enforcement (Phase 5), worktrees (Phase 5), durable shell transcripts, and any
egress outside an approved origin.

### Phase 5 — Claims, attribution, merged diff *(2 weeks)*

**Goal:** several agents on one tree, honestly coordinated.

- [ ] **P5.1** Claim ledger: acquire, heartbeat, stale scan, 24 h expiry, never `DELETE`
- [ ] **P5.2** MCP tools `rigel_claim`, `rigel_release`, `rigel_who` with the schemas from §7.1
- [ ] **P5.3** Watcher plus attribution: reported changes first, observed changes second, `ambiguous` flag
- [ ] **P5.4** Conflict rows and the three-way diff view
- [ ] **P5.5** Worktrees for spawned agents (`%APPDATA%\Rigel\worktrees\…`) and merge-back through the patch pipeline
- [ ] **P5.6** Merged diff UI: per-hunk attribution gutter, conflict strip, per-patch revert
- [ ] **P5.7** CLI `claim list|release`
- [ ] **P5.8** **Exit test:** two agents edit one file; the conflict surfaces within 2 s with both attributions; the UI never displays the word "locked" (asserted by a string test)

### Phase 6 — Dependencies and trust *(1.5 weeks)*

**Goal:** missing tools stop being a dead end without becoming a supply-chain hole.

- [ ] **P6.1** Tool detection per host (`windows`, `wsl:<distro>`) with version cache in `tool_status`
- [ ] **P6.2** Requirement inference from `allowed-tools` plus a body scan, showing the source line
- [ ] **P6.3** Trust score from §11.6 weights, with `rigel trust explain <pkg>` printing the arithmetic
- [ ] **P6.4** Install execution: command shown first, ledger row, exit code and duration recorded
- [ ] **P6.5** Unattended mode queues installs instead of running them (D13)
- [ ] **P6.6** Shell restart after install, with the agent told to re-check (Q40)
- [ ] **P6.7** Signature/hash check where the package manager exposes one
- [ ] **P6.8** **Exit test:** on a machine without `ripgrep`, the requirement shows its source line; the install runs only after approval; the new terminal sees the binary; with unattended on, the same action queues

### Phase 7 — Hardening and open-source release *(1.5 weeks)*

- [ ] **P7.1** `rigel doctor`: DB integrity, migrations, probes, orphan blobs, stale claims, budget reconcile, keychain, config drift
- [ ] **P7.2** Crash-recovery matrix: kill during patch apply, migration, install, checkpoint — each recovers cleanly
- [ ] **P7.3** Docs: README rewritten around the workspace, CONTRIBUTING, SECURITY (from §10), ARCHITECTURE
- [ ] **P7.4** LICENSE chosen and applied
- [ ] **P7.5** Signed Windows installer; WebView2 bootstrapper; minimum Windows 10 1809
- [ ] **P7.6** macOS and Linux builds documented as unsupported-but-buildable, or marked `dropped`
- [ ] **P7.7** Update manifest with an opt-out switch and signature check (N5)
- [ ] **P7.8** Migration note for anyone who used the prototype's localStorage index
- [ ] **P7.9** **Exit test:** clean Windows VM, signed build, open a repo, run the shelf, run one native task, `rigel doctor` green, no Node install, no unexpected network calls

### 11.1 Sequencing rationale

Phases 1–3 are the shortest path to something the author would open on purpose: a shelf that works and a
chat that bills honestly. Phase 4 is the expensive one (four protocols) and comes after the UI has a
shape, so adapter work lands in a stable shell. It now starts with one harness rather than four in
parallel (D36): two of the four cannot authenticate on this machine, and a single working approval
path is worth more than four half-tested ones. Phase 5 is the differentiator but needs adapters to be
worth anything. Phase 6 is separable and can be pulled earlier if a real task blocks on it. Phase 7 is
not optional: an unsigned, undocumented binary is not the deliverable D59 describes.

### 11.2 Total

~15 weeks of focused work, roughly four and a half months of evenings — D30, D32 and D33 add about a
week and a half to Phase 4. If that is too long, the honest cut is Phase 4 down to **Codex only**
(D36): ~2 weeks saved, and the three shell/egress powers still land because they are what the native
agent is missing today.

---

## 12. Testing

| Layer | What | Where |
|---|---|---|
| Unit | claim state machine, trust arithmetic, budget reserve/account, path normalisation, memory redaction | `crates/rigel-core/tests` |
| Golden fixtures | one recorded session per harness per scenario (start, tool call, approval, error, interrupt) | `crates/rigel-adapters/tests/fixtures` |
| Fake harness binaries | small bins speaking ACP / app-server / NDJSON with scripted hangs and malformed lines | `crates/rigel-adapters/tests/bin` |
| Integration | patch CAS, resume after kill, conflict detection on a real git repo, worktree merge-back | `crates/rigel-core/tests/it` |
| UI | Playwright over the four surfaces' key states | `ui/e2e` |
| Manual | per-phase script in `docs/qa/<phase>.md`, run before ticking the exit test | — |

Non-negotiable tests, by name:

1. `patch_apply_is_cas` — stale base returns `STALE_BASE` and writes nothing.
2. `resume_never_duplicates_patches` — kill between journal write and rename; assert idempotence.
3. `claim_never_reports_locked` — string scan over UI copy and MCP descriptions (N1 as a test).
4. `budget_stops_within_one_turn` — fake adapter emits usage above cap; assert interrupt called once.
5. `memory_rejects_paths` — every pattern in §7.1.
6. `fixture_replay_matches_normalisation` — recorded harness streams normalise to expected items.

A failing test is fixed by fixing the cause. No suppressions, no `#[ignore]` above, no snapshot updates
without reading the diff.

---

## 13. Risks

| # | Risk | Likelihood | Impact | Mitigation / tripwire |
|---|---|---|---|---|
| R1 | Codex app-server changes or is dropped (experimental) | high | medium | `codex exec --json` fallback; fixtures pin behaviour; feature-detect every call |
| R2 | Claude stream schema drift | medium | medium | golden fixtures; unknown types become `unmapped`, never dropped |
| R3 | Protocol work overruns Phase 4 | high | high | timebox each adapter to 5 days; ship Codex + Claude first if late |
| R4 | Advisory claims disappoint after the demo | medium | high | "unverified" in the UI from day one; the merged diff, not the lock, is the safety net |
| R5 | Cost accounting drifts from real bills | medium | medium | reconcile check I6; `usage.cost` preferred; weekly doctor warning |
| R6 | Installing dependencies becomes a supply-chain hole | medium | high | trust thresholds + ledger + unattended ban + signature check |
| R7 | Memory commits leak secrets or paths | medium | high | validator (I7) + tests; memory writes appear in the diff for review |
| R8 | Tauri + WebView2 quirks on old Windows | low | medium | minimum Windows 10 1809; bootstrapper in installer |
| R9 | Scope creep into an IDE | high | high | §1 "what it is not" is a review checklist; no editor buffer, ever |
| R10 | A harness changes its auth rules or pricing | medium | medium | D21 keeps every harness on its own API key, so a vendor login change cannot break the app |

---

## 14. Appendix

### 14.1 Target workspace layout

```
Cargo.toml                       # workspace
crates/
  rigel-core/      domain, storage, session, claims, patches, budget, memory, trust, skills
  rigel-adapters/  acp/, codex/, claude/, native/, probe, normalise
  rigel-mcp/       stdio MCP server
  rigel-cli/       `rigel` binary (clap)
  rigel-app/       Tauri 2 shell, commands, event bridge
sidecar/           optional Node bridge for SDK-only capabilities (S4), bundled, lazily started
ui/                React 19 + TS + Vite (moved from prototype/)
  src/components/  shelf/, session/, diff/, settings/, shell/
  src/state/       zustand stores, event router
  e2e/             Playwright
docs/
migrations/        numbered SQL
scripts/
```

Tauri's `tauri.conf.json` may live in `crates/rigel-app`; the frontend `dist` path points at `ui/dist`.

### 14.2 Module responsibilities

| Module | Owns | Must not |
|---|---|---|
| `session` | turns, items, checkpoints, replay | know about harnesses |
| `claims` | claim ledger, heartbeats, conflicts | write files |
| `patches` | CAS apply, journal, revert | decide policy (that is `trust`/`approvals`) |
| `budget` | reserve, account, interrupt signal | call adapters directly (emits an event) |
| `skills` | index, FTS, pinning, tool wrapping | execute scripts |
| `memory` | committed files, validation | store paths |
| `trust` | scores, ledger, policy | run installs (that is `install`) |
| `adapters::*` | spawn, protocol, normalise | write project files, own storage |
| `rigel-mcp` | tool surface for agents | bypass core services |

### 14.3 Glossary

- **ACP** — Agent Client Protocol, JSON-RPC over stdio; Cursor and Cline speak it.
- **Advisory claim** — a registered intent to edit, unenforced by the OS or the harness.
- **Attribution** — mapping a filesystem change to the agent that caused it.
- **Intervention** — any user action that unblocks progress; the metric's denominator.
- **Item** — one normalised event inside a turn (message, command, file change…).
- **Pin** — binding a session to a specific skill body hash.
- **Shelf** — the skill catalogue surface.
- **Turn** — one prompt → one completion, including tool calls.
- **Worktree** — a git checkout at a separate path, used to isolate spawned agents.

### 14.4 Where the intent comes from

- `docs/interview-answers.md` — 60 answers, seven collisions, reasoning behind D1–D17.
- §15 below — the round-2 questions (Q67–Q86) and their answers, recorded as D22–D37.
- `docs/phase-0-teardown.md` — competitor evidence and the gap table.
- `docs/design-prompt.md` — the visual system (N8).
- `docs/skill-intelligence.md` — discovery, fingerprinting, safety, ranking detail.

Where those documents conflict with §3, §3 wins. Where they make claims about external tools, the
vendor docs linked in §4.2 win over both.

### 14.5 First commit for the implementing agent

Do not start with the UI. Start with the contract:

1. Create the workspace, the two DDL files and the migration runner.
2. Land §12's six tests as `#[ignore]`d stubs with their names and one-line intent — un-ignore each as
   its phase lands.
3. Write `rigel doctor --json` returning the *empty* shape it will eventually fill.

That sequence gives every later phase somewhere to put its evidence, and designs out the failure mode
"it works on my machine but nobody can check".

---

## 15. Open questions — round 2 (Q67–Q86)

Twenty decisions that shape the next phases. **Answered 21 Oct 2026.** Answers are recorded as D22–D37
in §3.1 and summarised in §15.0. The option lists stay as the record of what was offered; the
`(recommended)` markers show the defaults that were suggested, not necessarily the outcome.

### 15.0 Answers

| Q | Answer given | Decision |
|---|---|---|
| 67 | Resume the newest open session on launch | D22 |
| 68 | Read-only unified diff per patch, with revert | D23 |
| 69 | One window; switch projects in-app | D24 |
| 70 | Palette covers everything | D25 |
| 71 | Several providers at once, one per agent; OmniRoute is fine if it can front all the keys | D26 |
| 72 | Auto-start, retry once, then a clear failure | D27 |
| 73 | Picker with a saved default | D28 |
| 74 | Separate reasoning-effort setting | D28 |
| 75 | Start with the app | D27 |
| 76 | `estimated` plus token counts | D29 |
| 77 | Full shell, approval per command | D30 |
| 78 | Auto-apply, journaled and revertible | D31 |
| 79 | Agents deliberate, snapshot first, then ask | D32 |
| 80 | Fetch with per-origin approval | D33 |
| 81 | Budget follows real provider usage | D29 |
| 82 | Pooled under the session cap | D34 |
| 83 | Auto-end after 24 h | D35 |
| 84 | Implementer's choice → Codex, since there is no Claude subscription | D36 |
| 85 | Wire `rigel-mcp` in from day one | D36 |
| 86 | Signed MSI, opt-in updater, MIT/Apache-2.0, no telemetry | D37 |

Four answers were free text rather than a letter; each is worked out below before the question list.

### 15.1 "Multiple providers for multiple agents at once" (Q71 → D26)

The ask, restated: several upstream providers usable **simultaneously**, with each agent able to sit on
a different one, using keys already on this machine — and OmniRoute is acceptable *if* it can front all
of them.

It can, and this is the one place where the router is genuinely the right layer. Verified on this
machine with `omniroute providers list`: seven provider connections are already configured
(`antigravity`, `claude` — marked **banned**, `codex`, `codex-cloud`, `huggingface`, `kiro`,
`opencode`), and the CLI exposes `providers add|edit|rotate|metrics`, `combo`, `simulate` and
`cost --group-by api-key`. One OpenAI-compatible surface, many upstreams.

The design that follows:

- **Binding, not global state.** `agent` rows gain `provider` and `model`; the client is resolved per
  agent at spawn, not per process. Two agents on two providers is two clients, not a new architecture —
  turns already run on tokio.
- **One OmniRoute API key per agent** (`omniroute keys add`, `keys policy`, `keys expiration`). This is
  what makes concurrency safe: per-key budget, per-key policy, and `cost --group-by api-key` gives the
  roster exact per-agent spend without inventing an attribution scheme.
- **Direct fallback stays.** OpenRouter remains a real second path (D18), so a router outage is not a
  total outage.
- **Untested:** two simultaneous streams through one router. It is a ten-minute check — two turns in
  flight on different keys, confirm both complete and bill separately — and it belongs in P4.2.

One thing this does *not* decide: whether several agents are **visible side by side**. D24 keeps one
window and a project switcher; a split roster view is a separate call. Ask before building it.

### 15.2 The round table before a destructive action (Q79 → D32)

Taken literally, the answer is three mechanisms, not one:

1. **Deliberation.** Three read-only agents argue pro and con for one round, no tools beyond read, and
   the transcript attaches to the approval item. This is advisory. The gate itself stays deterministic:
   a classifier marks the operation destructive, and *that* is what requires approval. Agents must not
   be able to vote the gate away, or a prompt-injected agent becomes the exploit. (P4.16)
2. **Snapshot.** "Back up the PC first" is scoped to the state that has no other copy — the uncommitted
   tree (D38): a `git stash create` object for tracked changes, plus a copy of untracked non-ignored
   files. `rigel snapshot restore <id>` puts both back. A disk image needs VSS and elevation; a Windows
   restore point is deliberately *not* used, because it leaves project files alone and would protect
   everything except the work at risk. (P4.17)
3. **Approval.** Exact command and scope, as N2 already requires. (P4.15)

Cost and latency, so nobody is surprised: one round of three small agents is roughly 2–4k output tokens
— a few cents — and 5–15 s of wall clock. That is fine for a rare operation and unacceptable for every
write, which is exactly why it only fires on the classifier's say-so.

### 15.3 "Budget from the API usage" (Q81 → D29)

The instinct is right and the mechanism needs splitting in two, because provider-reported usage **lags**
by minutes and is only as granular as the key. It cannot be the thing that stops a turn.

- **Enforcement stays local.** The ledger already charges every step as it lands and stops within one
turn (P3.8, §12). That remains the hard cap. D10 is unchanged.
- **Reconciliation reads the provider.** `rigel doctor` compares the ledger against
  `omniroute cost --group-by api-key` (and `usage quota|utilization` for upstream limits), plus
  OpenRouter's key endpoint for credits. Drift over ~2% is a visible warning, not a silent correction.
- **Prices come from the router** where it exposes them, which fixes the gap that made
  `gpt-5.6-sol-medium` fall back to the conservative default.
- **Per-agent keys make it exact** — same mechanism as D26, which is why these two decisions belong
  together.

The answer did not name a window. Until someone does, the cap stays **$5 per session**; a daily or
weekly ceiling is a separate setting and should not be invented.

### 15.4 Codex first (Q84 → D36)

No Claude subscription, and the router's Claude connection is banned, so Claude Code would be built
against an account that cannot answer. Codex's connection is active, `codex exec --json` is its stable
headless surface, and `omniroute setup-codex` already generates profiles pointed at the router.

So: Codex first, pointed at the router with explicit `-c` overrides rather than OmniRoute's own
launcher, which is broken on this machine (see §4.2). With `requires_openai_auth=false` no key is even
needed — the router answered without one — but Rigel sends its key anyway so the setting keeps working
once router auth is enforced. If a direct OpenAI key is preferred later, it is one line behind the
adapter trait. `rigel-mcp` is wired in from day one (Q85): it is how approvals and skill tools reach any
harness, and retrofitting it after the adapter is written is the expensive order.

The probe also killed an assumption: a one-word Codex turn reported **155,895 input tokens** because
Codex auto-loads this machine's 2,378 skills. At $3/M input that is ~$0.47, so the $5 session cap buys
about ten Codex turns. Trimming that injection is P4.19, not a Phase 7 nicety.

### 15.5 Closed, 21 Oct 2026 — D38–D40

**Snapshot scope → D38: uncommitted work, not a restore point.**
Windows System Restore rolls back system files, drivers, the registry and installed programs; Microsoft's
own wording is that it **does not affect personal files**
([Microsoft Q&A](https://learn.microsoft.com/en-sg/answers/questions/6016372/how-do-i-fix-my-system-restore-to-an-earlier-date)).
For a coding agent that is precisely backwards — it would leave the project untouched and reconfigure
the machine. It also needs elevation and takes minutes.

What actually protects the work is the state that has no copy anywhere else: the uncommitted tree.
`git stash create` writes a stash commit without changing the index, the worktree or any ref, and the
manifest records its SHA; untracked non-ignored files are copied next to it. Restore is
`git checkout <sha> -- .` plus the copied files, exposed as `rigel snapshot restore <id>`. Committed
work needs no snapshot. A non-git directory — or a repository with no commits yet — gets the whole tree
copied, capped at 2 000 files / 200 MB; above the cap Rigel refuses and asks for an explicit skip
rather than pretending it took a backup.

**Codex upstream → D39: router by default, no silent fallback.**
An `omniroute setup-codex` profile with a Rigel-owned router key keeps the native agent and Codex on one
bill and one key policy, which is the whole reason D26 chose the router. The risk is concrete:
`codex exec --json` talks OpenAI-compatible HTTP and should pass through fine, while app-server is
version-dependent and may not honour a custom base URL. So P4.5 probes both before anything is built on
top: if `exec --json` fails through the router, the default flips to the direct OpenAI key and the reason
is recorded here. The setting is per agent (`codex.upstream`), so this is one flag either way, and a
missing upstream fails loudly instead of quietly billing another provider.

**Agent visibility → D40: roster plus one split pane.**
One window (D24) does not mean one transcript. The roster lists every agent — status, spend, claims,
`observed only` — and any two of them can be shown side by side for a comparison. Two is the cap:
three panes at 13px body text stops being an instrument and becomes a spreadsheet. Everything beyond
two is the roster's job.

### A. The shape of a working day

**67. What does Rigel show when it opens?**
Today it always opens Shelf, and a project must be typed in before Session works.

- **A.** Resume the newest open session for the last project, with Shelf one click away. *(recommended)*
- **B.** Keep Shelf as the launch screen; sessions stay a deliberate second step.
- **C.** Open a session list first, scoped to the last project.

*Blocks:* shell routing, remembering the last project. *Today:* Shelf only.

**68. How much of the Diff surface do you actually want?**
The rail has a Diff item with no real content; patch diffs are already stored and rendered inside
Session.

- **A.** A read-only merged diff with per-hunk attribution, opened from a patch or a session. *(recommended)*
- **B.** An editor: accept/reject individual hunks and apply from Diff.
- **C.** Drop the Diff tab; the transcript and patch detail are enough.

*Blocks:* P5 scope, conflict UI.

**69. One project at a time, or many?**
The window holds a single `projectPath` today, so comparing two repos means reopening.

- **A.** One project per window; restore the last one on launch. *(recommended)*
- **B.** Tabs for several projects in one window.
- **C.** A global session list across projects, with the tree in a side panel.

*Blocks:* how many SQLite pools the app owns, window state.

**70. Keyboard-first or mouse-first?**
Only `/` focuses the shelf search; there is no global command surface.

- **A.** A command palette (Ctrl/Cmd+K) for sessions, providers, settings and stop/start. *(recommended)*
- **B.** A handful of shortcuts, no palette.
- **C.** Mouse only for now.

*Blocks:* nothing structural; pure UI.

### B. Provider and model

**71. Is OmniRoute the only provider, or one of two?**
Settings now offers OmniRoute (default) and OpenRouter, each with its own keychain entry.

- **A.** Keep both; OmniRoute is the default, OpenRouter is the escape hatch. *(recommended)*
- **B.** OmniRoute only — remove OpenRouter and its key path.
- **C.** OpenRouter only; drop the local router.

*Blocks:* onboarding copy, key commands, docs, `RIGEL_PROVIDER`.

**72. When the router is down and cannot be started, what should a turn do?**
Today it stops with an error. Your OpenAI and Anthropic keys are in the keychain but no provider
uses them.

- **A.** Stop, and show a fix-it panel (start router, open Settings, check logs). *(recommended)*
- **B.** Fall back to OpenRouter automatically when its key exists.
- **C.** Fall back to OpenAI/Anthropic directly using the stored keys.

*Blocks:* error UX, and whether the bill can silently change model or price mid-session.

**73. How is the model chosen?**
The default is `gpt-5.6-sol-medium`; `--model` overrides per turn, and the router advertises hundreds
of models, including `auto/*` combos.

- **A.** A picker in Settings (searchable model list), saved as the session default. *(recommended)*
- **B.** Three presets — fast / coding / reasoning — that map to model ids.
- **C.** Keep one fixed default; power users pass `--model`.

*Blocks:* Settings UI, whether the model is a session or an agent property.

**74. Is reasoning effort part of the model id or its own setting?**
Today it is the `-medium` suffix (`gpt-5.6-sol-medium`), which is OmniRoute's convention.

- **A.** Keep it in the model id. *(recommended)*
- **B.** A separate `reasoning.effort` setting sent on each request; model id stays clean.
- **C.** Both: the setting wins when present, the suffix otherwise.

*Blocks:* provider adapter, model validation, which ids the picker shows.

**75. When should the router start?**
Today the app warms it on launch and retries on each turn; Settings can start it manually.

- **A.** Start with the app, keep the lazy retry. *(recommended)*
- **B.** Start only on the first turn, not at launch.
- **C.** Manual only — never start a process behind the user's back.

*Blocks:* launch time, tray behavior, and whether a closed window leaves OmniRoute running.

**76. How honest should the bill be when the router reports no cost?**
OmniRoute returns token usage but no `usage.cost`, so every OmniRoute turn is billed from Rigel's
local table and marked `estimated`. `gpt-5.6-sol-medium` is not in that table, so it uses the
conservative default.

- **A.** Keep `estimated`, and sync prices from the router's model metadata when it exposes them. *(recommended)*
- **B.** Keep estimates silently; the UI already says `estimated`.
- **C.** Treat OmniRoute as the source of truth and show no dollar figure when it omits cost.

*Blocks:* the budget guarantee's wording, pricing table upkeep.

### C. What the agent may do

**77. Does the native agent get a shell?**
D19 says no general bash loop; the tool set is read/list/edit/write/skills/budget/spawn_agents.

- **A.** Keep D19 — no shell. *(recommended)*
- **B.** Add `run_command` limited to an allowlist (git status, cargo test, rg), approval each time.
- **C.** Full shell with approvals.

*Blocks:* P4 parity, sandbox story, the exit tests that assume no process spawning.

**78. Are file writes auto-applied?**
Today an edit is journaled and applied atomically, revertible per patch; there is no per-write prompt.

- **A.** Keep auto-apply inside the project, with journal and revert. *(recommended)*
- **B.** Approve every patch before it touches disk.
- **C.** Approve only deletes, writes outside the project, or diffs above a size threshold.

*Blocks:* approval UI reuse in Phase 4, unattended mode.

**79. What happens when the model asks for something destructive?**
No destructive tool exists today, so this only matters once one is added.

- **A.** Refuse. *(recommended)*
- **B.** Approval, with the exact command shown.
- **C.** Allow when the path is inside the project.

*Blocks:* the tool policy table and its tests.

**80. Does the agent get network tools in v1?**
Only skill indexing calls the network today; `web_search` exists as an item kind but nothing emits it.

- **A.** No agent network tools in v1. *(recommended)*
- **B.** Web search and fetch with a per-domain allowlist.
- **C.** Full fetch with approval per origin.

*Blocks:* egress rules in §10, item rendering, prompt-injection story.

### D. Money and sessions

**81. What is the default budget, and over what window?**
Today: 500¢ ($5) per session, enforced within one turn; no daily or weekly cap.

- **A.** Keep $5 per session. *(recommended)*
- **B.** $5 per session plus a $20/day ceiling across sessions.
- **C.** Let the agent propose a per-task budget that the user confirms.

*Blocks:* settings schema, enforcement scope, unattended mode.

**82. Do subagents get their own ceiling?**
Up to four read-only children run per turn, each billed to the parent session; near the cap a fan-out
can overshoot by one step per running child.

- **A.** Pooled under the session cap, as today. *(recommended)*
- **B.** A per-child cap in addition to the session cap.
- **C.** No subagents by default; opt in per turn.

*Blocks:* budget service changes, roster UI.

**83. Do sessions ever close themselves?**
D1 says a session never cleanly ends. In practice the list only grows, and startup resumes the newest
non-ended one.

- **A.** Never auto-end; add an explicit Archive action. *(recommended)*
- **B.** Auto-archive after 7 days idle.
- **C.** Auto-end after 24 hours; resuming starts a new session.

*Blocks:* resume semantics, list size, what `active` means in the UI.

### E. Harnesses and release

**84. Which external harness first?**
Phase 4 lists Codex, Claude Code, Cursor and Cline. Each has a different approval protocol and a
different amount of protocol risk.

- **A.** Claude Code — stable `stream-json` plus an MCP permission tool. *(recommended)*
- **B.** Codex — app-server is experimental; `exec --json` is the fallback.
- **C.** Cursor via ACP.
- **D.** Cline via ACP (needs the SDK-only approval surface).

*Blocks:* Phase 4 order, fixtures, the first end-to-end approval test.

**85. Does Rigel's MCP server get wired into that harness in v1?**
`rigel-mcp` already exposes scoped tools over stdio, but no external harness connects to it yet.

- **A.** Yes — it is how approvals and skill tools reach a harness. *(recommended)*
- **B.** Ship harnesses without MCP; skills stay in Rigel's own UI.
- **C.** Defer MCP wiring until after the first harness works.

*Blocks:* P4.6/P4.10, the approval path.

**86. What is the 1.0 artifact?**
P7.4 still has no license decision (the workspace claims MIT OR Apache-2.0), and there is no updater
or telemetry today.

- **A.** Signed MSI, opt-in signed auto-update, MIT/Apache-2.0, zero telemetry. *(recommended)*
- **B.** Portable zip first; license and updater later.
- **C.** MSI plus opt-in crash reports to a hosted endpoint.

*Blocks:* P7, installer work, what a stranger can download this month.

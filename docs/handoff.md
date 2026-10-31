# Rigel — implementation handoff

**Audience:** a coding agent implementing this, with a human reviewing. Every claim about an external
CLI carries a source URL; every decision carries an ID that later sections cite. Where this document
and an older doc disagree, this document wins — see [§3.3 Superseded](#33-superseded).

**Last reviewed:** 22 Oct 2026 (the agent room ships with in-app, windowless agent launch, and
is merged into `main` — §2.8–§2.10).

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

One desktop window where several coding agents — Codex, Claude Code, Cursor, Cline, Gemini CLI, and Rigel's own
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
| **P4.1** Harness registry | `crates/rigel-adapters/src/registry.rs`, `crates/rigel-cli/src/harness.rs`, `ui/src/settings.tsx` | probes `codex`, `claude`, `agent`/`cursor-agent` and `cline` (PATHEXT resolution and `cmd /C` for npm shims); `adapters.json` re-probed when a binary's mtime or PATH location changes; missing rows carry an install hint; auth lines name the keychain provider. 21 Oct: Codex 0.162.1 and Claude 2.1.251 report `api key`; Cursor and Cline are `installed: false`. 8 unit tests, `rigel harness probe --json`, 4 cards checked in the browser preview — `27377b3`. **22 Oct:** Gemini CLI joins as a fifth row (`gemini --acp`, `GEMINI_API_KEY`, install hint `@google/gemini-cli`); `0005_gemini_harness` widens the `agents.harness` CHECK and the adapter unit test count becomes 9 |
| **P4.2** Per-agent API keys | `crates/rigel-core/src/credentials.rs`, `crates/rigel-adapters/src/codex.rs`, `ui/src/settings.tsx` | env-first test store; per-spawn redacted env injection; honest `ApiKey` / `Subscription` / `None` probe; pre-spawn refusal and first-event child kill in `tests/codex.rs`. Direct Codex resolves `rigel/openai`; only the local OmniRoute endpoint on port 20128 resolves `rigel/omniroute`, still injected as `OPENAI_API_KEY` because that is the name Codex accepts. Review fix: the provider now names that variable with `env_key` and leaves `requires_openai_auth` off, so Codex cannot fall back to the ChatGPT login — `45ac7e7`, `27377b3` |
| **P4.5** Codex adapter | `crates/rigel-adapters/src/codex.rs` | app-server primary, `exec --json` fallback that never re-runs a turn which already produced tokens, per-turn usage from `thread/tokenUsage/updated`; `tests/codex.rs` (4) against `tests/fixtures/fake_codex.rs`; real turn 21 Oct — 155,894 in / 5 out — `7b3c2fb` |
| `rigel codex` CLI | `crates/rigel-cli/src/harness.rs` | `probe --json` reports 0.162.1 and an absolute path; `run --json` returns text, transport, fallback flag and usage |
| **P4.6** Claude adapter | `crates/rigel-adapters/src/claude.rs`, `tests/claude.rs`, `tests/fixtures/fake_claude.rs` | NDJSON stream normalised into `Started` / `Delta` / `Message` / `Notice`; per-turn usage and `total_cost_usd`; argv, cwd and stream shapes pinned by 11 tests (prompt on argv, key never on argv, `--bare` present, subscription identity killed, key redacted out of every event, blocked turn surfaced, silent non-zero exit refused, a truncated stream without a `result` line rejected, an init line with no `apiKeySource` refused). Probe reports 2.1.251 against the real CLI. `rigel claude probe|run` is the turn loop (P4.6/P4.10): the MCP config is written per run, the approval is answered on stdin and the bill keeps the provider's own cost. The window sends one too: `turn_send` creates the row, returns it, then adopts that same row, streams `Delta`s and closes on `TurnFinished`, and the packaged binary hosts `rigel-app mcp` itself so `rigel_approve` reaches the card instead of a terminal — `crates/rigel-app/src/claude.rs`, `crates/rigel-adapters/src/turn.rs`, `crates/rigel-app/tests/mcp_mode.rs` (a real child process, the question answered on the bus) |
| **P4.3** ACP client | `crates/rigel-adapters/src/acp.rs`, `tests/acp.rs`, `tests/fixtures/fake_acp.rs` | JSON-RPC v1 client for `agent acp` / `cline --acp` / `gemini --acp`; initialize → session/new → session/prompt; `session/update` normalised into message, thought, tool-call, plan, usage, notice and `unmapped` events; `session/request_permission` routed to an `Approver` and answered only with an option the agent offered, otherwise `cancelled`; 9 tests against a scripted agent (approval outcome on the wire, unoffered allow downgraded, protocol 99 refused, hung turn cancelled and acknowledged, a pending approval answered `cancelled`, relative cwd refused, no key no spawn) |

### 2.8 Multi-agent chat — the P6 probe, verified 21–22 Oct 2026

The phase 5/6 question was "can several real CLI agents share one room, under Rigel's rules,
without a new protocol?" The answer is yes, and the working prototype is a tab in this app, not a
side experiment. The long-form record is [qa/agentchattr-integration.md](qa/agentchattr-integration.md);
the design draft is [multi-agent-channels-draft.md](multi-agent-channels-draft.md).

| Piece | Where | What it does |
|---|---|---|
| Chat tab | `ui/src/agentchat.tsx`, rail entry in `ui/src/App.tsx`, styles in `ui/src/shell.css` | roster pills, channels, stream, composer; the window boots the room at launch (`servers_start`), the tab can retry; never touches the server directly |
| Swarm button | `ui/src/session.tsx` composer → `swarm` in `ui/src/App.tsx` | hands the typed task to **every available room agent at once** as one `@mention` broadcast on `#general`, then opens Chat with the outcome; a room that is unauthed or has no online agents refuses with a visible reason instead of pretending to fan out |
| Core front | `crates/rigel-core/src/agent/agentchat.rs` | spawns `run.py` when the room is down (the OmniRoute contract), reads the session token from its stdout into core memory, exposes `status / channels / agents / messages / send` |
| Tauri commands | `crates/rigel-app/src/lib.rs` | `servers_start / chat_status / chat_start / chat_channels / chat_agents / chat_messages / chat_send / chat_start_agents`, registered in the handler |
| Hidden agent launch | `agentchat::start_agents` (core), `chat_start_agents` (Tauri), **Start agents** in the roster | spawns every configured harness (`config.toml` + `config.local.toml`) that is installed, with `CREATE_NO_WINDOW` — a real console with no window, so the wrapper's `WriteConsoleInput` injection still drives the TUI. Unattended flags match the auto-approve bats; API agents without `api_key_env` are skipped. Zero console windows verified 22 Oct |
| External agent rows | `rigel agent register` (`crates/rigel-cli/src/phase3.rs`, `phase3/agent.rs`) | mints an `agents` row for a harness so `rigel mcp --agent` is scoped and billed like a native agent |
| MCP server | `crates/rigel-mcp/src/lib.rs` | stdio JSON-RPC; six advertised tools (`rigel_skills_search`, `rigel_skill_load`, `rigel_patch_propose`, `rigel_budget`, `rigel_memory_read`, `rigel_memory_write`); claim tools stay unadvertised until P5 |

**Verified in the running desktop app (21 Oct, late):** `cargo run -p rigel-app` in dev mode
(no tauri-cli on this machine; vite on :5173 is the dev server) → opened the project from the
Shelf tab → Chat tab → **Start the chat server** → `running: true, authed: true` with the token
held only in Rust → mentioned an agent → it replied in-channel.

**Startup verified (22 Oct, sixth session):** with `RIGEL_AGENTCHAT_DIR` pointed at a throwaway
checkout and `RIGEL_AGENTCHAT_URL` at a free port, launching the built app made the fake `run.py`
answer on that port within ten seconds, with the Chat tab never opened — `servers_start` boots the
room from the window's first effect, and the same command warms the router when the provider needs
it. The check's process tree was stopped afterwards.

**Swarm verified (21 Oct, late):** with `claude`, `codex` and `phi4` online, one press sent the
task to all three; each acknowledged in `#general` and they split production code from
tests/verification without being told which to take. A follow-up broadcast stood them down with
no file edits.

**Rigel MCP from a mention — end to end:** Codex called `rigel_skills_search` and
`rigel_memory_write` during the mention; the artifact is on disk at
`.rigel/memory/<codex-agent-id>/topics/mention-check.md` and its chat reply quoted the real top
hit (`skillhub:mattpocock-skills-setup-ts-deep-modules`) and the memory sha256. Claude's server
and config are proven (raw stdio handshake lists all six tools; `claude -p --mcp-config …`
returned the correct hit), but the **interactive TUI session** still exposes only
`rigel_budget` — a Claude-Code-side gating quirk, recorded with evidence in the QA doc. Treat
per-harness MCP trust as the friction to automate, not a Rigel protocol problem: `rigel agent trust`
now writes the two trust files (22 Oct), and the QA doc records what that does and does not prove.

**Operational traps future agents will hit (all fixed here, none obvious):**

1. `vendor/agentchattr/config.toml` **`cwd` must be `"../.."`** (repo root). With `".."` the
   wrapper's project merge reads `vendor/.mcp.json`, so `--mcp-config` never contains the rigel
   servers, and Codex's "project" is `vendor/` too.
2. **Codex 0.162.1 does not read `.mcp.json`.** Its servers live in `~/.codex/config.toml`
   (`[mcp_servers.rigel-codex]`); the vendor README's `.mcp.json` instruction is stale.
3. **Claude Code gates project MCP servers** behind approval. `.claude/settings.local.json`
   (`enableAllProjectMcpServers`, gitignored) plus the `enabledMcpjsonServers` project entry in
   `~/.claude.json` make them visible to fresh sessions/`claude mcp list`; the running TUI may
   still lag. `rigel agent trust --project .` writes both files from `.mcp.json` — do not hand-edit
   them again.
4. **One rigel server per agent.** Keeping `rigel-claude`, `rigel-codex` and a user-scope `rigel`
   registered at once produces name collisions where only one tool survives in Claude's list.
   `.mcp.json` now carries exactly `rigel-claude`; Codex has its own entry.
5. **A restart can leave stale CLI sessions behind.** Each wrapper launch adds one; mentions
   then route into a dead or tool-less ghost (the symptom: "tools not exposed" or silence).
   Kill every `claude.exe` + `wrapper.py claude` before relaunching, and let the registry
   heartbeat expire so the canonical `@claude`/`@codex` names free up.
6. **`.mcp.json` is machine-specific and now gitignored** alongside `vendor/`. It carries this
   worktree's absolute paths and the session/agent ids minted by `rigel agent register`; a
   checkout that needs Claude's MCP wiring has to regenerate it rather than inherit it.
7. **The room's session token lives in Rigel's process.** If Rigel exits while the python server
   keeps running, the next instance reports `authed: false` and cannot send; close that server so
   Rigel can spawn and own a fresh one. Agents are children of Rigel, so a restart leaves them
   reconnecting to a dead origin until the new room is up and **Start agents** is pressed.

**Hidden agents, verified 22 Oct:** twelve console windows had accumulated from repeated
`windows/start_*.bat` launches — every wrapper dies with its console, so those were empty shells
still holding slots — and `taskkill /T` closed all of them. The Chat roster's **Start agents**
button now launches the configured harnesses itself: `agentchat::start_agents` reads
`config.toml` then `config.local.toml` (line-based, local wins), skips a CLI that is not on
`PATH` and an API agent whose `api_key_env` is unset, and spawns `wrapper.py` /
`wrapper_api.py` with `CREATE_NO_WINDOW`. That flag matters: the process still gets a console
with valid `GetStdHandle` handles — which `wrapper_windows.inject` needs for
`WriteConsoleInput` — while `GetConsoleWindow()` returns 0, so nothing appears on screen. A
standalone probe pinned that (`stdin_console=True stdout_console=True hwnd=0`), and the live run
confirmed it: `Get-Process` showed no console window for `python|claude|codex|cmd|conhost`, the
app owned three `wrapper*.py` children plus one `conhost.exe`, the roster read 3 online, and
`@codex` answered from the correct worktree folder in about 40 s.

Because a hidden console cannot answer a prompt, the launchers pass the same unattended flags the
auto-approve bats used (`--dangerously-skip-permissions`, `--dangerously-bypass-approvals-and-sandbox`),
and `harness_argv` is a pure function with a test so those flags stay visible and reviewable.
`/api/status` also carries room-level keys (`paused`) with no presence fields; the roster filters
on `typeof available === "boolean"` so they never render as blank pills.

### 2.9 The 22 Oct merge — the room plus the Phase 4 safety work

`main` had already landed the Phase 4 safety chain and a per-turn model picker written in another
thread; this branch carried the chat room, the Swarm button, the live-catalogue picker and the
windowless agent launchers. Both sides touched `ui/src/session.tsx`, `ui/src/api.ts`,
`crates/rigel-app/src/lib.rs`, `docs/handoff.md` and `README.md`, so the merge was reconciled by
hand rather than accepted wholesale.

| Area | Resolution |
|---|---|
| Model picker | The live catalogue plus a separate effort wins. `ui/src/models.ts` (a hardcoded per-provider list and `<select>`) is deleted; nothing imported it. The control is the chip in `ui/src/modelpicker.tsx`, and it writes through the same `settings_set` command as the Settings tab. |
| Default model | `configured_model(&settings, &provider)` still picks the provider's own default (Gemini keeps its default), and `wire_model` then applies the effort for OmniRoute ids that already encode one. |
| Effort | One setting, sent as `reasoning_effort`, never folded into the model id. Three call sites have nothing to inherit yet and now say so: the CLI `Coordinator` (`crates/rigel-cli/src/phase3/agent.rs`), the deliberation round (`crates/rigel-core/src/agent/deliberation.rs`) and Gemini's test request all pass `effort: None`. Threading `--effort` through the CLI is open (§2.10). |
| Transcript rows | The branch's command row and the merged `command` / `web_search` rows both stay: destructive advice, denied and timed-out states, and untrusted fetches still render. |
| Handoff / README | This file keeps the P6 section and D42–D69; the phase counts (11/19) and the Phase 4 capability table come from `main`. README gained the `skills sweep` command, the prototype dev line and a corrected Session-surface row. |
| Gemini key | `key_status` reports `gemini`, so Settings no longer claims a key is missing when one exists. |

Commits: `83e5db0` on the branch, `2ca2ead` the merge on `main`, pushed to `origin/main` the same
day. Verified by `cargo test --workspace` (102 core unit tests plus every adapter/CLI suite,
green), `npx tsc --noEmit`, `npm run build`, and a live run of the rebuilt app on the merged tree:
room booted, three agents online, zero console windows, Session showing the model/effort chip and
the Swarm button.

Two things the merge exposed that are worth keeping in mind:

- `ChatRequest` grew an `effort` field, so every literal had to state it. Adding a field to a
  request struct is a breaking change across files that do not otherwise move — expect it.
- One test asserted the *machine* had `vendor/agentchattr` next to the repo. That passed in the
  worktree and failed in the primary checkout, which is exactly the wrong kind of test. It now
  builds its own fixture in `std::env::temp_dir()` and asserts `dir_for` lands on it.

### 2.10 Where the work stands, and what is open (22 Oct 2026)

Verified working in the app: open a project → Chat → **Start the chat server** → **Start agents**
→ three agents (Claude, Codex, Phi4) online with no window on screen; mention one and it replies;
press **Swarm** on the Session tab and every online agent gets the task. Session has a live model
and effort picker; Settings mirrors it.

The seven gaps this section carried at midday are closed, each with its commit:

| Gap | What landed | Commit |
|---|---|---|
| P4.7 golden fixtures | 15 recorded streams under `crates/rigel-adapters/tests/fixtures/golden/`, replayed by `tests/replay.rs`; P4.7 is ticked in the Phase 4 list and the directory README records provenance and the scenario gaps | `20a1f60` |
| Agent lifecycle | `start_agents` keeps every child instead of forgetting it: `launched()` lists what Rigel owns, `stop_agent`/`stop_all` kill the wrapper's whole tree (`taskkill /T /F`) and drop the entry when it exits, the roster offers Start/Stop per agent, and closing the window stops what Rigel started | `4a17aea` |
| Effort plumbing | `--effort low\|medium\|high` on `rigel agent run`, validated against the same allowlist Settings uses, threaded into `Coordinator` and `ModelDeliberator`; `crates/rigel-core/tests/native.rs` asserts `reasoning_effort` reaches the wire on both paths | `ff67a92` |
| Swarm preflight | Swarm opens the roster with checkboxes and broadcasts only to the ticked online agents; the unauthed-room and empty-room refusals are unchanged | `5607810` |
| Checkout-less room | the Chat tab names the clone path, the dependency step and `RIGEL_AGENTCHAT_DIR` instead of failing at spawn; `setup_help` is covered by a test | `0fc6a26` |
| Claude TUI trust | `rigel agent trust` writes `.claude/settings.local.json` and this project's `~/.claude.json` entry from the project's `.mcp.json`, idempotently, with `--dry-run`; evidence in the QA doc | `0bafa04` |
| Spawn-path test | `launch_hidden` is the testable half of `spawn_harness`; a Windows test spawns a stub `wrapper.py` and asserts `GetConsoleWindow() == 0`, a writable `CONIN$` buffer and the unattended flag on the argv | `3d3022b` |

P4.8 landed after that list closed: `crates/rigel-adapters/src/normalise.rs` is now the single table
that answers "which of the ten kinds is this?" for Claude tool names, Codex item types in both
spellings and ACP's `kind` vocabulary. A Claude `tool_use` block and its `tool_result`, a Codex
`commandExecution` / `fileChange` / `mcpToolCall` / `webSearch` / plan / reasoning item on either
transport, and an ACP tool call all become items with the bodies the native agent already writes;
anything unmappable is an `error` item with `unmapped: true` and the wire event under `raw`. The
golden `tool_call` replays assert the shapes, so that row is no longer a recorded gap — `a12e7c9`.

The roster UI (P4.9, D40) landed next, in `a1388a7`: a subagent is indented under the agent whose turn
spawned it instead of being listed as a peer; a harness agent carries an `observed only` tag because
Rigel can show its turns but cannot send it work until P4.10; selecting an agent opens its own
transcript, child turns included, and selecting a second puts the two side by side with a hairline
between them — a third selection replaces the older pane rather than growing a grid. Claims were the
one D40 element with no backend then; the ledger landed in Phase 5, so the roster foot now points at
the Diff tab, where holder, stale rows and conflicts live. Per-agent claim counts in the roster are
still not drawn.

Three of those commits are Phase 4 checklist lines (P4.7, P4.8, P4.9): the checklist now reads
**13/19** (P4.11 dropped per D48), and the next one in order is P4.10 (end-to-end approvals across
the three protocols).

**Later, same day.** P4.10, P4.13–P4.18 are done and ticked (approvals on all three protocols with
scope buttons; the command, egress, classifier, deliberation and snapshot lines audited against
their tests; the destructive path proved end to end in `command.rs`). **Phase 4 is now closed:**
P4.4 drives the installed Cline through `cline --acp` (verified live through `initialize` and
`session/new`), P4.12's matrix is `scripts/p4-exit-test.sh` (three harnesses, one killed, resume
closes only that turn), and P4.19 records Codex input tokens on the turn, trims the skills catalogue
to 1 000 tokens by default and prints the estimate before the request. **Phase 5 is closed too:**
`scripts/p5-exit-test.sh` covers the ledger, attribution and word test; P5.5's worktrees and
P5.6's per-hunk gutter are implemented and tested. Real vendor turns for Cursor and Cline remain
unavailable on this machine (subscription-only and no key); every protocol path is exercised
through the fixture binaries instead, and the exit tests say so.

**CI is green again** (`5de38c9`): `cargo fmt --all -- --check` and `cargo clippy --workspace
--all-targets -- -D warnings` both pass. The lint that was outstanding was `turn_send`'s eight
arguments — Tauri's `invoke` matches by name, so the signature stays flat and carries an allow that
says so.

**22 Oct: the Phase 4 exit test was red and is now green.** Step 6 required every harness turn to
carry a `command` item containing `rigel doctor --json`; Cursor and Cline failed it. The cause was
in the fixture, not the client: `fake_acp` asked `session/request_permission` without first sending
the `session/update` `tool_call` that a real ACP agent sends, so the turn had no tool call to
record. The fixture now announces the call (title, `kind: execute`, `status: pending`, and the
command under `rawInput`), `tests/acp.rs` asserts the decoded `AcpEvent::ToolCall`, and the exit
test asks whether *any* command body carries the text instead of only the lexicographically
largest one. Step 4 also raced: a fixed three-second sleep could kill the hung CLI before its turn
row existed, leaving `resume` with nothing to close, so the script now waits until the project DB
shows that session's running turn. The window's command line reads a harness body (`command`, or
`raw_input.command`) and treats `complete`/`completed` as success, so a Codex or ACP command no
longer renders as `$ ?` with a failure style. `scripts/p4-exit-test.sh` (run twice) and
`scripts/p5-exit-test.sh` pass; `cargo test --workspace`, `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings` and `npm --prefix ui run build` are green.

What is still open, in the order it should be taken:

1. ~~**Claude runs from the CLI but not from the window.**~~ **Closed 22 Oct.** `turn_send` now
   adopts the row it returned to the renderer, runs the adapter on its own thread, streams `Delta`s
   and closes on `TurnFinished`; the packaged binary hosts the stdio server itself
   (`rigel-app mcp …`), so `rigel_approve` reaches the window's card rather than a terminal. The
   recorder moved to `rigel-adapters` so the CLI and the window bill a turn the same way, and the
   roster's `observed only` label now follows whether the window can actually send to that harness
   (`native`, `claude`) — `crates/rigel-app/src/claude.rs`, `crates/rigel-adapters/src/turn.rs`,
   `crates/rigel-app/tests/mcp_mode.rs`. **What that test does not prove:** nobody has watched a
   live Claude turn arrive in the running window, which needs an Anthropic key and a person.
2. **One human look at Claude's TUI.** The trust files are now written by a command, but the six
   tools inside a *running* interactive session have not been re-observed since; Claude reads trust
   at startup, so this needs a restart and a person. It is the last open line in the QA doc.
3. **This checkout has no `vendor/agentchattr`**, so the live room check needs a machine that has the
   clone (or `RIGEL_AGENTCHAT_DIR`). The Chat tab now explains exactly that rather than failing at
   spawn time.
4. **Phase 7's exit test needs a clean VM and a certificate** (P7.5, P7.9). The signing pipeline and
   both installers exist; what is left is environmental: a certificate whose chain Windows trusts, a
   Windows 10 1809 image, and a person for SmartScreen. **Run on 22 Oct, elevated:** the install path
   is no longer unexecuted — `msiexec /i … /qn` exit 0, the payload at
   `C:\Program Files\Rigel\rigel-app.exe`, the window alive after 20 s, one native turn and a shelf
   search behind a dead proxy, `msiexec /x` exit 0 and the machine clean afterwards. The transcript
   and the two fail-by-design rows are in `docs/qa/p7-acceptance.md`. Windows Sandbox is now enabled
   (the restart after enabling it did not take; see the fifth-session note) and `scripts/p7-sandbox.ps1` runs the same acceptance inside it with the
   checkout mapped read-only, which is the clean machine this host can provide; the 1809 floor and
   SmartScreen still need an image, and `ok trusted` still needs a certificate — both certificate
   stores were checked and are empty.

**Review note (22 Oct 2026, third session):** the window can send a Claude turn (see item 1 above),
so the roster's `observed only` label now follows sendability rather than "not native". P7.5 moved
from "config only" to real artifacts: `cargo tauri build` produces the MSI (8.0 MiB) and the NSIS
setup (5.4 MiB) from `crates/rigel-app`, and `scripts/sign-installers.ps1` signs and verifies them,
exercised end to end with a throwaway self-signed certificate (unsigned → exit 1, signed → note
about the untrusted chain, no certificate → exit 2). The second signing pass (sixth session) found
that signing the app executable before `cargo tauri build` is not enough: Tauri patches the binary
while packaging, so the embedded copy's signature is stale. The script now lets Tauri sign the
patched payload through a temporary config and verifies it out of the MSI. Two things were learned the hard way and are
now pinned: Tauri runs `beforeBuildCommand` one level above the app directory, so building from the
repository root resolved `../ui` to the wrong place — build from `crates/rigel-app`, as
`docs/PLATFORMS.md` now says and `scripts/p7-exit-test.sh` asserts. P7.9's machine-checkable half is
`scripts/p7-vm-check.ps1`: the build floor, Node's absence, the MSI signature, a silent install, the
WebView2 bootstrapper, the window starting, `doctor`, one native turn offline, the shelf and an
uninstall, with only SmartScreen and the look of the window left to a person.

Gates after these changes, all green: `cargo test --workspace`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo fmt --all -- --check`, `npm --prefix ui run build`,
`node scripts/handoff-status.mjs`, `scripts/p3..p7-exit-test.sh`, and `cargo tauri build`
(MSI + NSIS). The MSI's payload was extracted with `msiexec /a` and contains `rigel-app.exe`
(23 MB) — which is why `scripts/p7-vm-check.ps1` looks for any `*rigel*.exe` rather than `Rigel.exe`.

**Review note (22 Oct 2026, fourth session):** the open items were narrowed to what is actually
environmental. An elevated prompt made the install path runnable, so `scripts/p7-vm-check.ps1` ran
with the MSI for the first time and passed every step a developer machine can pass — see
`docs/qa/p7-acceptance.md`, which now separates what the host run proves, what a sandbox proves and
what still needs an image. `Containers-DisposableClientVM` was enabled on this host (Hyper-V was
already on), which needs one restart before `WindowsSandbox.exe` exists; `scripts/p7-sandbox.ps1`
writes a `.wsb` with absolute paths at launch and `scripts/sandbox/p7-in-sandbox.ps1` runs the
acceptance inside, so no machine-specific path is committed. The two rows that fail here fail by
design: Node is installed, and the MSI is unsigned because neither certificate store holds a
code-signing certificate after the throwaway one was removed. P7.5 and P7.9 stay open, with the
reason recorded rather than a tick.

**Review note (22 Oct 2026, fifth session):** the restart before this session did not happen.
`LastBootUpTime` was still 17 Oct (five days of uptime), the most recent `Kernel-Boot` event 27 says
boot type `0x1` (hiberboot) on 19 Oct, and CBS still reports `RebootPending`. `HiberbootEnabled=1`,
so "shut down and start again" resumes a hibernated session and applies no servicing; only Restart
does. The feature therefore reads `Enabled` while nothing is staged: no `WindowsSandbox.exe`, no
provisioned or Store package (`winget search` finds none), so there is no no-reboot route and the
run is one restart away. To make that restart enough on its own, `scripts/p7-sandbox.ps1 -DryRun` now
prints the `.wsb` before the binary check (verified: two mounts, command line intact), and the
one-shot `schtasks` task "Rigel P7.9 sandbox acceptance" runs the launcher 45 s after this user's
next sign-in and deletes itself; exercised, it returns 2 plus `docs/qa/records/p7-sandbox-launch.log`
while the sandbox is absent, and 0 once it launches. `docs/qa/p7-acceptance.md` holds the evidence
and the command to arm it again after a failed boot.

**Review note (22 Oct 2026, sixth session):** Gemini CLI is now a fifth external harness, driven
through the ACP client exactly like Cursor and Cline: probe `gemini`, invocation `gemini --acp` (the
flag replaced `--experimental-acp` in 0.33), key `GEMINI_API_KEY` — the same keychain entry the
native `gemini` provider uses — and the install hint `@google/gemini-cli`. The schema had to move
with it: `agents.harness` carried a five-value CHECK and SQLite cannot widen one in place, so
`0005_gemini_harness` rebuilds the table, and `migrate::apply` now runs migrations on a connection
with foreign keys disabled (the pragma is a no-op inside a transaction) and refuses the commit when
`foreign_key_check` finds anything. Without that, the rebuild's `DROP TABLE agents` would cascade
through `turns` and `claims` and empty the transcript. A schema test seeds a v4 database, migrates
it, and asserts the rows survived and an unknown harness is still refused. The Phase 4 matrix now
runs a Gemini turn and prints five harness rows. Gemini's vendor-login file is read only to avoid
calling an OAuth session an API key; its current builds migrate that file into a hybrid store, so
an absent file is not proof of being signed out.

**Review note (27 Oct 2026):** the chat layer stopped being three hand-written matches. **Vertex AI**
joins as an adapter — project and location in the URL (a non-global location gets its regional host),
an OAuth access token resolved from `VERTEX_ACCESS_TOKEN`/`rigel/vertex` or from
`gcloud auth print-access-token` when nothing is stored, and ids in the publisher-prefixed
`google/gemini-…` namespace — and seven more OpenAI-compatible vendors arrive as rows: OpenAI, xAI,
Groq, DeepSeek, Mistral, Together, Fireworks. Two rules now sit between Settings and the wire:
`chat_model_id()` strips a trailing `:batch` before the request body is built, so the refiner's 404
cannot recur, and `providers::is_model` keeps an id that another door owns (AI Studio's
`gemini-4-argon` on Vertex, or vice versa) from being sent at all. The window renders the table
through `providers_list`, so a new adapter no longer means editing the renderer, the key status or
the turn path. Base URLs were taken from each vendor's OpenAI-compatibility page; only OmniRoute,
OpenRouter and Gemini hold keys on this machine, so the other rows are covered by unit tests against
scripted servers rather than a live turn, and Fireworks is the one row whose `/models` listing was
not confirmed on its OpenAI-compatible base.

**Review note (27 Oct 2026, later the same day): Claude arrives as an adapter of its own.** The
Messages API is not `/chat/completions`, so `crates/rigel-core/src/agent/anthropic.rs` carries its own
request encoder and event decoder: bearer auth, the system prompt hoisted to a top-level `system`
field, a **required** `max_tokens` (the one request field with no default, so one is supplied), tool
results as `tool_result` blocks on the `user` turn that answers a `tool_use` block, consecutive
turns of one role merged into one, and the named `message_*` / `content_block_*` events assembled
through the same `ToolCallBuilder` and `Delta`s every other adapter emits. Usage arrives split across
two events; because a consumer **replaces** its usage row rather than merging, the full figure is
re-sent on `message_delta`. Two fields are deliberately absent, both because the current API version
says so: `temperature`, which version `2026-10-21` removed (sending it is a 400 on every model), and
a `thinking` object — manual `budget_tokens` thinking is deprecated on the 4.6 generation and
rejected by 4.7 and later, so the request's `effort` becomes `output_config.effort` instead. The row
carries `anthropic-version` as a table field, so the picker's roster call sends it too; its namespace
is `claude-…`, so another door's id is replaced by the default rather than sent; and
`supports_subagents` now excludes it, because the cheap allowlist is OpenRouter-shaped and every id
would have been swapped for the default — an expensive one. Tests cover the encoder, both
tool-argument assembly paths, the 429 retry, the error event and the two-absent-fields rule.
**The live attempt reached the API and was refused**: this machine has a stored `rigel/anthropic`
entry, and a real two-step turn was answered `401 invalid x-api-key` — twice, once with each
credential form, which says that entry is not a key this account can use, not which header carried
it. The request itself is therefore verified up to the provider's own error page (path, version
header, body shape all accepted enough to be authenticated against), and everything after the
credential is verified against a scripted server: **the first live Claude turn still needs a working
key.**

**Review note (27 Oct 2026, round 2 of the day): the picker's roster, the shelf's freshness and the
window's settings.** Four things landed together because they are the same idea — a screen that
says what it knows and writes something real.

1. **The model roster moved into the core and stopped being one provider's list.**
   `crates/rigel-core/src/agent/catalog.rs` holds `catalog()` (one door), `roster()` (the merge) and
   the HTTP half as `read_roster()`, so the window's `models_list` and the new `rigel models`
   command cannot disagree. The rules: the current provider leads, OpenRouter is offered with no key
   because its list is public, a provider that publishes no roster contributes its own default row,
   a failure is reported per provider instead of emptying the picker, a row is deduplicated **per
   provider** (the same id on two doors is two rows), and the read is capped at six rosters. Live on
   this machine the merge returns **1 220 rows** across four doors (627 router, 464 OpenRouter, 128
   OpenAI, 1 Gemini default) — which is why the picker gained provider chips and search rather than
   silently showing the first N. Tests: a scripted server for the parse/sort/dedupe and the bearer
   header, the pure merge rule, and the no-roster default.
2. **The skill shelf refreshes itself.** `SkillIndex::refresh()` re-syncs every source that is not
   fresh — the same pass `rigel skills refresh` runs — and the window's launch path calls it when
   the `skills_auto_update` setting is on (it defaults to on), spawning it so a slow hub never
   delays a launch. Each source's outcome is recorded as a row (`checked` / `refreshed` / one line
   per source), the shelf's Settings page shows the state, the last sync and the failure, and a
   source that was `broken` stays `broken` with its reason rather than being retried into
   silence. Verified live against the three real sources on this machine: 3 stale → 3 refreshed.
   The `terminal_shell`, `terminal_font_size`, `terminal_scrollback` and `max_steps` settings were
   added in the same pass — `save_settings` clamps each to the range its consumer accepts.
3. **The Terminal surface was rebuilt.** The old page was a single bare host: an unframed canvas
   that ran the full width, no status, and a preview that claimed there was no PTY. It is now a
   framed surface with a status pane (shell, cwd, state, font · lines), a hint that says which of
   the two things it is, an error line when a shell refuses to start, and a preview line editor with
   a banner — so the browser build shows the surface honestly instead of looking broken. Font size
   and scrollback are read from settings and applied to a live terminal; the shell program is read
   when a tab opens and is clamped to the OS default when blank.
4. **Settings became eleven pages.** One scrolling column had grown to hold providers, harnesses,
   spend, safety, rules, notifications and updates; it is now a nav of pages — Agent, Providers,
   Harnesses, Safety, Spending, Terminal, Skill shelf, Project rules, Notifications, Updates,
   Diagnostics — with per-page loading (the shelf's sources are read when that page opens, not at
   launch), a per-row fact and state dot in the nav, and one inspector per page. The rule the
   screen follows: **every control writes something the core reads.** That is why `theme`,
   `default_budget_cents` and `unattended_default` are deliberately *not* on it — they are in the
   settings file and nothing consumes them yet, and a switch that changes nothing teaches the user
   not to trust the screen.
5. **Forty questions that this state invites (Q136–Q175)** are in
   [grilling-round-6.md](grilling-round-6.md). The five that matter most are named at its end: the
   picker's chat-only filter, the local-model row, a rollback path for the updater, a live Claude
   key, and the end-to-end update test.

**Twenty more questions this state invites (Q176–Q195)** are in
[grilling-round-7.md](grilling-round-7.md). **Answered 30 Oct 2026; decisions D127–D146.** The round
asked what the window can now do that it still cannot — images into a vision model, file completion,
per-session drafts, forking, project defaults, importing another harness, a declared race, pause,
handoff, per-agent models, a transcript filter, the keyboard layer, focus mode, notification
routing, shelf pinning plus a cheap skill scout, pin diffs, source pause/forget, picker favourites,
model metadata, and more providers ordered by cost and latency. Two answers redirected the
question: file completion is covered by attaching a project file in place (D146), and the provider
table grows rather than staying closed (D145).

**Twenty questions about using the window (Q196–Q215)** are in
[grilling-round-8.md](grilling-round-8.md): one search box and a palette, session management, a
transcript outline, bookmarks, prompt history and snippets, session templates, a cost preflight, an
attention inbox, a return digest, line provenance, a bill explorer, accessibility, UI scale and
density, a taskbar badge, a step-through replay, a two-model comparison, a blast-radius preview,
skill distillation, a failure heatmap and shared sessions. The last six are deliberately hard and
name a narrow first version each. **Answered 30 Oct 2026; decisions D147–D166.** Nineteen follow the
round's recommendation; Q215 declined it, so shared control is recorded with the identity work and
the two-approval rule that must precede it.

**Round 3 of the grilling (Q87–Q101) was answered on the evening of 22 Oct; decisions D71–D85.** It is the
first round that is about shipping rather than building, and it changes the open list more than any
session since the merge. In the order the answers force:

1. **The release sequence is settled (D86):** an unsigned private beta first, the certificate ordered in
   parallel, and no public tag, release page or launch post until it exists. D84's launch is the gate the
   certificate holds.
2. **The room becomes installable and remote-capable.** D74 freezes `run.py` into a sidecar; D76 copies
   the transcript into the project DB; D75 adds the auth-path badge; D87 puts a LAN bind with per-agent
   auth in 0.1.0, which supersedes D70 for the room. This is the largest block of new work and the one
   that makes the flagship true on a clean machine and on a second one.
3. **Two safety-shaped items before a public binary:** D78's sliding-window redactor, and D82's
   diagnostics export plus crash log. Both are bounded and testable with the existing fixtures.
4. **Two deviations stop being deviations:** D79 builds the real three-way merge and D80 makes spawned
   subagents writable. D80 is the riskier of the two — writable subagents move claims, snapshots,
   budgets, attribution and merge review (D53, D64) onto a path that has never carried them, and
   `agent::spawn` is read-only by construction today.
5. **The harness claim narrows.** D81 keeps Cursor's and Cline's code and fixtures but takes them out of
   README, PLATFORMS and the roster's supported set; the prose must stop saying five.
6. **D83 rewrites a promise, not a test:** a startup update check means the settings copy and SECURITY.md
   say a request happens at launch, and `update.rs`'s "nothing checks at startup" assertion moves to "one
   check, then never again this launch".

**Round 4 (Q104–Q123, decisions D88–D107) turned the feature question into a build queue.** Thirteen
features are in 0.1.0, seven are explicitly after it, and one overrode a recommendation. The queue, in
an order that respects dependencies:

| # | Work | Size | Note |
|---|---|---|---|
| 1 | Rules interop (D91) | S | read-only first; export is a Settings action |
| 2 | Desktop notifications (D90) | XS | three events, no sound |
| 3 | Session export (D95) | XS | shares the diagnostics assembly |
| 4 | Skills over MCP (D96) | XS | two tools, local index only |
| 5 | Spend view + per-task cap (D92) | S | query over existing usage rows |
| 6 | Per-turn checkpoints (D89) | S | `snapshot.rs` already restores |
| 7 | Hunk comments (D88) | S | needs a delivery turn to the author agent |
| 8 | Role presets (D94) | S | saved prompt + model/effort + tools |
| 9 | skills.sh import (D93) | S | audit verdict stored as dated evidence |
| 10 | Plan mode (D98) | M | a new first phase in the turn loop |
| 11 | PR via `gh` (D97) | M | explicit click only; no token |
| 12 | Container sandbox (D107) | L | **conflicts with D56 and PLATFORMS**: optional backend only, Docker/WSL2 detected, app must pass P7.9 without it |

The seven deferrals (D99–D106) are the task board, MCP catalogue, skills lockfile, mobile read-only
view, remote approvals, issue import, hooks and the merge queue. They wait on D80 or on a second
machine, and they are recorded so they are not re-proposed as new.

**Round 5 (Q124–Q135, decisions D108–D119) settled signing, three safety defaults and the launch path.**
Everything except the certificate is consistent; D109 blocks the rest.

- **The release path is:** ten hand-onboarded users → Show HN → Console.dev and the Rust/Tauri
  subreddits (D112); direct signed MSI/EXE with winget and a Scoop bucket (D110/D111); Store after 1.0.
- **The signing route is settled (D120):** SignPath Foundation first (free for qualifying OSS), Azure Artifact Signing at $9.99/month as the fallback, both started in parallel. D86's gate stands — nothing public ships unsigned, and the private beta does not wait for it.
- **The safety defaults are code, not docs:** three writable agents (D113), plan approval before the first
  edit (D114), a blocking clean-context reviewer on writable merges (D115), and a $20/day global cap
  (D116). These land with D80's writable subagents — a writable path without them would be the exact
  failure the evidence documents.
- **Imported text is data, not instruction** (D117): rule sources are named in the transcript, skill
  bodies are shown before first use, and scripts still ask.
- **Verification gets its own harness** (D118), and support stays asynchronous (D119).

**Round-7 safety block (23 Oct 2026): D80 with D113–D116.** Writable subagents shipped with
the four safety defaults that make the path defensible, not after them:

- **D80** — a writable child is registered with a claim on each assigned file, gets a detached
  worktree at `HEAD`, and runs there with the normal tool set limited to that assignment (an
  assigned file that does not exist yet is allowed, so `write_file` is usable). The parent sees the
  worktree path and the changed files, not the child's transcript. Its work lands only through
  `merge_subagent`, which reads the diff from the worktree, reviews it, records the verdict, and
  merges file by file through the CAS journal, attributed to the child. A clean merge removes the
  worktree; a conflict leaves it for inspection.
- **D113** — three writable children at once per project, counted from the durable `agents.writable`
  flag while they run; read-only children are unlimited. The number is a row in `safety_settings`
  and Settings → Safety edits it (1–16).
- **D114** — a writable fan-out requires a plan per child and one user answer before registration:
  approve, skip with a recorded reason, or deny. The answer is `ApprovalBus`'s plan question (the
  same card, its own verbs); approval is stored as `agents.plan_json`, a waiver as `agents.plan_note`,
  and a denial starts nothing at all.
- **D115** — the merge reviewer is a separate model call with clean context: system instructions
  plus the unified diff, no transcript, no tools, `REVIEW_MODEL` (the cheapest allowlist entry).
  It answers APPROVE or BLOCK; anything else is `unavailable` and blocks. An override needs a typed
  reason, which is stored beside the verdict in `merge_reviews` and reported in the tool result.
- **D116** — `daily_spend` is incremented in the same transaction as the session total, and the
  $20/day cap is on by default across every session in the project. `reserve` refuses the turn
  before it starts; `account` returns `DailyExceeded` and interrupts every running agent after the
  step that crosses it. The 80% warning fires once per day; Settings → Safety shows the day and
  raises or clears the cap.

**The writable path is visible where the patches are (23 Oct 2026).** The Diff view gains a
`Subagent worktrees` block between Claims and Patches. Each writable child is listed with the plan
the user approved (or the recorded waiver), the stored verdict — `approved`, `blocked` or
`unavailable` — the reviewer's reason, the hash of the diff it judged, and the override reason when
there is one. A block that merged anyway is drawn in the danger colour and labelled `merged over a
block`: it is the one row that must not read as routine. The pending diff is read on demand through
`writable_diff`, which returns the worktree's unified diff and writes nothing; the row list rides
the existing two-second coordination poll because it carries no file bodies. Once a merge removes
the checkout the row says so instead of showing a path that no longer exists. Reading the list,
the review and the diff is covered by the writable suite, including a blocked merge with its
override and a pending diff read that leaves the main tree untouched.

**The Diff view can decide a review now (23 Oct 2026).** A stored review that has not merged
carries its decision inline. An approved one — the reviewer said yes but a conflict interrupted
the merge — has a single `merge this review` button that needs no reason. A blocked or
unavailable one needs a typed reason before `override & merge` enables. Neither button calls a
model: the verdict stands as written, the reason is written to `merge_reviews.override_note`, and
`apply_review` only proceeds while the worktree's current diff still hashes to the review's
`diff_sha`. A checkout that moved after the review is refused with that sentence rather than
merged under a verdict about other bytes. The files still go through the CAS journal attributed
to the child, so a stale main tree is refused by the pipeline; a partial merge reports
applied/conflicted/skipped, leaves the review open, and the row keeps its checkout. Two tests
cover both paths: an approved review applied with no agent turn (attribution intact, review marked
merged, checkout removed), and a blocked one refused without a reason, refused once the worktree
changed, then applied with the reason stored beside the unchanged `blocked` verdict.

**A merge in the window is written into the transcript (23 Oct 2026).** `apply_review` now posts
a `note` item on the parent turn that spawned the child — the turn the child's own rows already
hang under — so the user's decision exists in the transcript and not only in `merge_reviews`.
The note names the child (role and the first eight characters of its id), what landed (`1 file
landed in the main tree`), or the applied/conflicted/skipped counts with the checkout still in
place; when a block was crossed it also carries the verdict and the user's reason. Refusals, a
stale review and a second apply post nothing. A turn's prompt is built from scratch, so a
transcript row alone would never reach the model: `SessionService::previous_notes` reads the notes
left on the agent's previous turn and `drive` restates them in the preamble (“The user acted in
the window…”), which is the one path by which a window action enters a model call. The UI paints
a note as an italic line in the user's voice (`you · …`), not as model output. The writable suite
asserts one note per merge, with the override reason carried through; the native suite asserts
the note is restated exactly once, in the turn after the one it was posted on.

**Round-6 safety pair (23 Oct 2026):** D78 and D82 are code, not notes. `StreamRedactor` in
`crates/rigel-adapters/src/lib.rs` holds back the longest suffix that could still become a secret
(at most a full secret's length, not length − 1: a chunk ending exactly on a secret must not leak
its first byte), and Claude, Codex and ACP deltas all pass through it; tests cover a secret split
across three chunks, a multibyte boundary and the flush at end of turn. D82 adds
`rigel_core::diagnostics`: an opt-in Markdown bundle with the database integrity check, schema
versions, the newest turn, key-presence only, and a redacted log tail, written under
`<app data>/diagnostics/` and copied to the clipboard; a panic hook appends a redacted record to
`<app data>/crash.log`. Settings gains the action, and nothing is sent anywhere. The two remaining
open items are unchanged: P7.5 needs a trusted certificate and P7.9 needs a clean boot.

**Round-7 features, first three (23 Oct 2026):** D95 exports one Markdown bundle and its JSON
under `<app data>/exports/`, built from the same redaction path as diagnostics and linked from the
Session header. D89 now records a snapshot after every completed turn (best effort, under
`<repo>/.rigel/snapshots/` and gitignored), lists checkpoints per turn and restores one only on an
explicit click. D90 adds three per-event notification switches to Settings; notifications are
silent, never fire for a focused window, and use the WebView Notification API after a user-granted
permission. Session export, checkpoints and notifications each have their own tests or build
coverage; no external service is involved.

**Three more queue items (23 Oct 2026):** D91 adds `rigel_core::rules`, which reads
`AGENTS.md`, `CLAUDE.md` and `.cursor/rules/*` with per-file and total caps, labels them as
imported data in the native preamble, lists the sources in Settings and copies committed memory
as an `AGENTS.md` block. D96 exposes `rigel_skills_show` over MCP: source, trust evidence, session
pin, index body hash and install path, all from the local index with no network. D97 composes a
pull-request title and body from the session's turns and runs `gh pr create` only from an explicit
Diff-tab click; `gh` supplies its own auth and Rigel never holds a forge token.

**Environment facts a fresh agent will trip over:** the debug app loads `http://localhost:5173`, so
`npm --prefix ui run dev` must be running before `cargo run -p rigel-app`; `vendor/agentchattr`
and `.mcp.json` are local-only; never launch agents with `vendor/agentchattr/windows/*.bat`
(windowed consoles, orphaned on restart); and `--json` runs default to deny, so an approved
command must say so explicitly.

---

## 3. Decisions of record

### 3.1 Product

| ID | Decision | Source |
|---|---|---|
| D1 | The unit is a **project-level session**; the UI shows windows onto a log. **Amended by D35:** a session idle for 24 h auto-ends, and resuming starts a new session. | Q17, Q83 |
| D2 | Memory is **per agent and portable**; it lives in the repo as text. **Amended by D43:** the artifact is one central file with per-agent sections, not one file per agent. | Q18, Q47 |
| D3 | The index is **local-only** and machine-specific. | Q56 |
| D4 | Uploaded files are **context only**, never project content. | Q19 |
| D5 | File claims are **advisory**: an MCP tool agents may call, plus violation detection. No locking. | Q14→Q31→Q51 |
| D6 | Only agents Rigel **spawns** are forced to register. Third-party subagents are observed best-effort. | Q53 + research |
| D7 | Spawned agents run in **isolated worktrees**; the main session runs in the live tree. | Q52 |
| D8 | Writes Rigel performs are **atomic and journalled**; direct harness writes are not. | Q33 |
| D9 | Skills are invoked as **tools**, and skill versions are **pinned per session**. | Q38, Q39 |
| D10 | Budget caps are mandatory and stop agents automatically. **Amended by D42:** default-on with an explicit, confirmed no-cap opt-out; unattended sessions still require a cap. | Q25 |
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
| D42 | The budget cap is a **first-class per-session setting** (default $5; subagents pool under it per D34), with an explicit **"no cap" value behind a one-time confirm**. Unattended mode still refuses sessions with no cap. Enforcement within a turn is unchanged; D10 keeps its default. | grilling round 3, 21 Oct |
| D43 | Memory is **one central `MEMORY.md` at the project root with per-agent sections**, append-mostly, **unbounded on disk** — no size cap, watermarks or rotation. The implementation keeps the redaction validator (machine paths and secrets out) and the index rebuild; only the file split changes. | grilling round 3, 21 Oct |
| D44 | The **whole file enters every turn's context** — no tail window, no FTS retrieval, no truncation. The file can grow forever on disk, but the token bill is the pre-turn estimate's job to show. If it gets big, the user decides what happens. | grilling round 3, 21 Oct |
| D45 | Each agent's system prompt carries the **summary header of the central file with a pointer to it shown every turn**; the agents read the file on demand via a memory-read tool. No full-file per-turn injection for harness agents. | grilling round 3, 21 Oct |
| D46 | Spawned Codex runs against a **Rigel-pinned temp skills root**: the session's pinned skills only, never the machine-wide set (D9). Pinning happens at session start; no mid-session mutation of what Codex sees. | grilling round 3, 21 Oct |
| D47 | The interventions-per-completed-task metric (D17) renders **always in the status bar** alongside spend (N3 figures). Not roster-only, not end-of-session-only. | grilling round 3, 21 Oct |
| D48 | The Cline Node sidecar (S4, P4.11) is **dropped**: ACP is the only path to Cline's approval surface; a vendored SDK is not maintained for marginal capability; Cline SDK-only features are unavailable and the roster says so. | grilling round 3, 21 Oct |
| D49 | Code signing is **deferred**. 1.0 remains the signed-MSI line (D37 unchanged); no certificate is bought before external users exist. Interim distribution: local build + documented run steps. | grilling round 3, 21 Oct |
| D50 | **The centrepiece is confirmed**: several agents on one project, branched into isolated worktrees (D7), merged into one attributed diff, with three-way conflict UI and advisory claims — not a one-agent shell with multi-agent plumbing underneath. Phase 5 is the spine, not the tail of Phase 4. | user, "3 agents branching out like i wanted", 21 Oct |
| D51 | The **next code session** after this one opens with **P4.7 golden fixtures** (fresh Cursor and Cline probes now exist), before any new tool, UI or safety-chain work. | grilling round 3, 21 Oct |
| D52 | **Teammate awareness.** Every agent's system prompt opens with a roster block — teammate names, harnesses, roles, live status, and current claims (Q26). On top of that, **full transcript broadcast**: each agent's transcript streams into every other agent's context while working (Q27). The pre-turn estimate must show the input-token cost this implies (D29, P4.19). **Amended by D68:** the broadcast clause is replaced — agents get running summaries of teammates and can read a teammate's full transcript on demand. | grilling round 4, 21 Oct |
| D53 | **Claim gate on Rigel-performed writes.** A write Rigel itself performs (patch pipeline) refuses a path a live teammate has claimed unless the acting agent acknowledges the claim first. Direct harness writes stay observed-only — D5's advisory boundary settled in collision 1. The gate never extends to writes Rigel does not perform. | grilling round 4, 21 Oct |
| D54 | **Coordination shape: coordinator delegates.** The native agent splits work, spawns harness agents and reads their results; peers coordinate via awareness and claims. Matches the existing subagent model; peer-orchestration is a later setting, not v1. | grilling round 4, 21 Oct |
| D55 | Spawning agents work in worktrees at `%APPDATA%\Rigel\worktrees\` (P5.5 unchanged); the main session stays in the live tree. | grilling round 4, 21 Oct |
| D56 | Skills and commands execute in the **project's Windows shell with PATH passthrough**; WSL is **opt-in per agent** for Linux-tooling tasks. No WSL-by-default, no container isolation in v1. | grilling round 4, 21 Oct |
| D57 | Tool hosts show as **one merged best-guess list** in the UI; underneath, `tool_status` stays per-host (P6.1 unchanged) and detail views reveal host presence — so the footgun of picking an unreachable tool has an escape hatch. | grilling round 4, 21 Oct |
| D58 | An interactive command gets a **bounded wait, then kill and record**: the command item is marked `killed: waiting for input` and the agent is told to use a non-interactive form. No canned input, no human-attach pane in v1. | grilling round 4, 21 Oct |
| D59 | Skills are **frozen per session** for the native agent too (D46 extends); reproducible pins, bounded by the 24 h session window (D35). | grilling round 4, 21 Oct |
| D60 | First run seeds the shelf with a **small curated, hand-reviewed list** indexing on first launch; the empty state still offers indexing your own repos alongside it. | grilling round 4, 21 Oct |
| D61 | Skill invocations render as **collapsed, expandable tool lines** in the transcript — consistent with the existing transcript pattern. | grilling round 4, 21 Oct |
| D62 | Private local skills are registered **by reference**, not copied: no blobs, no clustering, and a deleted source file marks the row `broken` on the next reconcile. | grilling round 4, 21 Oct |
| D63 | Uploads are **session-scoped and re-readable** via a tool until the session ends. Cap: **100 MB, any type** — large or binary uploads are summarized/labeled on ingest, never raw-injected into a prompt. An explicit **adopt-as-project-content** action turns an upload into a normal file from then on (D4 amended by this carve-out, Q40). | grilling round 4, 21 Oct |
| D64 | **Merge-back: auto-merge gated by cross-agent review.** On turn completion Rigel attempts the merge automatically; before it lands, the teammates that are running read the diff and record findings on the merge item — they pick up one another's mistakes. Non-overlapping hunks land with attribution; a blocking reviewer flag **pauses** the merge for you; otherwise merge proceeds with the review record attached. Recommended held open only when review flags, or a conflict row exists. The user delegated the remaining parameters (reviewer set, blocking-class definition) to the implementer this round; revisable. | grilling round 4, 21 Oct |
| D65 | **Public visibility is deferred to 1.0** (signed MSI line, D37). Supersedes the "open source, build in public" *timing* from [answer 59](interview-answers.md); the MIT OR Apache-2.0 license is unchanged and files in the repo stay as they are. | grilling round 4, 21 Oct |
| D66 | Worktree agents **keep running past the 24 h idle auto-end** (D35): the wall clock closes the session log, not the agent. You are notified, and the finished work lands as a pending merge in the next session — never a silent auto-finalize of half-finished work. | grilling round 4, 21 Oct |
| D67 | Agent names are **auto-generated and editable** (`codex-1`, `claude-2`, renameable in the roster) — stable IDs for ledger attribution, friendly for the awareness block (D52). | grilling round 4, 21 Oct |
| D68 | **Teammate awareness, final shape.** The full-broadcast clause of D52 is **overturned on re-ask in plain language** (the cost was underweighted): agents receive a live roster block plus **running status summaries** of teammates, and a full teammate transcript is **readable on demand via a tool call** — nothing hidden, but nothing shepherded into every prompt. The claim gate (D53) and coordinator shape (D54) were re-confirmed unchanged. | grilling round 5, 21 Oct |
| D69 | **Provider coverage on this machine:** the user holds an **OpenAI** key and a **Google (Gemini) API key**, and wants a **small local model running on this PC** joining the agent team as the slow-but-cheap teammate — explicitly accepted as slower. Rigel treats the local model as a normal provider connection (via the router's local endpoint), and the roster labels it `local / slow`. Local-model turns are best suited to cheap roles (review pass, deliberation, summarising). **Setup verified 21 Oct:** Ollama connected through OmniRoute (`ollama-local`), winner of the head-to-head race is `qwen35-4b-q4-original` with `think: false` (without it the model narrates its reasoning into replies); evidence in `docs/qa/local-model.md`. | grilling round 5, 21 Oct |
| D70 | **Remote access is a future want, not a v1 goal:** the user pictures checking on agent work from a phone or another machine someday. Nothing remote ships in v1; the architecture must not preclude it (local-first state, no assumptions that the UI and core are always on the same machine). | grilling round 5, 21 Oct |
| D71 | **Code signing is deferred for the beta.** The first installable builds are **unsigned**, published with a SHA-256 next to the artifact; the signing pipeline stays in the repo and the throwaway-certificate exercise stays in `scripts/p7-exit-test.sh`. P7.5 is re-scoped from "signed installer" to "post-beta"; this confirms D49 and amends D37's signed-MSI clause. | grilling round 6 (Q87), 22 Oct |
| D72 | **The first public version is `0.1.0`, labelled beta**; `1.0` is reserved for the day P7.9 passes on a machine nobody built on. Note the collision with D71 on *when* the beta goes out — put to the user as Q102. | grilling round 6 (Q88), 22 Oct |
| D73 | **The Windows floor is stated honestly:** 1809 remains the documented technical floor (WebView2 requires it), and the verified line is **Windows 11 22H2+**. `docs/PLATFORMS.md` must carry both sentences and never call 1809 supported. | grilling round 6 (Q89), 22 Oct |
| D74 | **The room ships as a frozen sidecar.** `run.py` and its dependencies are bundled with PyInstaller and installed with the app; `vendor/agentchattr` stays the developer-only path, and `RIGEL_AGENTCHAT_DIR` still overrides. This is what keeps PLATFORMS' "no Python required" promise true while the Chat tab works on a clean machine. The sidecar is a second runtime (S4 permits exactly one, for a surface the Rust core cannot reach) and owns no state beyond the room's own JSONL. | grilling round 6 (Q90), 22 Oct |
| D75 | **Both auth paths, one badge.** External harness agents may use their own CLI's login (subscription included); the native provider path stays keys-only. Rigel never reads or stores a subscription credential, and bills nothing for turns a vendor CLI pays for. The roster shows which path each agent is on. **Amends D21.** | grilling round 6 (Q91), 22 Oct |
| D76 | **A room transcript is copied read-only into the project database**, so resume and the transcript pane show what the room said, with its own agent rows. It is **not** ingested into the billing ledger: external CLIs bill themselves, and a cost Rigel did not observe would be the first invented number in a product whose pitch is an honest bill. | grilling round 6 (Q92), 22 Oct |
| D77 | **The room may bind a LAN interface, with real per-agent auth.** If it ships in v1 this **supersedes D70** and turns `127.0.0.1:8300` plus one session token into an authenticated multi-machine surface; the loopback-only refusal in `agentchat.rs` and the local-user sentence in SECURITY.md are then wrong and must move with it. Scope put to the user as Q103. | grilling round 6 (Q93), 22 Oct |
| D78 | **Redaction gets a sliding window.** A streaming delta holds back a partial-secret tail (at most the longest secret's length minus one) and flushes it on the next chunk or at end of stream, so a credential split across chunks cannot reach a rendered event. This closes the deviation recorded under P4.6 rather than documenting it. | grilling round 6 (Q94), 22 Oct |
| D79 | **The conflict view becomes a real three-way merge:** rendered base/ours/theirs text, and a resolution that writes a merged file through the CAS patch pipeline. Replaces the hash-only deviation under P5.4; `migrations/project/0004_patch_hunks.sql` and `diff.tsx` grow with it. This is new implementation work, not a rename. **Built 23 Oct:** migration 0010 adds `claims.base_text` — captured when the claim is taken, the one moment the file on disk is the version the claimant started from — and `conflict_sides`, which stores the three rendered versions. `ours` is the claimant's own applied patches walked forward from that base and verified against every `result_hash`; a side that does not hash to the ledger stays empty with a note. `merge::three_way` (`crates/rigel-core/src/merge.rs`) is a line-level LCS merge with `<<<<<<< ours` blocks, CRLF-aware and capped at 4 000 lines a side. `conflict_sides`, `conflict_merge` and `conflict_resolve_merged` are the app commands; the write goes through `PatchPipeline::propose_with` with the captured `theirs` hash as the CAS base — a file that moved after capture is refused with `StaleBase` — is attributed to `user`, and `conflict_sides.merged_patch_id` records the patch that carried it. The Diff view expands a row into the three panes, merges, edits and writes; the old hashes stay visible. `tests/merge.rs` covers the captured hashes, an external write recovered by merging with the claimant's version, a linear writer that needs no write, marker output, a claim collision with no observed hash, the pre-0010 fallback and a stale write refused. | grilling round 6 (Q95), 22 Oct |
| D80 | **Spawned subagents become writable**, putting worktrees (P5.5) on the main path instead of the external-agent side road. Every safety surface moves with them: claims gate (D53), pre-destructive snapshot, per-agent budget, attribution and merge-back review (D64). Closes the P5.5 deviation by removing it rather than declaring it. | grilling round 6 (Q96), 22 Oct |
| D81 | **Cursor and Cline leave the advertised matrix.** Their adapters stay in the tree behind the fixture harnesses and the exit test keeps exercising them, but README, PLATFORMS, the roster and the Phase 4 prose stop claiming five supported harnesses: Codex, Claude and Gemini are the supported set, and the other two are labelled "unverified on this machine". **Amends the Phase 4 exit-test claim, not its coverage.** | grilling round 6 (Q97), 22 Oct |
| D82 | **Diagnostics are opt-in and never sent.** An "Export diagnostics" action gathers doctor JSON, the last turn and a redacted log into one copyable bundle, and a local crash log is written beside the app data. Nothing phones home; D37's no-telemetry clause is untouched. | grilling round 6 (Q98), 22 Oct |
| D83 | **The updater may check at startup.** The Ed25519 manifest check runs once at launch (behind the existing switch), which **amends D37's opt-in-only clause and P7.7's "nothing checks at startup"**. Consequences to carry: the settings copy must say a request is made at launch, the offline path must fail silently and fast, and the update test that asserts no startup check must be rewritten. | grilling round 6 (Q99), 22 Oct |
| D84 | **The first users come from a public launch post**, not one-at-a-time recruiting. **Collides with D65** (public visibility deferred to the 1.0 signed line) and with D71 (unsigned beta): a launch while unsigned is the largest risk this round accepted, and it is the question Q102 settles. | grilling round 6 (Q100), 22 Oct |
| D85 | **No paid surface in year one.** MIT/Apache-2.0 stays, nothing is gated, and the shared cross-machine room/claims plane is explicitly later. Adoption and interventions-per-task (D17) are the only measures. | grilling round 6 (Q101), 22 Oct |
| D86 | **The release sequence is settled: an unsigned private beta now, a public launch only once a trusted certificate exists.** D71's unsigned builds go to a handful of people with a published SHA-256; D84's launch post waits for signing, so D65's "visibility at 1.0" becomes "visibility once signed". D72's condition is corrected rather than dropped: it is the *public* beta that waits for the certificate, not the private one. P7.5 is therefore ordered in parallel with the beta, not after it. | grilling round 6 follow-up (Q102), 22 Oct |
| D87 | **The LAN room is v1 scope, not a direction.** D77 stands as scope: 0.1.0 may bind a LAN interface with real auth, which supersedes D70's "nothing remote in v1" **for the room only**. Shipping with it: a bind setting, a token per agent instead of one session token, the auth design in SECURITY.md, and tests for a wrong-token agent and a foreign-origin post. Everything else in D70 (phone access, cross-machine sessions) stays out. | grilling round 6 follow-up (Q103), 22 Oct |
| D88 | **Inline hunk comments are in 0.1.0.** A comment attaches to a `patch_hunks` row, names its author agent, and is delivered to that agent as a turn. The Diff tab gains a comment box per hunk; the patch stays read-only. **Built 23 Oct:** `hunk_comments` (migration 0009) stores the hunk, its patch, the hunk's author and the commenter separately; `HunkComments::add` copies the author from `patch_hunks`, refuses an empty body and caps at 4 000 chars. `hunk_comment_add` / `hunk_comments` / `hunk_comment_deliver` are the app commands; delivery sends `path + @@ header + body` to that agent through `turn_send` and records the turn id, and delivering twice keeps the first id. The Diff tab has a thread and a comment box per hunk with a `send to <agent>` button; the patch's status and diff are asserted unchanged. `crates/rigel-core/src/comments.rs`, `migrations/project/0009_hunk_comments.sql`, `tests/comments.rs`, `ui/src/diff.tsx`. | grilling round 7 (Q104), 22 Oct |
| D89 | **Per-turn checkpoints are in 0.1.0.** `snapshot.rs` takes a checkpoint at the end of every turn (in addition to the pre-destructive one), lists it in the transcript, and restores it on request. The pre-destructive snapshot keeps its own id and reason. | grilling round 7 (Q105), 22 Oct |
| D90 | **Desktop notifications are in 0.1.0** for three events: approval waiting, turn finished, turn failed. Per-event toggles, no sound by default, no notification for a turn the user is watching. | grilling round 7 (Q106), 22 Oct |
| D91 | **Rules interop is in 0.1.0.** The session preamble reads `AGENTS.md`, `CLAUDE.md` and `.cursor/rules/*`; Settings lists which sources were used; Rigel's memory can be exported as an `AGENTS.md` block. Reading is additive — D2/D43 memory stays the source of record. | grilling round 7 (Q107), 22 Oct |
| D92 | **A cross-session spend view and per-task caps are in 0.1.0.** A query over existing usage rows answers cost per project and per agent over time, and a task may set its own cap below the session cap. **Amends D10 additively** — the session cap is still the outer bound. | grilling round 7 (Q108), 22 Oct |
| D93 | **skills.sh import is in 0.1.0**, storing the audit verdict (Gen/Socket/Snyk) with its date as trust evidence. Rigel's own arithmetic stays the visible score; an audit is an input, not a grade. **Built 23 Oct:** `rigel skills import --skills-sh <page>` parses the `/security/<vendor>` links, reads each verdict's own date from the audit page (`Audited by Socket on Sep 15, 2026`), indexes only that skill's body (`GitHubSource::with_slug`), and writes one `audit:<vendor>=<verdict>` evidence row per vendor, labelled `audited`. A page with no audit section is refused and writes nothing. `crates/rigel-core/src/skills/skillssh.rs`, `tests/skillssh.rs`; verified live against `vercel-labs/skills/find-skills`: three verdicts dated 15 Sep 2026, one body indexed, grade still Rigel's own `C`. | grilling round 7 (Q109), 22 Oct |
| D94 | **Agent role presets are in 0.1.0:** implementer, reviewer, tester, researcher — each a saved prompt, model/effort pair and tool allowance, applied from the roster. | grilling round 7 (Q110), 22 Oct |
| D95 | **Session export is in 0.1.0:** one Markdown bundle per session (prompt, turns, tool calls, per-agent spend, linked patches) with the JSON beside it. Shares the assembly path with D82's diagnostics export. | grilling round 7 (Q111), 22 Oct |
| D96 | **The shelf is queryable over MCP in 0.1.0:** `rigel_skills_search` and `rigel_skills_show` return id, source, trust score, pins and install path from the local index. No network. | grilling round 7 (Q112), 22 Oct |
| D97 | **PR creation from the Diff tab is in 0.1.0**, only when `gh` is present and authenticated, only on an explicit click, composing title and body from the turns. Rigel never holds a forge token. | grilling round 7 (Q113), 22 Oct |
| D98 | **Plan mode is in 0.1.0:** a read-only first phase writes a plan and proposed claims to the transcript and waits for approval before any edit or command. | grilling round 7 (Q114), 22 Oct |
| D99 | **The task board is deferred to after 0.1.0.** It is a view over worktrees and writable agents (D80); building it before D80 would show tasks that cannot execute. | grilling round 7 (Q115), 22 Oct |
| D100 | **The MCP catalogue is deferred.** The read-only registry view can come sooner than the trust cards; the scanning half is not scoped. | grilling round 7 (Q116), 22 Oct |
| D101 | **A skills lockfile is deferred** until the Agent Skills `skills.json` RFC settles; session pins (D9/D59) remain the guarantee in the meantime. | grilling round 7 (Q117), 22 Oct |
| D102 | **A mobile read-only room view is deferred** until after D87 has been used on a second machine. | grilling round 7 (Q118), 22 Oct |
| D103 | **Remote approvals are deferred**, and are gated on D102 having shipped: approval is the first remote action that can destroy work. **Narrowed by D167** for a room *decision card*, which is a question an agent asked in the open rather than a tool approval. | grilling round 7 (Q119), 22 Oct |
| D104 | **Issue import is deferred** behind the task board (D99); it is one integration per tracker. | grilling round 7 (Q120), 22 Oct |
| D105 | **Lifecycle hooks are deferred.** MCP already covers most of the ground without a second scripting layer inside the turn loop. | grilling round 7 (Q121), 22 Oct |
| D106 | **A merge queue is deferred** behind D80: it earns its keep only when several writable agents merge at once. | grilling round 7 (Q122), 22 Oct |
| D107 | **Container sandbox execution is in 0.1.0 — and it is the riskiest thing this round added.** It **supersedes D56's "no container isolation in v1"** and collides with PLATFORMS' "no package manager required": a real container needs Docker Desktop or WSL2, neither of which the installer ships. The only shape that keeps both promises is an **optional backend**: `run_command` detects a container runtime, offers it per agent, and falls back to today's approval + cwd-jail path when none exists. The app must start, run a turn and pass P7.9 with no container runtime present. Decide the backend (Docker Desktop vs WSL2) before writing code. | grilling round 7 (Q123), 22 Oct |
| D108 | **The signing identity is a US/Canada individual**, which makes Azure Artifact Signing's individual Public Trust route available at **$9.99/month** — the cheapest compliant route found ($120/yr, no hardware token, key in Microsoft's HSM). Identity validation (1–20 business days) is the long pole and can start before any code is written. | grilling round 8 (Q124), 22 Oct |
| D109 | **The signing budget is $0 — stay unsigned.** This **collides with D86** (the certificate gates the public launch) and with D110 (a signed direct installer plus winget). Pending Q136, which reconciles them; until it is answered, no release step may rely on either. | grilling round 8 (Q125), 22 Oct |
| D110 | **Direct download first, Store later.** A signed MSI/EXE from GitHub Releases, winget as the package channel, and the Microsoft Store revisited after 1.0 — a second packaging pipeline (offline WebView2, +127 MB, MSIX via a Windows-11-only CLI) is not a beta task. | grilling round 8 (Q126), 22 Oct |
| D111 | **winget plus a Scoop bucket at the first signed release.** Both are manifests, not code: winget wants a stable URL, SHA-256 and a silent install (all exist), and the Scoop bucket is a repo Rigel owns. Chocolatey only if enterprise users ask. | grilling round 8 (Q127), 22 Oct |
| D112 | **Ten hand-onboarded users before any public launch**, then Show HN, then Console.dev and r/rust/r/tauri. Product Hunt is optional and later; paid promotion is not the plan. The evidence is that launches produce attention and attention converts only when installation needs no help. | grilling round 8 (Q128), 22 Oct |
| D113 | **Three writable agents at once, unlimited read-only.** The cap is a safety default, configurable upward, and sits alongside D80's writable subagents. Evidence: multi-agent traces fail 41–86.7%, and a reported 13-worktree run lost work to Git lock contention. | grilling round 8 (Q129), 22 Oct |
| D114 | **Plan approval is mandatory before a writable agent's first edit**, skippable per task with a recorded reason. Uses D98's plan phase; read-only agents are exempt. | grilling round 8 (Q130), 22 Oct |
| D115 | **A clean-context reviewer blocks writable-agent merges by default**, overridable with a recorded reason. D64's review becomes blocking rather than advisory; the reviewer sees the diff, not the author's context. | grilling round 8 (Q131), 22 Oct |
| D116 | **A $20/day global spend cap is on by default**, with a visible alert and a one-click raise; it sits above D10's session cap and D92's per-task cap. A cap that can be raised in one click is a guardrail, not a lock. | grilling round 8 (Q132), 22 Oct |
| D117 | **Imported rules and skills are read, displayed, and never treated as authority.** The transcript names which rule sources loaded, a skill's body is shown before first use (D93), and no skill script runs outside the normal approval path. D91's interop stands; this settles its trust treatment. | grilling round 8 (Q133), 22 Oct |
| D118 | **`rigel bench` is built internally now; numbers publish only after 30 real tasks.** The harness is also how D88–D98 get verified, and it measures the claim Rigel can actually defend — bounded, reviewable, billed — not SWE-bench. | grilling round 8 (Q134), 22 Oct |
| D119 | **Support is GitHub issues plus Discussions; Discord only if demand appears, with no support promise.** D82's diagnostics export is the intake format. | grilling round 8 (Q135), 22 Oct |
| D121 | **One provider table, and Gemini gets a second door.** The chat adapters were three hand-written matches (window, CLI, keychain) and nothing checked which ids each one may be sent, so the refiner's `google/gemini-4-argon:batch` was answered `404 … cannot be used with the chat/completions endpoint`: the id named the provider's *batch* adapter. Adapters now live in one table (`crates/rigel-core/src/agent/providers.rs`) — OmniRoute, OpenRouter, Gemini (AI Studio), **Vertex AI**, OpenAI, xAI, Groq, DeepSeek, Mistral, Together and Fireworks — `chat_model_id()` drops a batch-only suffix before the body is built, and an id another door owns is replaced by that provider's default rather than sent. Anthropic stayed out of this table until it had an adapter for the Messages API, which is not this contract (**D122**). | user, 27 Oct |
| D120 | **Signing route: SignPath Foundation first, Azure Artifact Signing as the fallback.** SignPath signs qualifying OSS for free, which fits the MIT/Apache licence already chosen; Azure's individual Public Trust route is open to a US/Canada individual at **$9.99/month** if SignPath declines. Start the SignPath application and the Azure identity validation in parallel, because validation is 1–20 business days and it is the long pole. **Supersedes D109's blanket $0:** the budget ceiling is $120/year, not zero. D86 and D110 stand — nothing public ships unsigned. | grilling round 8 follow-up (Q136), 22 Oct |
| D122 | **Claude's Messages API gets an adapter of its own** — D121's open edge. `crates/rigel-core/src/agent/anthropic.rs` streams that API beside the OpenAI-compatible rows while the provider **row** stays in the same table, so Settings, `rigel keys status` and the picker needed no branch. Two wire facts are pinned by tests because guessing either is a 400 on every model: `temperature` is not sent (API version `2026-10-21` removed the sampling parameters — this is the first adapter whose request shape is version-specific), and reasoning is steered with `output_config.effort` rather than a `thinking` budget (manual mode is deprecated on 4.6, rejected by 4.7 and later). Ids are Anthropic's own `claude-…`; the default `claude-opus-5-5` has a row in `pricing.rs`, and other listed models fall back to the conservative estimate until a row is added. Answering Q72 option C's premise: the stored `rigel/anthropic` key now has a provider that uses it. | user, 27 Oct |
| D123 | **The model picker opens on rows that can answer a chat turn; the rest are one toggle away.** The merged roster was 1 222 rows on this machine and 212 of them were not chat — 185 `aihorde/…` Stable-Diffusion checkpoints, 25 OpenAI ids (embeddings, moderation, image), 2 on OpenRouter (`google/lyria-3-*`); the counts move with the vendors, the shape does not — so the first screen of a chat picker is not a checkpoint. The rule lives in the core (`catalog.rs::is_chat_like`) so the window, `rigel models` and the JSON face cannot disagree: a row is hidden when the vendor declares **every** output non-chat (`architecture.output_modalities`, or the `input->output` string) **or** its id names a non-chat family. The id test is phrase-matched on separators, learned from two live false positives — `dva/inkling-*` must not be hidden for containing `kling`, `sora-2` must be — and the declaration alone cannot decide: `google/lyria-3-pro-preview` declares `text+image->text+audio` (a music model still returns text about the track) while `openai/gpt-5-image` declares `text+image+file->text+image` (a chat model that can also draw), and only the id separates them. Hidden rows are **kept**: the toggle names the count and shows them, `rigel models --all` prints them with a `non-chat` tag, and `--json` carries every row with its `chat` flag. | user, 27 Oct |
| D124 | **The picker's chat filter is a saved setting, not popover state.** D123 shipped the toggle as component state, so every launch reopened on chat rows even for a person who works with image or audio models. `Settings::chat_models_only` — default `true`, `#[serde(default = "default_true")]` so an older `settings.toml` gains it rather than failing to parse — is written through the same `settings_set` command as the model, from both surfaces that host the picker (Session and Settings → Agent), and the window has one default-settings shape (`ui/src/api.ts::defaultSettings`) instead of a copy per caller. The filtering rule stays in the core; this is only where the toggle's choice lives. | user, 27 Oct |
| D125 | **The window's layout pass fixes the squeeze and makes one selection language.** The Session header had collapsed its title to 108 px at the default window size — the tools row's max-content width won every flex negotiation, so a 24 px heading wrapped to one word per line — and the same pattern was waiting in Settings. `.detail-head` now wraps its actions under the title at 320 px, `.session-tools` has a flex basis, and the roster/inspector columns give the middle pane 48 px back below 1180 px. Selection is one amber inset bar everywhere (shelf, agent roster, chat channels, settings nav, model rows). Three CSS defects surfaced in the pass: the composer's agent `<select>` drew with undefined `--line`/`--panel`, four rules used an undefined `--well`, and one used `--amber` instead of `--accent`; `--faint` was 3.97:1 on the background — below the brief's non-negotiable AA line — and is now `#7b838d` (5.0:1), with the token table updated. The picker gained a titled header with a working close (focus returns to the trigger), a one-line filter count that is right-aligned, and scroll containment for short windows; Settings now names the file it actually writes, `settings.toml`. | user, 27 Oct |
| D126 | **The light theme is shipped as a second palette, not an inversion.** The brief required light and the setting already existed (`Settings::theme`, `dark` by default) but only `data-theme` was applied — there was no light palette, so choosing it changed nothing except the native scrollbars. `ui/src/theme.ts` now owns the value (unrecognised or unreadable means dark), `App` applies it as the window mounts, and Settings → Appearance is a two-card picker whose cards preview the theme they select; the choice saves through the same `settings_set` path as every other preference. `shell.css` carries a `:root[data-theme="light"]` block and every rule reads tokens, which is why no component needed a branch. Text was chosen against white and measured: muted 6.65:1 on white / 5.93:1 on raised, faint 5.49 / 4.90, accent text `#9c5d00` 5.28 / 4.71, verified 5.43 / 4.84, danger 6.57 / 5.87 — the darkest surface is the number that matters, not white. Fills, borders, focus rings and the cursor keep the amber identity; only text in the accent hue darkens. xterm cannot read CSS tokens, so the terminal carries two complete ANSI palettes (every light slot at least 4.5:1, including bright black, which is what a prompt writes metadata in) and swaps them in place through a `data-theme` observer — the PTY and its scrollback survive the switch. The preview banner stopped using a fixed 256-colour grey for the same reason. Two light-only defects surfaced in the audit: the status bar's separators were painted in the 7–10% hairline and disappeared on white (a `--sep` token now carries punctuation and link underlines in both themes), and the chat channel dot's state class was overridden by an inline per-agent colour (2.98:1 on white), so it now reads the same verified/busy/down tokens as everything else. The picker's two cards are `<button>`s, which the UA lays out with centred flex children — without `align-items: stretch` the preview swatch collapsed to its 24px padding and the bars measured zero, a bug only a screenshot caught. | user, 27 Oct |
| D127 | **Attached images reach vision models — after the signed-beta gate.** An attachment is a path today, and `read_file` answers `BINARY` for anything not UTF-8. The composer will inline bytes for rows that declare image input (OpenRouter/OpenAI-compatible `image_url`, Anthropic image block, Gemini `inline_data`), with MIME sniffing, a 5 MB cap, a downscale above 2,000 px, and a "text-only model" note when the picked row cannot take one; images the user did not attach are never read. | grilling Q176, 30 Oct |
| D128 | **An unsent draft belongs to its session and survives a restart.** One draft per project + session in `localStorage`, keyed by project root and session id (never the prompt text), restored on select and cleared on send. | grilling Q178, 30 Oct |
| D129 | **A session forks from a chosen turn, after the gate.** The fork copies the transcript prefix, roster and pinned skills, records `parent_session_id`, and may pick a different model; patches stay with the original and the fork starts from the tree as it is now. The copy rule and the schema change are the work. | grilling Q179, 30 Oct |
| D130 | **Model, effort and steps stay machine-wide — per-project rows were declined.** The picker's choice remains turn-scoped and Settings holds the one default; the precedence question is closed rather than deferred. | grilling Q180, 30 Oct |
| D131 | **Claude Code and Codex sessions import with full resume — read-only import was declined.** The importer owns what it parses: turns are resumable and tool calls may replay. Accepted consequence: two moving formats, so an unrecognised version is refused rather than guessed, and every imported turn keeps its origin tag. | grilling Q181, 30 Oct |
| D132 | **Two writable children may race on one file list, after the gate.** Both claims are recorded as a race, their pending diffs sit side by side, one is merged and the other's worktree pruned; spend doubles by design and the cap still applies. | grilling Q182, 30 Oct |
| D133 | **A turn pauses at the next step boundary.** `paused` keeps the message list, skills and remaining budget, holds no room, and Resume continues the same turn; cancel still throws it away. | grilling Q183, 30 Oct |
| D134 | **A finished turn gets a "Hand to…" action.** It prefills a prompt naming the changed files and the outcome for the chosen agent; the user edits and sends, and nothing starts without the send. | grilling Q184, 30 Oct |
| D135 | **Each agent can have its own model.** A per-agent picker at spawn and in the roster, defaulting to the session model; coordinator-spawned children default to the eight-model cheap allowlist, and a mid-session change is recorded in the transcript. | grilling Q185, 30 Oct |
| D136 | **The transcript filters to All / Tools / Diffs / Errors / Yours**, remembered per session, keeping every turn header and the running state. | grilling Q186, 30 Oct |
| D137 | **The keyboard layer is rebindable — the fixed map was declined.** A shortcuts page, conflict detection and a `?` cheat sheet replace the six fixed keys; the accepted cost is a config surface, and a new view must ship with a default binding. | grilling Q187, 30 Oct |
| D138 | **Focus mode hides the rail and inspector for the current view**, toggled with `Ctrl+.` and remembered per view; the list stays. | grilling Q188, 30 Oct |
| D139 | **Clicking a notification opens the session and turn that raised it**, and an approval notification opens its card; the payload carries ids, never paths or prompts. | grilling Q189, 30 Oct |
| D140 | **The shelf pins a skill to the session, and a cheap scout proposes skills.** Pin writes the current body hash, shows a composer chip, names the skill in the turn's preamble, and gains unpin/list calls. A lite agent — the cheapest allowlist entry — reads a compact session summary, searches the local index and offers a few candidates to pin; it runs from a button and once when a session's first turn starts, never per turn. Task agents get master prompts shipped per role (chat, implementer, reviewer, tester, researcher), editable in Settings and recorded in the transcript when used. A pin remains untrusted instruction, not permission. | grilling Q190, 30 Oct |
| D141 | **A moved pin shows its change**: a changed badge in the session's pinned list, a diff between the pinned and current bodies, and Keep pin / Update, with the update reason recorded on the session. | grilling Q191, 30 Oct |
| D142 | **A source gains Pause and a two-step Forget.** Pause stops refreshing and keeps rows and history; Forget deletes rows and blobs except a body a session pinned, which is kept and marked `missing`. | grilling Q192, 30 Oct |
| D143 | **The picker remembers starred models and the last five picks**, saved in `settings.toml`, with the current model visible whatever the filter. | grilling Q193, 30 Oct |
| D144 | **The picker shows published context length, input/output price and modality tags**, "unknown" where a vendor publishes nothing and no derived price; the vision flag is what D127 reads. | grilling Q194, 30 Oct |
| D145 | **More curated providers, ordered by cost and latency, with a character filter — the closed-table recommendation was declined.** Add OpenAI-compatible vendors that publish pricing (Cerebras, Nvidia NIM, Perplexity, Lambda, Hyperbolic, SambaNova and similar), each passing the same door rules; no arbitrary base URL. The picker sorts cheapest → most expensive with unknown prices last (D144) and by latency measured from real turns (time-to-first-token and total, "unknown" until a model has run); a provider switch is one click. "Less filtered" is a maintainer-curated, editable id list shown as a filter, never inferred from a name. | grilling Q195, 30 Oct |
| D146 | **Attaching a project file references it in place, and the `@` popover is not built.** The native picker already returns paths, but `attachment_import` copies every file into `.rigel/attachments`, so an attached source file is a shadow copy the agent edits instead of the project's. A file whose canonical path is inside the project root will be returned as its project-relative path with no copy; files outside keep the copy and the `.gitignore`, directories stay refused, and the prompt line distinguishes `in place` from `copied`. This covers the need Q177's completion menu was for; no separate file list or pane. | grilling Q177, 30 Oct |
| D147 | **One `Ctrl+K` palette searches actions, skills, sessions, turns and patches.** A single core `search_all` call, capped per section, current session first; choosing a turn opens its session at that row. The palette is a renderer over existing rows plus a turn-body index, not a second store. **Built 30 Oct:** `crates/rigel-core/src/search.rs` binds the query, escapes `%`/`_`/`\`, and returns sessions, turns (prompt matches first, then item bodies in the 500 most recent top-level turns, deduped per turn) and patches; `search_all` adds up to six shelf hits and `ui/src/palette.tsx` renders the sections with ↑/↓/Enter/Escape. A jump to a turn opens the named session in `SessionView` and marks the turn; a jump to a patch makes that session active in `DiffView` and marks the patch; a skill jump falls back to the loaded detail when the row is not on the current shelf page. `Ctrl+K` is ignored inside the terminal, where the shell owns the key. Tests: `crates/rigel-core/tests/search.rs` (title, prompt, item-body and patch matches; empty query; current-session ordering; a literal `%`). | grilling Q196, 30 Oct |
| D148 | **The window gains a session list with rename, pin, archive and a text/status filter.** There is no session list today — `SessionView` opens the newest non-ended session — so the list itself is the feature; `pinned_at` and `archived_at` are new nullable columns (`title` already exists). Archived sessions are hidden by default, still searchable and resumable, and the automatic title stays the fallback. **Built 30 Oct:** migration `0012_session_list` adds both columns and an index on `pinned_at`; `Session` carries them, `list()` returns pinned first then newest, and `rename` / `set_pinned` / `set_archived` each return the stored row. Rename keeps one line and 120 characters and treats whitespace as "use the automatic title"; archiving never ends a session. `session_rename` / `session_pin` / `session_archive` cross IPC, and the Session pane gained a **Sessions | Agents** switch: text filter, status filter, an archived toggle, inline rename (Enter saves, Escape cancels), pin and archive/restore on a hovered row, with archived rows going back to hidden but staying in the palette and in `list()`. The tab is a mode inside the existing pane, not a seventh rail surface. Tests: `crates/rigel-core/tests/sessions.rs::a_session_can_be_renamed_pinned_and_archived` covers trimming, the one-line/120-character boundary, clearing, pinned ordering across a reopen, returning to newest-first, archiving without ending, and the three not-found paths; `ui/tests/sessions.spec.ts` drives the pane in browser preview through filter, rename, pin, archive and restore. | grilling Q197, 30 Oct |
| D149 | **A turn outline rail, derived at render time.** One mark per user prompt, diff, error and approval, with a sticky current position; no schema, and it carries D136's filter counts. **Built 30 Oct:** `outline` is a `useMemo` over the same pane entries the filter counts read; marks are prompt / diff / error / approval, sized and coloured per kind. `trackOutline` follows scroll against a 32px line from the top and marks the group the reader is in; clicking any mark centres that turn and flashes it. The rail is hidden in the two-pane comparison, where it would describe neither pane. No table was added. Test: `ui/tests/transcript.spec.ts` checks the marks exist and the jump runs. | grilling Q198, 30 Oct |
| D150 | **Bookmarks: a star on any transcript item or patch hunk**, listed per session with jump links and stored in the project database. **Built 30 Oct:** migration `0013_bookmarks` (`session_id`, `kind` of `item` or `hunk`, `target_id`, `label`, `created_at`, unique per target); `SessionService::toggle_bookmark` returns the stored row or `None`, `bookmarks()` lists newest first. The star is on every transcript line and every hunk head; the inspector lists the session's bookmarks with jump links (a hunk jump opens Diff on that hunk through `onOpenHunk`). Labels are cut at 160 characters and the kind is validated in the core. Tests: `crates/rigel-core/tests/sessions.rs::a_bookmark_toggles_and_stays_per_session`; `ui/tests/bookmarks.spec.ts`. | grilling Q199, 30 Oct |
| D151 | **Composer history and snippets.** Up/Down cycles the session's prompts (a modifier widens it to all sessions), boundary-aware; a snippets menu with placeholders. Nothing sends without the button. **Built 30 Oct:** history is `localStorage` per project (last 100 prompts, recorded only when a turn is delivered); Up/Down only move when the caret is on the first/last line, `Ctrl+Up` widens to the whole project, and the draft typed before recalling comes back at the end of the list. The snippets menu is project-local (`rigel.snippets.<project>`), offers three starters, inserts text with the first `{placeholder}` selected, and saves the current draft as a snippet. Inserting is never sending — Enter still opens the review card. Test: `ui/tests/composer.spec.ts`. | grilling Q200, 30 Oct |
| D152 | **Project-local session templates.** Model, effort, steps, roster, pinned skills and a prompt skeleton, offered at create and stored per project — never committed, because model ids are machine-specific (N7). **Built 30 Oct:** `crates/rigel-core/src/templates.rs` reads and writes `.rigel/templates.json` (atomic temp + rename, 40-template cap, one-line 60-character names, missing file is an empty list); `template_list` / `template_save` / `template_delete` cross IPC. `+ session` opens the project's templates when there are any, with Blank session still one click; starting from one creates the session, spawns its roster with each agent's own model, applies the template's model/effort through the picker's path (a turn reads one window-wide default, D130) and steps through `settings_set`, and leaves the prompt in the composer, unsent. "save template" stores the current session's shape. **Not in the template yet:** pinned skills, because the window still has no skill-pin surface (D140); the menu says so by omission rather than saving a field nothing applies. Tests: `crates/rigel-core/tests/templates.rs`; `ui/tests/templates.spec.ts`. | grilling Q201, 30 Oct |
| D153 | **A cost preflight line, shown as a range.** Estimated from the last N comparable turns and the prompt's tokens, "no history yet" when the sample is small; a warning that never blocks send. **Built 30 Oct:** the last eight completed top-level turns are the sample; the 20th–80th percentile of their bills is scaled by the draft's tokens against the sample's median prompt (clamped 0.4–3×). Under three completed turns the line says "cost history: no history yet — N of 3 completed turns recorded" instead of inventing a range. The line sits under the first-step quote, names the sample size in its tooltip, warns when the high end exceeds the remaining cap, and never touches the Send button. Test: `ui/tests/composer.spec.ts`. | grilling Q202, 30 Oct |
| D154 | **An Attention view with a rail count.** Pending approvals, failed turns, budget stops, stale claims, unmerged reviews and blocked merges, grouped by session with jump links; unresolved items only, cleared by resolution. **Built 30 Oct:** `crates/rigel-core/src/attention.rs` reads each kind from the ledger — a failed or budget-stopped turn waits until that agent completes a later turn, a claim waits until it is stale and is cleared by release, and a review waits until merged (an overridden block is not listed) — and returns groups newest first with archived sessions included. `attention_list` adds the bus's open approvals as one “Waiting on you” block. **Follow-up 31 Oct:** `PendingApproval` carries optional `project_root`, `session_id`, `agent_id` and `turn_id`; `turn_send`'s `WindowApprover` records the project and all three ids from the turn it was built for, and the MCP permission tool records the project, session and agent it knows. `Attention::with_approvals` keeps only questions whose project matches the open one — a file with no project stays visible rather than hiding a live question — and maps the origin into each row, so an approval opens the turn that asked; an older queued file without ids still opens the Session card. The rule is `PendingApproval::belongs_to`, and the Session card's `approval_pending` takes the open project and reads `ApprovalBus::pending_for`, so another project's question is not listed there either (its “approval waiting” notification follows that read). `ui/src/attention.tsx` renders the groups with a kind tag and a jump: a turn row opens the session at that turn, a claim or review opens Diff, an approval opens the Session card. The rail badge polls the same read every ten seconds, and the view every five. Tests: `crates/rigel-core/tests/attention.rs` (each kind lists and then clears at its resolution); `ui/tests/attention.spec.ts` (badge count, grouping, approval targets its recorded turn, block → Diff); `crates/rigel-core/tests/attention.rs::approvals_lead_the_view_and_keep_the_turn_that_asked` covers the merge, the other-project question left out, and the older file with no origin. | grilling Q203, 30 Oct |
| D155 | **A while-you-were-away card on focus.** Turns finished, patches merged, approvals waiting and spend since the last focus; dismissible, and complementary to D139 rather than a second notification path. **Built 30 Oct:** the baseline is stamped on blur (and on opening a session, so the first focus shows nothing) and read at focus from refs, never by restarting the listener per streamed item. The card appears only when a turn finished, a patch applied, spend moved or the waiting-approval count changed, then reports the current counts; Dismiss clears it and a second focus with no change keeps it away. Notification ids and jumps stay D139's. Test: `ui/tests/away.spec.ts`. | grilling Q204, 30 Oct |
| D156 | **Line provenance from a diff line to hunk → patch → turn → tool call → model.** Read-only, exact at the hunk boundary and "unknown" elsewhere; no causal claim beyond the recorded chain. **Built 30 Oct:** `crates/rigel-core/src/provenance.rs` joins the patch row to the agent, then to the newest item whose `result.patch_id` names it, then to the turn; `patch_provenance` crosses IPC. Every diff line is a row with its new-file number, and `where from` in the hunk head opens the same panel from the hunk boundary. The panel prints the hunk's old/new ranges, patch id/path/status, turn seq/status, tool and item kind, agent role/harness/model — "unknown" with the reason for any missing link, and a "complete" flag the UI honours. Tests: `crates/rigel-core/tests/provenance.rs` (full chain and an unlinked patch); `ui/tests/provenance.spec.ts`. | grilling Q205, 30 Oct |
| D157 | **A bill explorer with grouping and a counterfactual column.** By day, project, agent and model; recorded tokens priced at another model's rates, labelled arithmetic, never called a saving. | grilling Q206, 30 Oct |
| D158 | **A full accessibility pass, with a keyboard-only walkthrough in the exit test.** Labelled transcript structure, focus-visible everywhere, skip links, reduced motion and a high-contrast palette. **Built 30 Oct, window pass:** a "Skip to content" link is the first Tab stop and targets a focusable `<main id="main" tabIndex={-1}>` on every surface; every turn is an `article` labelled `Turn N, status`; the existing global `:focus-visible` ring and `prefers-reduced-motion` rule are joined by a `prefers-contrast: more` palette that raises muted/faint/hairlines in both themes. Test: `ui/tests/a11y.spec.ts` (first Tab reaches the skip link and focuses main; a turn carries its label; `emulateMedia({ contrast: "more" })` changes `--faint`). **Still open:** a keyboard-only pass of the full exit test on a real build, not the browser preview. | grilling Q207, 30 Oct |
| D159 | **UI scale, compact density and persisted pane widths.** 90/100/110/125%, compact rows, draggable widths remembered per project, and the inspector collapses before the transcript is squeezed. **Built 30 Oct:** `ui_scale` (one of 90/100/110/125, clamped in `settings_set`) and `compact_density` are saved in `settings.toml`; `ui/src/display.ts` writes `--ui-scale` and `data-density` on `<html>`, with `#root` zoomed and divided by the same factor so the shell still fills the window. Appearance gains a scale segmented control and Roomy/Compact. Two splitters over the gutters drag the list (200–420px) and inspector (220–460px), nudge with Arrow keys and reset on double-click, saved per project in `localStorage`; the inspector and its handle disappear below 1080px before the transcript is squeezed. The terminal keeps its own cell size. Test: `ui/tests/display.spec.ts` (125% still fits the window, density lands, ArrowRight moves the list to 276px). | grilling Q208, 30 Oct |
| D160 | **A taskbar overlay badge, one window.** Running-agent count and a needs-attention state; multiple windows stay out (D24). | grilling Q209, 30 Oct |
| D161 | **A read-only step-through viewer for a recorded turn.** Next/previous over recorded inputs and outputs with inline diffs, no re-execution; full deterministic replay only if a real debugging need appears. | grilling Q210, 30 Oct |
| D162 | **A two-model comparison run, after D129.** Two forks, two worktrees, the same prompt, diffs/bills/tests side by side, one merged; spend doubles by design and the cap and merge gate still hold. | grilling Q211, 30 Oct |
| D163 | **A proven-targets blast-radius preview before a destructive command.** Parse `rm`/`mv`/`git clean`/`git reset`, resolve against the cwd, list existing paths and counts, and say "cannot determine" for the rest; it never executes anything. | grilling Q212, 30 Oct |
| D164 | **Distill a session into a draft skill with a fixture, after the beta.** `SKILL.md` plus a test and trigger list, into a local draft source, shown for review and never auto-pinned; the fixture runs in the sandbox and D117's imported-body trust rules apply. | grilling Q213, 30 Oct |
| D165 | **A per-file failure heatmap from the patch ledger.** Applied, reverted, conflicted and blocked counts over a window, click to filter the patches, small samples labelled as small. | grilling Q214, 30 Oct |
| D167 | **The phone answers decision cards and reads the board and the rule book.** The room's socket already carried `jobs`, `rules` and decision cards; the iOS client dropped all three as `.ignored`. It now decodes `jobs`/`job`/`rules`/`rule`/`message_update` and draws three surfaces over the one connection — timeline, board, rules — plus exactly one new write: `POST /api/messages/{id}/resolve_decision`, behind a confirmation card, because the room records the choice on the card and posts `@sender <choice>` into the channel. **D103 is narrowed, not overturned:** a decision card is not a destructive tool approval, and tool approvals, moving a job, activating a rule and starting an agent all stay on the desktop. The phone settles on the room's `message_update`, never on its own optimism, which is the same rule the composer follows. | user, 30 Oct |
| D166 | **Shared control of one live session — the recommendation against it was declined.** Target: two people on the same session with presence, cursors, and either able to approve, pause or send. It is gated on **per-person identity** (D87 issues per-agent tokens, not people) and on D87 having run on a second machine; the read-only observer is the first milestone, not the product. Two simultaneous approvals need a deterministic rule before any code. This is the largest security surface in the round: it makes approval remote, and approval is the first remote action that can destroy work. | grilling Q215, 30 Oct |

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
| "Budget caps are **mandatory**" (D10) | §3.1 | Amended by D42: default-on with an explicit, confirmed no-cap opt-out; unattended mode still requires a cap. |
| "Memory is **one file per agent**" (D2) | §3.1 | Amended by D43: one central MEMORY.md with per-agent sections; per-agent identity survives as section headers, not files. |
| "Uploads are context only, never project content" (D4) | §3.1 | Amended by D63 (carve-out): an explicit adopt action turns an upload into a normal file; before adoption nothing changed. |
| "Full transcript broadcast" (D52, round 4) | §3.1 | Overturned by D68 on plain-language re-ask: live summaries + on-demand reads instead; the token cost of broadcasting everyone into everyone was underweighted when the first answer was given. |
| "Open source, build in public" (interview answer 59, *timing*) | docs/interview-answers.md | Amended by D65: the license is unchanged, but public visibility waits for the 1.0 line (D37). |
| Claude Code as the first harness | §15 draft recommendation | D36: Codex first; Claude deferred. |
| "API keys only, never a subscription login" (D21) | §3.1 | D75: external harnesses may use their own CLI login; only the native provider path is keys-only. |
| "Signed MSI" as the 1.0 line (D37) | §3.1, §11 Phase 7 | D71: the beta ships unsigned with a published SHA-256; signing moves after the beta, and P7.5 is re-scoped. |
| "Opt-in updater; nothing checks on a timer or at startup" (D37, P7.7) | §3.1, P7.7 | D83: one manifest check at launch behind the switch, with the copy and the test changed to match. |
| "Public visibility deferred to 1.0" (D65) | §3.1 | D86: a private unsigned beta may go out first, but the public launch waits for a trusted certificate. |
| "Nothing remote ships in v1" (D70) | §3.1 | D87: the room binds LAN in 0.1.0 with per-agent auth; other remote access stays out. |
| Hash-only conflict record, no merged file (P5.4 deviation) | §11 Phase 5 | D79: rendered base/ours/theirs and a merged file through the CAS pipeline. |
| Worktrees serve external agents only (P5.5 deviation) | §11 Phase 5 | D80: spawned subagents become writable and worktrees sit on the main path. |
| "Five harnesses supported" (Phase 4 prose, README) | §11 Phase 4, README | D81: Codex, Claude and Gemini are supported; Cursor and Cline keep their adapters and fixtures but leave the advertised set. |
| "Signing budget is $0 — stay unsigned" (D109) | §3.1 | D120: SignPath first, Azure at $9.99/mo as the fallback; the ceiling is $120/year. |

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
| Native agent | the selected adapter's credential: OmniRoute's own key, or the vendor's API key | `rigel/<adapter>` — `rigel/omniroute`, `rigel/openrouter`, `rigel/gemini`, `rigel/vertex` (a short-lived token), `rigel/openai`, `rigel/xai`, `rigel/groq`, … | — |

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
pub enum Harness { Native, Codex, Claude, Cursor, Cline, Gemini }
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
`body.unmapped = true` — visible in a debug pane, never silently dropped. The table itself is
`crates/rigel-adapters/src/normalise.rs` (P4.8), and one test there walks all ten so a kind cannot
quietly stop being reachable.

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
| `models_list` | — | `string[]` — the provider's live catalogue for the picker |
| `servers_start` | `{ project? }` | `()` — starts every local service a turn can need (router, chat server) without blocking the window |
| `chat_status` | `{ project? }` | `ChatStatus` (`running`, `authed`, `origin`, `dir`) |
| `chat_start` | `{ project? }` | `ChatStatus` — boots `run.py` and keeps the token in Rust |
| `chat_channels` / `chat_agents` / `chat_messages` | — / — / `{ channel?, since_id?, limit? }` | channels, presence, messages |
| `chat_send` | `{ channel, text }` | sends over the vendor websocket |
| `chat_start_agents` | `{ project? }` | launches the configured harnesses with `CREATE_NO_WINDOW`, returns the labels started |

The names above are the design vocabulary; the registered commands are snake_case (`turn_send`,
`session_start`, `chat_start_agents`, …).

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
| Network | No telemetry. Outbound calls: OmniRoute on loopback (native agent, which fans out to providers), the selected provider adapter's own base URL when configured that way, GitHub/marketplaces (indexing, on demand), update manifest (opt-out) |
| Router auth | OmniRoute answers `/v1/chat/completions` and `/v1/responses` from localhost **without a key** (probed 21 Oct); `/v1/models` returns 401. Rigel sends its key regardless, never treats the router as an auth boundary, and `rigel doctor` warns when router auth is disabled |
| Logs | Redact tokens by pattern; rotate at 10 MB; local only |
| Third-party terms | Rigel never reuses a vendor subscription's auth; each harness authenticates itself (§4.3.3) |

Prompt-injection stance, stated because agents read skill bodies and repo files: Rigel treats repo
content, skill content and agent output as **data**. No agent output is executed as a Rigel command.
Approvals are bound to an `approval_id` issued by the harness, not to text in the transcript.
---

## 11. The plan

**State as of 22 Oct 2026:** Phases 0–6 complete; Phase 7 in progress (7/9 — P7.5 needs
the release certificate and P7.9 a clean VM).
Run `node scripts/handoff-status.mjs` for live counts; this table is the summary.

| Phase | Scope | State | Items |
|---|---|---|---|
| 0 | Prototype | **complete** | 9/9 |
| 1 | Rust skeleton and storage | **complete** | 10/10 |
| 2 | The shelf, real | **complete** | 11/11 |
| 3 | Native agent, chat and the bill | **complete** | 11/11 |
| 4 | Harness adapters | **complete** | 18/18 |
| 5 | Claims, attribution, merged diff | **complete** | 8/8 |
| 6 | Dependencies and trust | **complete** | 8/8 |
| 7 | Hardening and open-source release | in progress | 7/9 |

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

**Review note (21 Oct 2026):** P4.2 landed before P4.1 under an explicit user waiver. Reviewing
`45ac7e7` found one defect: the OmniRoute provider asked Codex for OpenAI auth, which sends the turn
through the ChatGPT login in `~/.codex/auth.json` instead of the injected key. `27377b3` sets
`env_key="OPENAI_API_KEY"` with `requires_openai_auth=false`, so the per-agent key is the only
credential available (D21); the invented `exec` auth event is gone, since only app-server reports one.
That commit also lands P4.1.

Known gaps after P4.1: Cursor and Cline are not installed on this machine, so their probes are only
exercised as `installed: false` rows; Cursor's `Subscription` path (`~/.cursor/cli-config.json`) is
unverified on a machine that has the CLI. The Codex turn itself was not re-run after the provider fix
(one turn injects ~155k input tokens, P4.19); the overrides are covered against the fake binary and
match the documented custom-provider form. Next in order: P4.3/P4.4, with P4.7/P4.8 fixtures alongside.

**Review note (21 Oct 2026, second session):** P4.6 and P4.3 landed together because they share the
same shape — spawn a CLI, normalise its events, and make the credential boundary testable without
the vendor. Both adapters are exercised only against fixture binaries; Claude's real CLI answers
`--version` (2.1.251) and was *not* given a live turn, because one turn injects ~155k input tokens
(P4.19). Cursor and Cline are still not installed, so the ACP client is verified against
`tests/fixtures/fake_acp.rs` and nothing else.

Redaction is applied per event and again to the completed message and the stored turn, so a
credential that is echoed inside one chunk is caught. A credential split across two stream chunks
would still reach a transient event before the redacted final text replaces it; closing that needs
a streaming redactor and is recorded rather than assumed. Neither adapter has a CLI verb yet
(`rigel codex` is the only one); they are library surfaces until P4.10 wires the app's turn loop.

Two deliberate deviations, both recorded in their checklist lines: Claude does not yet receive
`--permission-prompt-tool mcp__rigel__approve` (the server it names arrives with P4.10, and a
dangling tool name would turn every permission request into an error), and P4.4 is untouched
because Cline's per-agent provider file has no verified format yet; spawning `cline --acp` without
one would authenticate as the vendor login or not at all, so `AcpClient::spawn` refuses it before
starting a process. Next: P4.4 once Cline's config shape is checked against an installed CLI, then
P4.7 fixtures and P4.8 normalisation.


- [x] **P4.1** `probe()` for all five, writing `adapters.json`; UI lists missing harnesses with an install hint — `27377b3`
- [x] **P4.2** Per-agent keychain injection; probe reports `auth_mode` and refuses `subscription` (D21) — `45ac7e7`
- [x] **P4.3** ACP client: `agent acp`, session updates, `session/request_permission` → approval UI — `AcpClient` speaks protocol v1, refuses a different major version and a relative cwd, routes every permission request through the `Approver` trait (deny-by-default), sends `session/cancel` on timeout, answers pending approvals with `cancelled` and waits for the agent's acknowledgement; unknown updates are kept as `unmapped` — `crates/rigel-adapters/src/acp.rs`, `tests/acp.rs` — `f2c8dbf`
- [x] **P4.4** ACP client: `cline --acp`, same client, separate probe — Cline now has an explicit key path (`rigel/cline` → `CLINE_API_KEY`) and `AcpConfig::cline` spawns `cline --acp`. **Verified live:** the installed Cline 3.0.73 answers `initialize` (protocol v1, `agentInfo.name = "cline"`) and `session/new` (Plan/Act modes, model list) with the key injected; `tests/acp.rs` pins the argv and that Cursor's key never reaches the Cline child. **Fixed on the way:** Windows resolution preferred npm's extensionless POSIX shim over `cline.cmd`, so Cline probed as unversioned and could not spawn; `pick_in_dir` now tries `PATHEXT` first, with a regression test. — `crates/rigel-adapters/src/{acp,registry,lib}.rs`
- [x] **P4.5** Codex adapter: app-server JSON-RPC primary (probe passed 21 Oct), `codex exec --json` fallback behind the trait. Write the `-c` overrides explicitly; do not shell out to `omniroute run`/`launch-codex` — `7b3c2fb`
- [x] **P4.6** Claude adapter: `claude -p --bare --output-format stream-json --verbose --include-partial-messages --permission-mode <mode>` — **deviation:** `--permission-prompt-tool` is not passed yet, because the MCP approval server it names does not exist until P4.10 and naming a missing tool would fail every permission request; the default policy is the CLI's own, which fails closed. `--bare` makes D21 enforceable: only `ANTHROPIC_API_KEY` authenticates, and a child that reports another `apiKeySource` is killed at its first event — `crates/rigel-adapters/src/claude.rs`, `tests/claude.rs` — `f2c8dbf`. **The deviation is closed:** `rigel claude run` writes the MCP config naming this build and passes `--permission-prompt-tool mcp__rigel__rigel_approve`, then answers `rigel_approve` from stdin while the turn streams — the same four scopes, in the same words, as the native agent. The turn is recorded like Codex's: prompt, normalised items, the CLI's `total_cost_usd` as `provider_micros`, a locally priced `Usage` row and a closed turn — `crates/rigel-cli/src/harness.rs`, `harness::session_tests`
- [x] **P4.7** Golden fixtures per harness per scenario (start, tool call, approval, error, interrupt) + replay tests — 15 JSONL streams under `crates/rigel-adapters/tests/fixtures/golden/`, replayed verbatim by the three fixture binaries through the production loops; `tests/replay.rs` (4 tests) also fails when a fixture has no scenario. Streams are authored to the documented shapes (provenance and unmapped scenarios in that directory's README): Claude/Codex tool calls and Codex approvals are pinned as gaps for P4.8/P4.10, and Codex `turn/completed` with status `interrupted` decoding as success is recorded for P4.12 — `20a1f60`
- [x] **P4.8** Normalisation for all ten item kinds; unmappable events retained as `unmapped` — `crates/rigel-adapters/src/normalise.rs` holds the mapping table (Claude tool names, Codex item types in both spellings, ACP `kind`), item bodies reuse the native agent's shapes, and an unmappable event is an `error` item with `unmapped: true` plus the raw payload. Claude decodes thinking, `tool_use` and `tool_result` (the result inherits the call's kind by id); Codex decodes commands, file changes, MCP calls, searches, plans, reasoning and errors on both transports; ACP tool calls carry their item kind — `tests/replay.rs` pins the golden `tool_call` stream per harness — `a12e7c9`
- [x] **P4.9** Roster UI (D40): nested subagents, `observed only` labels, per-agent spend and claims; at most two transcripts side by side, no tabs or grids — a subagent row is indented under the agent whose turn spawned it (derived from `parent_turn_id`, not seeded); a non-native harness carries `observed only` because Rigel cannot send it a turn yet; selecting an agent opens its own transcript and a second selection splits the pane, with a third replacing the older one. **Claims per agent are not drawn in the roster:** the ledger now exists (Phase 5) and the foot points at the Diff tab, which lists holder, staleness and conflicts — `ui/src/session.tsx`, verified in the browser preview against the sample roster — `a1388a7`
- [x] **P4.10** End-to-end approvals across all three protocols, with scope buttons (N2) — one vocabulary in `agent::approval` (`once`/`session`/`project`/`always`, keyed by the exact text the user read, stored per project and globally, never widened when a store cannot write). Codex app-server requests (`item/commandExecution|fileChange/requestApproval`, plus the legacy `execCommandApproval`/`applyPatchApproval`) are answered in place with `accept`/`acceptForSession`/`decline`; a server request is no longer mistaken for a response to one of ours. Claude gets `--mcp-config` plus `--permission-prompt-tool mcp__rigel__rigel_approve` when `with_permission_tool` is set, and `rigel_approve` answers `{behavior: allow|deny}` from the scope store, asking through an on-disk approval channel and refusing after the wait. ACP narrows a scope the agent cannot express (`always` → its `allow_once`) and never widens; the CLI remembers an `Always` answer itself. The window shows a card with all four scope buttons and Reject, backed by the same channel. `crates/rigel-core/src/agent/approval.rs`, `tools.rs`, `crates/rigel-adapters/src/{codex,claude,acp}.rs`, `crates/rigel-mcp/src/lib.rs`, `ui/src/session.tsx` — tests in `command.rs`, `codex.rs`, `claude.rs`, `acp.rs`, `rigel-mcp`; card inspected in the browser preview.
- [ ] ~~**P4.11** Sidecar decision recorded: implement it for Cline SDK-only features, or mark `dropped` with the reason~~ **dropped:** the Node sidecar is not worth reinventing Cline's SDK quirks when ACP already reaches its approval surface; Cline SDK-only features stay unavailable and the roster labels them as such (D48, 21 Oct).
- [x] **P4.12** **Exit test:** `scripts/p4-exit-test.sh` runs the matrix: `doctor --json` lists all five harnesses; a Codex turn records a `command` item for `rigel doctor --json` with `exit_code: 0` and 420 input tokens; Cursor, Cline and Gemini complete through the same ACP client with their own keys; a fifth turn is killed mid-flight, the other three complete, and `session resume` closes only the dead turn as `interrupted`; and a Claude turn records its own agent, the answer and the CLI's `total_cost_usd` as a provider cost. It passes. **Limit:** no real vendor turn is possible here (Cursor is subscription-only, Cline has no key); the script uses fixture binaries and documents how to point it at the real CLIs.
- [x] **P4.13** `run_command` tool (D30): exact text and scope shown before execution (N2), cwd jailed to the project, timeout and output cap, exit code recorded as a `command` item — audited against `tests/command.rs`: the prompt carries the exact text, cwd and clamped timeout, a cwd outside the root is refused *before* anyone is asked, both streams are capped at 64 KiB and read to EOF, the timeout kills the child, and the item records `exit_code`, `timed_out`, `approved`, `scope` and the policy rows. Denials are `command` items too, so the transcript shows what was asked for.
- [x] **P4.14** Egress tool (D33): `fetch_url` with approval per origin and once/session/always scopes, size and content-type caps, fetched text labelled untrusted in the transcript — redirects are followed by hand so a new origin asks again, the body cap is 256 KiB, only text-like content types are read, and the tool body carries `untrusted: true` with a note that a page grants no permission.
- [x] **P4.15** Destructive classifier (D32): deletes outside the project, force-push/reset, recursive remove, registry writes — each maps to a policy row with its own test — `agent::destructive` has a test per row (delete outside the root, force push, recursive remove, registry write, discarding uncommitted work) plus "ordinary commands have no findings" and stable unique ids.
- [x] **P4.16** Deliberation panel (D32): three read-only agents argue pro and con in one round with no tool access beyond read; the record attaches to the approval item, and the gate stays deterministic — it cannot be voted away — `agent::deliberation` sends one billed round with no tools, an unavailable advisor is recorded rather than fatal, and the note/positions are attached to both the allowed and the denied command item. `command::deliberation_runs_only_for_destructive_commands_and_is_recorded` pins that an ordinary command pays nothing for advice.
- [x] **P4.17** Pre-destructive snapshot (D32, D38): `git stash create` for tracked changes, untracked non-ignored files copied to `%APPDATA%\Rigel\snapshots\<turn>\` with a manifest, `rigel snapshot restore <id>`; non-git directories copy the whole tree under a 2 000-file / 200 MB cap and refuse above it. No disk image, no restore point — `snapshot.rs` implements both kinds with the caps as constants; `tests/snapshot.rs` restores tracked and untracked bytes exactly, ignores `.gitignore`d files and refuses above the cap.
- [x] **P4.18** **Exit test:** a destructive command triggers deliberation, snapshot and approval; deny leaves the tree byte-identical; approve records the snapshot id and the command text, and restoring the snapshot reproduces the pre-turn tree — `crates/rigel-core/tests/command.rs::a_destructive_command_is_gated_by_deliberation_a_snapshot_and_consent` runs `git reset --hard` against a real repository: the denial leaves the modified tracked file and the untracked file byte-identical, the advisors ran once with their note on the denied item, and no snapshot was taken; the approval runs the command, records the `git` snapshot id, and restoring it reproduces the pre-turn bytes. The test disables `core.autocrlf` so "byte-identical" means bytes.
- [x] **P4.19** Codex context budget (D29, N3) — `CodexConfig.skills_budget` (default 1 000; `0` sends `features.skip_host_skill_discovery=true`) goes out as `-c skills.max_context_tokens=…` on both transports and survives a direct connection. `rigel codex run` opens the newest open session (or starts one), registers a `codex` agent, marks that agent's stale running turns `interrupted`, and writes the prompt, normalised items, approvals, answer and a priced `Usage` row through `BudgetService`. `estimate_input` prints before the request and is returned in JSON: it adds the prompt to the last measured turn when there is one, so 155k is not estimated as 3k. Tests in `harness::session_tests` and `codex.rs` cover recording, the estimate basis, stale-turn interruption and isolation from another agent's running turn.

**Not yet:** claims enforcement (Phase 5), worktrees (Phase 5), durable shell transcripts, and any
egress outside an approved origin.

### Phase 5 — Claims, attribution, merged diff *(2 weeks)*

**Goal:** several agents on one tree, honestly coordinated.

- [x] **P5.1** Claim ledger: acquire, heartbeat, stale scan, 24 h expiry, never `DELETE` — `crates/rigel-core/src/claims.rs` on the Phase 1 `claims` table: paths are normalised and refused outside the root, a live claim held by another agent is returned as `held_by` (the schema's partial unique index means there is only ever one live row) and the collision is written to `claim_conflicts`; `heartbeat` refreshes every active row; `stale` is computed (90 s without a heartbeat) and reported, never auto-released; `expire` releases past 24 h with reason `expired`; `release` sets `released_at` and a reason and never deletes. Eight tests cover the advisory answer, idempotent claiming, the tombstone, expiry, staleness, unverified changes and path refusal.
- [x] **P5.2** MCP tools `rigel_claim`, `rigel_release`, `rigel_who` with the schemas from §7.1 — advertised and implemented in `crates/rigel-mcp/src/lib.rs`; `rigel_claim` returns `held`/`held_by` per path, `rigel_who` returns claims plus `unverified_changes`, and `rigel_release` is a tombstone. The tool list test pins that no advertised description contains the forbidden word.
- [x] **P5.3** Watcher plus attribution: reported changes first, observed changes second, `ambiguous` flag — `crates/rigel-core/src/attribute.rs` is a 250 ms poll (no new dependency): it stats the tree, hashes only files whose size or mtime moved, skips `.git`/`.rigel`/`target`/`node_modules`/build output, caps at 5 000 files and 1 MiB per hash, and reports deletions. Attribution names the single running agent, lists all of them and sets `ambiguous` when several were running. The desktop app runs a `ConflictWatcher` against the open project and records a row when a claimed path changes and the claimant was not the writer.
- [x] **P5.4** Conflict rows and the three-way diff view — conflicts are rows in `claim_conflicts` with claimant, writer, `base_hash` and `observed_hash`, listed by `ClaimService::conflicts`; the Diff view's strip shows both names, both hashes and a resolve action. **D79 replaced the hash-only deviation:** `claims.base_text` keeps the version the claimant started from, `conflict_sides` stores base/ours/theirs (each verified against its hash), and `ClaimService::merge` runs a line-level three-way merge. The strip expands to three panes; the merged text is editable and `conflict_resolve_merged` writes it through the CAS patch pipeline as a `user` patch, then records `merged_patch_id` on the conflict. A write that moved the file after capture is refused with `StaleBase` rather than overwritten. `crates/rigel-core/src/merge.rs`, `migrations/project/0010_conflict_sides.sql`, `tests/merge.rs`.
- [x] **P5.5** Worktrees and merge-back — `WorktreeService` creates a detached checkout under `%APPDATA%\Rigel\worktrees\<agent>`, records `agents.worktree`, previews the diff, and merges each changed file through the same CAS patch pipeline (a moved main tree is a conflict; a deletion is reported, not written). `rigel worktree create|list|preview|merge|remove` drives it, and `tests/worktree.rs` covers a clean merge, a moved main tree, new/deleted files, identical bytes, non-repositories and force-removal. **D80 removed the deviation:** `spawn_agents` accepts `mode: "write"`, registers a claim for every assigned file, gives the child its own worktree with the full tool set scoped to that file list, and returns the worktree and the changed files instead of the child's transcript. The child's changes land only through `merge_subagent`, which reads the diff, runs a clean-context reviewer, stores the verdict, and merges through the same CAS journal (`migrations/project/0011_writable_agents.sql`, `crates/rigel-core/src/safety.rs`, `tests/writable.rs`). Read-only children are unchanged and need no worktree.
- [x] **P5.6** Merged diff UI: per-hunk attribution gutter, conflict strip, per-patch revert — `migrations/project/0004_patch_hunks.sql` stores one row per hunk (header, ranges, patch, ordinal); `PatchPipeline::attributed_hunks` joins it to the patch's agent and status; `ui/src/diff.tsx` renders one gutter block per hunk with its own agent and status pill while keeping the conflict strip, claims list, unverified counts and per-patch revert; `patch_hunks` is a Tauri command. `tests/hunks.rs` pins two agents in one file, repeated edits, a reverted patch and header parsing.
- [x] **P5.7** CLI `claim list|release` — `rigel claim list [--path] [--history] [--json]` and `rigel claim release <path> [--agent] [--reason]` in `crates/rigel-cli/src/claims.rs`; releasing without `--agent` finds the current holder, and releasing an unheld path is a refusal with exit 1.
- [x] **P5.8** **Exit test:** two agents edit one file; the conflict surfaces within 2 s with both attributions; the UI never displays the word "locked" (asserted by a string test) — `scripts/p5-exit-test.sh` runs the ledger and attribution tests (including `a_conflict_surfaces_within_two_seconds_of_the_write`, which polls a real file write and asserts both claimant and writer on the row), the MCP claim round trip, a real `rigel claim list`/`release` against a temp project, and a word-boundary grep over `ui/src` plus the CLI output. It passes.

### Phase 6 — Dependencies and trust *(1.5 weeks)*

**Goal:** missing tools stop being a dead end without becoming a supply-chain hole.

- [x] **P6.1** Tool detection per host (`windows`, `wsl:<distro>`) with version cache in `tool_status` — `ToolService` probes a curated registry (`TOOLS`) per named host and caches version, path and SHA-256 in `tool_status`, so a shelf render spawns nothing; `rigel tools list` and `rigel tools which <tool> --json` read the cache, and Windows resolves through `PATHEXT` first — `crates/rigel-core/src/toolchain.rs` — `4fc76a5`
- [x] **P6.2** Requirement inference from `allowed-tools` plus a body scan, showing the source line — `requirements()` returns a `RequirementSource` with the file, line and text that asked for the tool, and `rigel tools requirements <skill-id>` prints it; a requirement is never reported without its source — `crates/rigel-core/tests/tools.rs`
- [x] **P6.3** Trust score from §11.6 weights, with `rigel trust explain <pkg>` printing the arithmetic — five factors, each with its reason, printed as arithmetic (`2 + 1 + 1 + 0 + 1 = 5 / 10`) rather than a bare number; a test pins the sums so a silent weight drift fails
- [x] **P6.4** Install execution: command shown first, ledger row, exit code and duration recorded — `rigel tools install` prints the manager's exact argv, asks, runs once, and writes the decision, exit code and duration; `rigel tools ledger` lists them and `--json` carries the same rows
- [x] **P6.5** Unattended mode queues installs instead of running them (D13) — `--unattended` writes a `queued` row and runs nothing; the p6 exit test proves the binary still does not exist afterwards
- [x] **P6.6** Shell restart after install, with the agent told to re-check (Q40) — `ToolService::restart` re-probes the host after an install and `InstallRecord::recheck()` is printed: "new shells see it; this one may not — re-check with `rigel tools which <tool>`". The agent is told what to do rather than assumed to know
- [x] **P6.7** Signature/hash check where the package manager exposes one — every detection records a SHA-256 pin and `verify` compares the installed bytes with it (`Pin::Missing`, `Pin::Moved` or a match); `rigel doctor` reports drifted pins, and apt is shown and refused rather than half-installed
- [x] **P6.8** **Exit test:** on a machine without `ripgrep`, the requirement shows its source line; the install runs only after approval; the new terminal sees the binary; with unattended on, the same action queues — `scripts/p6-exit-test.sh` builds a temporary PATH holding a shim `winget` and nothing else: the shim creates `rg` only when it runs, so seeing `rg` afterwards proves the approved install executed and seeing it absent proves the queue did not. It passes.

### Phase 7 — Hardening and open-source release *(1.5 weeks)*

- [x] **P7.1** `rigel doctor`: DB integrity, migrations, probes, orphan blobs, stale claims, budget reconcile, keychain, config drift — `crates/rigel-cli/src/doctor.rs` reports index and project integrity, schema versions, blob rows against blob files, stale claims, budget reconcile, the keychain, cached-config age, every harness, tool hash pins, unfinished installs and the update switch. One row per check with its own evidence; the exit code is the summary (`0` clear, `1` warnings, `2` failures) and nothing is mutated. `crates/rigel-cli/tests/doctor.rs` pins the exit codes, the evidence and the no-mutation rule — `4fc76a5`
- [x] **P7.2** Crash-recovery matrix: kill during patch apply, migration, install, checkpoint — each recovers cleanly — `crates/rigel-core/tests/recovery.rs` kills a migration (the version is unchanged), an install (the ledger keeps the decision and admits it never finished) and a checkpoint (resume closes the running turn); patch recovery is already driven through both `StopAt` windows in `guarantees.rs`, and the file says why a second copy would only test the copy
- [x] **P7.3** Docs: README rewritten around the workspace, CONTRIBUTING, SECURITY (from §10), ARCHITECTURE — README leads with the five surfaces; `CONTRIBUTING.md` gives build, checks, layout and the rule for a schema change; `SECURITY.md` is the threat model with every network call and what is not defended; `ARCHITECTURE.md` describes the pieces, the two databases and a turn
- [x] **P7.4** LICENSE chosen and applied — `MIT OR Apache-2.0`, in the workspace and every crate, with `LICENSE-MIT` and `LICENSE-APACHE` at the root; no contributor licence agreement and no telemetry to opt out of
- [ ] **P7.5** Signed Windows installer; WebView2 bootstrapper; minimum Windows 10 1809 — **partly:** `cargo tauri build` now produces both bundles on this machine (`Rigel_0.1.0_x64_en-US.msi`, 8.0 MiB; `Rigel_0.1.0_x64-setup.exe`, 5.4 MiB) from `crates/rigel-app/tauri.conf.json` (`msi` + `nsis`, `downloadBootstrapper` silent, downgrades refused). The build hook runs in `crates/`, not the config's directory, which is why it says `npm --prefix ../ui run build`; without that the UI never rebuilds. `scripts/sign-installers.ps1` now signs both layers, not just the wrappers: Tauri patches the app executable with bundle-type information after any external signing, so signing it before `cargo tauri build` leaves an invalid signature inside the MSI. The script passes the operator's thumbprint to Tauri with a temporary `--config`, so the patched executable (once per bundle type), the WiX helper DLLs and the MSI/NSIS wrappers are signed in one build; it then unpacks the MSI (`msiexec /a`) and verifies every installed PE against the wrapper's certificate. Verified 22 Oct with a throwaway self-signed certificate: `-Build` exit 0; `Rigel_0.1.0_x64_en-US.msi` and its embedded `rigel-app.exe` both validly signed and timestamped (`note ... chain untrusted`, as expected for a self-signed root); a byte appended to the MSI reports `HashMismatch` and exits 1; unsigned → `FAIL` (exit 1); no certificate → clean exit 2, including with `-Build` before anything is compiled. `scripts/p7-exit-test.sh` pins the exit codes, and `scripts/p7-vm-check.ps1` now checks the installed executable's signature against the installer's. **Still open:** a CA-issued or EV certificate and the resulting `ok trusted` lines; the test-signed copies were deleted afterwards and the bundles rebuilt unsigned, so nothing misleading is installable. 22 Oct: both stores were enumerated (`Get-ChildItem Cert:\CurrentUser\My,Cert:\LocalMachine\My -CodeSigningCert`) and hold no code-signing certificate at all — the throwaway certificate from the exercise was removed again — so this machine cannot produce the `ok trusted` line whatever is run on it. `docs/PLATFORMS.md` has the operator steps
- [x] **P7.6** macOS and Linux builds documented as unsupported-but-buildable, or marked `dropped` — `docs/PLATFORMS.md` records them as unsupported-but-buildable and names what is missing (no installer path, no keychain parity, no WebView2 equivalent)
- [x] **P7.7** Update manifest with an opt-out switch and signature check (N5) — `crates/rigel-core/src/update.rs` verifies an Ed25519 manifest over `version / url / sha256` (plus the optional MSI fields), then the artifact's hash; both switches (check and auto-install) are on by default and live in Settings → Updates (or `rigel update disable` from the CLI), and one signed check runs at launch
- [x] **P7.8** Migration note for anyone who used the prototype's localStorage index — `docs/migrating-prototype-index.md` plus `rigel skills import --from`; README's history note states that the `skilldb:` rows are historical and that nothing under `crates/` or `ui/` reads them
- [ ] **P7.9** **Exit test:** clean Windows VM, signed build, open a repo, run the shelf, run one native task, `rigel doctor` green, no Node install, no unexpected network calls — **the runnable half is done, the clean-machine half is not.** `scripts/p7-exit-test.sh` proves what one machine can: doctor's exit code matches its own summary, the CLI's commands run on a PATH holding only the OS shell, doctor/tools/the shelf make no network call behind a dead proxy, the installer config and licence expression match, and a signed manifest verifies while a tampered one is refused. 22 Oct, from an elevated prompt, `scripts/p7-vm-check.ps1` then ran **with** the install: `msiexec /i … /qn` exit 0, the payload at `C:\Program Files\Rigel\rigel-app.exe`, the window still alive after 20 s, one native turn and a shelf search behind a dead proxy, `msiexec /x` exit 0, and no file or uninstall entry left behind. The transcript and the fail-by-design rows (Node present, unsigned MSI) are in `docs/qa/p7-acceptance.md` and `docs/qa/records/`. Windows Sandbox is enabled on this host, but the enable is still waiting for a real boot (Fast Startup hid the last one; see the fifth-session note), so `scripts/p7-sandbox.ps1` runs the same acceptance inside it with the checkout mapped read-only once `WindowsSandbox.exe` exists — and `scripts/p7-queue-sandbox.ps1` is armed to do exactly that at the next sign-in, which is the clean machine this host can actually provide. **Still open:** a signed build (P7.5), the 1809 build floor, a first install on an image with no WebView2, SmartScreen, and a person looking at the window — none of which this machine can produce.

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
| UI | Playwright over the five surfaces' key states, in the browser preview | `ui/tests` |
| Manual | per-phase script in `docs/qa/<phase>.md`, run before ticking the exit test | — |

Non-negotiable tests, by name:

1. `patch_apply_is_cas` — stale base returns `STALE_BASE` and writes nothing.
2. `resume_never_duplicates_patches` — kill between journal write and rename; assert idempotence.
3. `claim_never_reports_locked` — scans the public copy (README and user-facing docs, `ui/src`, MCP descriptions) for a lock claim; a hit is allowed only in a prohibition (N1 as a test).
4. `budget_stops_within_one_turn` — fake adapter emits usage above cap; assert interrupt called once.
5. `memory_rejects_paths` — every pattern in §7.1.
6. `fixture_replay_covers_every_harness_and_scenario` — every harness and scenario the replay suite consumes has a fixture, and it is not empty; `crates/rigel-adapters/tests/replay.rs` runs the streams themselves.

As of 23 Oct 2026, `cargo test --workspace` covers 419 tests across the core, adapter, CLI and
app suites; `npx tsc --noEmit`, `npm --prefix ui run build` and the five-surface Playwright
smoke (`npm --prefix ui run test:e2e`) are part of the same gate.
The chat module's own tests pin the config parser, the harness argv (including the unattended
flags), the API-key readiness rule and the checkout guess; the spawn call itself is still
untested (§2.10, item 7).

All six are live tests: the two that were specifications (`P4.7`, `P5.8`) were implemented when
their surfaces landed. A failing test is fixed by fixing the cause. No suppressions, no `#[ignore]` above, no snapshot updates
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

### 15.6 Round 3, 21 Oct 2026 — D42–D51 (second grilling session)

| Q | Answer given | Decision |
|---|---|---|
| Session goal | Grill, then refine the plan (no code this session) | drives this section |
| Missing harnesses | Install Cursor + Cline now | done: Cursor 2026.10.19-6bbf2c4 (`%LOCALAPPDATA%\cursor-agent`), Cline 3.0.73 (npm global); probe reports all four `installed: true`; Cursor is `subscription` auth (refuses to spawn per D21 until a real `CURSOR_API_KEY`), Cline is `auth: none`/no provider |
| Next build | Nothing lands first after this; next code session = P4.7 golden fixtures first (fresh Cursor/Cline probes in fixtures) | D51 |
| Codex skill injection | Rigel-owned skills only per session | D46 |
| Budget | "No cap" instincts → opt-out with a confirm, unattended still requires a cap | D42 |
| Memory shape | One central file, per-agent sections, keeps building, no size limit | D43, context policy in D44/D45 |
| Deliberation models | Three cheap models (existing allowlist) argue pro/con | already D32 §15.2 wording; no change |
| Merge conflicts | Fail-closed to a conflict row; attribution shown; never silently reconciled | already §8.2 intent; no change |
| Cline sidecar | Dropped | D48 |
| 1.0 signing | Deferred until real users | D49 |
| Metric surface | Status bar always | D47 |
| Centrepiece | "3 agents branching out like i wanted" — multi-agent concurrent work confirmed as the product's spine | D50 |

**Restated, because it was fuzzy mid-session:** the main session always works in the **live tree**; **spawned agents get isolated worktrees** (D7) and merge back through the patch pipeline (P5.5). Two surfaces, one rule.

### 15.7 Round 4, 21 Oct 2026 — D52–D67 (20-question round; audit closed clean)

The audit question round: every line of the round-3 restatement confirmed correct as written — **no corrections**. The user's own addition this round, recorded above: **agents must know they are working alongside other agents** in a multi-agent session — settled as roster block + full transcript broadcast + the Rigel-write claim gate (D52/D53).

| Q | Answer given | Decision |
|---|---|---|
| Q26 awareness channel | Roster block in prompt | D52 |
| Q27 transcript sharing | Full broadcast (knowingly, costs visible in the pre-turn estimate) | D52 |
| Q28 claim etiquette | Gate on Rigel-performed writes; harness writes observed-only | D53 |
| Q29 coordination | Coordinator delegates | D54 |
| Q30 worktree home | `%APPDATA%\Rigel\worktrees` | D55 |
| Q31 execution env | Windows shell, WSL opt-in per agent | D56 |
| Q32 tool hosts | Merged best-guess view, per-host detail underneath | D57 |
| Q33 interactivity | Bounded wait, kill, record, tell the agent | D58 |
| Q34 mid-session skills | Frozen per session, everywhere | D59 |
| Q35 first-run shelf | Curated seed | D60 |
| Q36 skill call UX | Collapsed tool line | D61 |
| Q37 private skills | Reference, don't copy; deleted sources become `broken` | D62 |
| Q38 upload scope | Session-scoped, re-readable | D63 |
| Q39 upload caps | 100 MB, any type, summarized/labeled on ingest | D63 |
| Q40 uploads & merges | Opt-in adopt-as-content | D63 |
| Q41 phase order | Sequential as planned (4 → 5) | no change |
| Q42 public timing | At 1.0 | D65 |
| Q43 merge trigger | Free text: auto-merge, but deep cross-agent review so teammates pick up one another's mistakes; "i don't know" for the details → delegated | D64 |
| Q44 agent naming | Auto-generated, editable | D67 |
| Q45 idle mid-task | Notify, keep running | D66 |

### 15.8 Round 5, 21 Oct 2026 — D68–D70 (plain-language re-ask)

The user asked for the questions in plain language (no coding background). Re-asking the four picks with jargon removed changed one answer and confirmed three:

| Q | Plain-language result | Decision |
|---|---|---|
| 1 | "Everyone hears everything" was picked under jargon; on a plain re-explanation of the cost and distraction, the user switched to **summaries + on-demand reads** | D68 |
| 2 | Claim gate confirmed: app stops its own helpers, watches the external ones | D53 stands |
| 3 | Cross-agent review confirmed: auto-check, pause only when flagged, permanent review record | D64 stands |
| 4 | Money guardrail re-confirmed: cap on, turn-off allowed with warning, unattended always capped | D42 stands |
| 5 | First real task: **work on several things in parallel** — the centrepiece (D50) restated in the user's own words | D50 stands |
| 6 | Accounts on hand: **OpenAI + Google (Gemini)**; wants a **small local model on this PC** joining the team, slowness explicitly accepted | D69 |
| 7 | Someday: check on agent work remotely / from a phone | D70 |
| 8 | One combined helper-program list is fine; details show which "world" a program lives in | D57 stands |

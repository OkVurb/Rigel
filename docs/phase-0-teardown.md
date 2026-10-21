# Phase 0 — Competitor teardown, complaint mining, gap analysis

**Date:** 20 October 2026
**Scope:** Codex, Claude Code (CLI + desktop), Freebuff, Cursor, Cline (+ Roo Code). Public UX and open-source components only; no bundle inspection.
**Method:** primary docs and changelogs, GitHub issue trackers, Hacker News threads, Product Hunt, pricing pages. Reddit was reached via search-result snippets only, because Reddit blocks direct scraping (Firecrawl's policy refuses the domain outright).
**Confidence tags:** ✅ verified from a primary source · ⚠️ single user report, not prevalence data · ❓ unverified / could not confirm

---

## 1. Executive summary

1. **The feature checklist is now table stakes.** Parallel agents, worktrees, diff review with line comments, terminal, MCP, skills, session resume, and PR creation exist in *at least four* of the five products. Nothing on that list is a wedge any more.
2. **The real, repeated, cross-product pain is not missing features — it is unaccountable agent behaviour.** Users cannot tell (a) what a turn will cost before they run it, (b) whether the agent actually finished what it claimed, (c) which skill ran and why, or (d) how much of a limit remains. Every product has at least one open issue or thread on each.
3. **Skills discovery is the one area where the incumbents are genuinely weak, and they know it.** Claude ships skills as files with no browse-and-install path for plain skills; Codex has a `$skill-installer`; Freebuff's answer to "how do I install a skill?" was a GitHub issue resolved by *asking the agent to do it*. Meanwhile a third-party directory (skills.browseract.com) has aggregated 16,494 skills and 239M installs — demand is proven, the install UX is not.
4. **The catalogue is commoditized; the install/enable/verify layer is not.** An app that owns "install this skill once, works in Claude Code, Codex, Cursor and Cline, and I can see exactly what it will touch" is doing something none of them do.
5. **Cost opacity is the single most emotional complaint across all five.** Quotas tightened in 2025–26 at Cursor, Claude, Codex and Freebuff, and every one of them produced a public backlash. Predictability, not price, is the unserved need.

---

## 2. Feature matrix

✅ full · ⚠️ partial / conditional · ❌ absent · ❓ unverified

| Capability | Claude Code | Codex | Cursor | Cline | Freebuff | **Target app** |
|---|:--:|:--:|:--:|:--:|:--:|:--:|
| Chat + file edits | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Diff review, per-hunk | ✅ | ✅ | ✅ | ✅ | ⚠️ | ✅ |
| Diff comments sent back to agent | ✅ | ✅ | ⚠️ | ❌ | ❓ | ✅ |
| Integrated terminal | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Approval modes (ask / auto / bypass / plan) | ✅ | ✅ | ⚠️ | ✅ | ⚠️ | ✅ |
| Session-scoped approval (not just once/permanent) | ❌ | ❓ | ❓ | ❌ | ❓ | **✅** |
| Parallel sessions | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| Git worktree isolation | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| Built-in git stage/commit/PR | ✅ | ✅ | ✅ | ⚠️ | ⚠️ | ✅ |
| MCP support | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| MCP in-app marketplace | ❌ | ⚠️ | ⚠️ | ✅ | ❌ | **✅** |
| Agent Skills (`SKILL.md`) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| **In-app browse-install of skills** | ❌ | ⚠️ (`$skill-installer`) | ❌ | ✅ (Desktop) | ❌ | **✅ (wedge)** |
| Installs to *other* tools' skill dirs | ❌ | ❌ | ❌ | ⚠️ | ❌ | **✅** |
| Pre-install inspection of what a skill touches | ❌ | ❌ | ❌ | ⚠️ | ❌ | **✅** |
| Explains why a skill did/didn't trigger | ❌ | ❌ | ❌ | ❌ | ❌ | **✅** |
| Live per-session cost/token meter | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ | **✅** |
| Verified completion (checks run before "done") | ❌ | ❌ | ⚠️ | ❌ | ⚠️ | **✅** |
| Subagents with separate context | ✅ | ✅ | ✅ | ⚠️ | ✅ | ✅ |
| Hooks / lifecycle events | ✅ | ⚠️ | ⚠️ | ❌ | ❓ | ⚠️ |
| Session resume | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Continuity-preserving compaction | ⚠️ | ❓ | ❓ | ⚠️ | ❓ | **✅** |
| BYO API keys | ❌ | ⚠️ | ⚠️ | ✅ | ❌ | ✅ |
| Zero telemetry option | ❌ | ❌ | ⚠️ | ✅ | ❌ | ✅ |

**Reading the matrix:** we are not out-featuring them. We are filling four empty rows (`in-app skill install`, `cross-tool install`, `pre-install inspection`, `why didn't it trigger`) and two weak ones (`cost meter`, `verified completion`).

---

## 3. Where the on-disk skill layouts actually live

This matters because "install once, works everywhere" is only possible if we write to the right paths.

| Tool | Global skills | Project skills | Plugin / package manifest |
|---|---|---|---|
| Claude Code | `~/.claude/skills/<name>/SKILL.md` | `.claude/skills/<name>/` | `.claude-plugin/plugin.json`; cache at `~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/`; registry `~/.claude/plugins/installed_plugins.json` |
| Codex | `~/.agents/skills/` (+ `/etc/codex/skills`) | `.agents/skills/` scanned from cwd up to repo root | config `~/.codex/config.toml`; MCP via `codex mcp add` |
| Cursor | `~/.agents/skills/` or `~/.cursor/skills/` | `.agents/skills/` or `.cursor/skills/` | MCP `~/.cursor/mcp.json`, `.cursor/mcp.json`; rules `.cursor/rules/*.mdc` |
| Cline | `~/.cline/skills/` (+ reads `~/.claude/skills/`) | `.cline/skills/`, `.claude/skills/` | MCP `~/.cline/data/settings/cline_mcp_settings.json` |
| Freebuff | `~/.agents/skills/`, `~/.claude/skills/` | `.agents/skills/`, `.claude/skills/` | MCP `.agents/mcp.json` |
| **Target app** | own canonical library | enablement entries only | one generic package manifest |

**Key finding:** `.agents/skills/` is emerging as the cross-tool location, and four of six tools read it. A single canonical store that materializes into `.agents/skills/` plus per-tool links covers almost the entire market.

**Agent Skills spec essentials:** directory containing `SKILL.md` with required frontmatter `name` (1–64 chars, lowercase alphanumeric + hyphens, must match directory) and `description` (1–1024 chars, states what + when to use). Optional `license`, `compatibility`, `metadata`, experimental `allowed-tools`. Progressive disclosure: metadata always loaded, body on activation, bundled files on demand. Adopters confirmed: Claude, Codex, Cursor, Cline, Roo, GitHub Copilot, VS Code.

---

## 4. Per-competitor: strengths and mined complaints

### 4.1 Claude Code (CLI + Claude Desktop "Code" tab)

**Strongest at:** the most complete *supervision* loop. Desktop arranges chat / diff / browser / terminal / file / plan / tasks panes, runs local, SSH, WSL or cloud, and lets the agent verify its own work in a real browser. Hooks give deterministic lifecycle control that no other product matches. Subagents isolate context properly.

**Complaints (all ⚠️ individual reports):**

- **Quota burn.** *"I burn through the whole damn quota in like ONE OR TWO DAYS"* and *"6 days of production a month is not worth your subscription."* — [issue #9424](https://github.com/anthropics/claude-code/issues/9424). A Max user asked for *"clear, real-time visibility of remaining weekly capacity"* — [#76006](https://github.com/anthropics/claude-code/issues/76006). Anthropic's counterpoint: limits unchanged, Opus 4.5 just runs longer ([#16157](https://github.com/anthropics/claude-code/issues/16157)).
- **No session-scoped approval.** *"There's no middle ground for users who want to run autonomous workflows within a single session without granting permanent tool access."* — [#17079](https://github.com/anthropics/claude-code/issues/17079), repeated for Desktop in [#48479](https://github.com/anthropics/claude-code/issues/48479).
- **Skills fail silently.** *"Way more often than not, i remind the agent that the skill exists before it does anything."* and the ecosystem's *"silent failure"* problem when a skill isn't selected — [HN](https://news.ycombinator.com/item?id=46871173).
- **Compaction loses the thread.** Default summary drops the next action, settled decisions, failed approaches and project rules; closed as *not planned* — [#13642](https://github.com/anthropics/claude-code/issues/13642). (`/autocompact` has since shipped, so the threshold part is addressed.)
- **Oversight vs. noise.** *"I want Claude to tell me what it's reading in the first 3 seconds"* after output was condensed — [HN](https://news.ycombinator.com/item?id=46978710).
- **Desktop feels heavy.** Conversations taking *"a full second to load"* — [HN](https://news.ycombinator.com/item?id=47104973).

**Pricing:** Pro $20/mo, Max 5× $100, Max 20× $200; five-hour and weekly limits shared across all surfaces.

### 4.2 Codex (CLI + app + IDE extension)

**Strongest at:** being a supervised *command center*. Parallel threads, isolated worktrees, handoff between worktree and local checkout, diff review with inline comments and chunk-level staging, and GitHub PR review scoped to P0/P1 findings. The OS-enforced sandbox with network off by default is the strongest default-secure posture of the five.

**Complaints:**

- **Quota burn.** *"within 2 hours of working I managed to burn through ~20% of my tokens"* — [#14593](https://github.com/openai/codex/issues/14593).
- **Silent non-completion.** *"It'll say it's doing something and it just stops, doesn't do it."* — [#6439](https://github.com/openai/codex/issues/6439).
- **Model routing breaks reproducibility.** *"Silently swapping models breaks reproducibility…"* — [#11189](https://github.com/openai/codex/issues/11189).
- **Latency.** *"Having to wait for an undefined amount of time before getting a result is definitely not the best."* — [HN](https://news.ycombinator.com/item?id=44042070).
- **CLI feel.** *"Codex CLI is way less pleasant to use than Opus."* — [HN](https://news.ycombinator.com/item?id=46584247).

**Skills:** directories with `SKILL.md` at `.agents/skills`, `~/.agents/skills`, `/etc/codex/skills`; install with `$skill-installer <name>`, invoke with `$skill-name` or `/skills`. **Pricing:** Plus $20/mo, Pro from $100/mo, Business $20/user/mo.

### 4.3 Cursor

**Strongest at:** the tightest editor loop. Tab completion plus next-edit prediction, `Cmd+K` inline edit, agent multi-file work with reviewable diffs, and cloud agents in isolated VMs are a genuinely better *in-editor* experience than a separate app can offer. Do not compete here — compete in the manager and supervision layers.

**Complaints:**

- **Usage burn.** *"I'm a heavy coder and end up burning through two Cursor Pro accounts every week."* and *"Cursor has become a king in burning tokens unnecessarily…"* — [Cursor forum](https://forum.cursor.com/t/think-cursor-pro-is-20-month-nope-it-s-20-per-week-or-even-less-it-runs-out-faster/136405).
- **The June 2025 pricing change.** Pro moved from 500 requests to a **$20 pool billed at API rates**; Cursor publicly apologised for unclear communication and refunded unexpected charges from 16 June–4 July 2025 ([blog](https://cursor.com/blog/june-2025-pricing)). *"$20 on API pricing is what Claude Pro will give you in a day… this is a massive limitation"* — [HN](https://news.ycombinator.com/item?id=44470148). *"I spend so much time inside cursor, that I am spending 10-20$ per day on additional requests"* — [HN](https://news.ycombinator.com/item?id=44185256).
- "From unlimited to unusable" was the title of a widely-shared r/cursor thread (⚠️ snippet only; page not fetchable).
- **Current tiers** (❓ moves fast): Free, Plus $8, Premium $30, Super $100, Ultra $200, weekly pools.

### 4.4 Cline / Roo Code

**Strongest at:** transparency and price. Cline is free with BYOK, Plan/Act separation, shadow-git checkpoints with file-or-history restore, and an actual MCP marketplace in-app. Roo's custom tool-scoped modes and orchestrator are the most opinionated approach to constraining an agent.

**Important:** ⚠️ Roo Code's repository states the extension **shut down on 15 May 2026**. Study it as a design source, not a live competitor.

**Complaints:**

- **Token waste in search.** *"In a short session I can use 4-5m tokens, with almost 200k token per request, for <1000 tokens in return."* — [cline#2110](https://github.com/cline/cline/issues/2110). *"A very simple code location task can end up consuming an enormous amount of tokens."* — [cline#8336](https://github.com/cline/cline/issues/8336).
- **Context destruction.** *"If the model reads a file which is too large, that data will mess up the instructions and previous context causing it to lose track of what is was doing completely."* — [roo#1532](https://github.com/RooCodeInc/Roo-Code/issues/1532).
- **MCP drops mid-task.** *"On a regular basis when Roo is mid-task… it will suddenly lose MCP connection and either error out or the coder will fail at completing the task."* — [roo#1856](https://github.com/RooCodeInc/Roo-Code/issues/1856).
- **False completion.** *"Roo Code reports that the task was completed successfully"* while the terminal shows an error — [roo#2042](https://github.com/RooCodeInc/Roo-Code/issues/2042).
- **Cost per request.** *"RooCode can code 0.5 to 1 USD per request."* — [HN](https://news.ycombinator.com/item?id=43642212).

**Pricing:** Cline agent free; BYOK, pay-as-you-go credits, or ClinePass $9.99/mo.

### 4.5 Freebuff (the closest product to what we're building)

**Strongest at:** zero-config entry. No API key needed to start, ad-funded $0 tier, browser-style tabs with one agent thread per project, worktree isolation per thread, and it can drive locally-installed Claude Code or Codex using the user's own accounts.

**Complaints — and the most important signal in this document:**

- **Skill install is a prompt, not a product.** [Issue #1041](https://github.com/CodebuffAI/freebuff/issues/1041) asked for a skill-install command; it was closed as solved because the user could *ask the agent to install it from a GitHub URL*. That is the gap we are building into.
- **Usage caps moving under users.** Complaints about daily caps and usage consumed by short/failed runs — [#1280](https://github.com/CodebuffAI/freebuff/issues/1280). Current plans: Free 30 usage-hours/day, Starter $8, Plus $25, Pro $60.
- **Windows reliability.** Indefinite loading UI across several versions — [#1136](https://github.com/CodebuffAI/freebuff/issues/1136); crashes, server errors, slow tab switching — [#1321](https://github.com/CodebuffAI/freebuff/issues/1321).
- **Extensibility.** A user asked for a real plugin/extension model and criticised autonomy and subagent limits — [#1039](https://github.com/CodebuffAI/freebuff/issues/1039).

**This is our direct competitor and our best evidence:** its own users are asking for the feature we are leading with.

---

## 5. Complaint clusters (what actually hurts)

| Cluster | Products affected | Root cause | Our answer |
|---|---|---|---|
| **Cost unpredictability** | All 5 | Limits opaque, shared across surfaces, no pre-flight estimate | Live meter + cap + handoff to own key |
| **Unverified "done"** | Codex, Roo, Cline, Claude | Agent self-reports success | Completion requires passing check output |
| **Skills silently don't fire** | Claude (explicit), all | Model decides from description alone | Trigger debugger + deterministic `@skill` pin |
| **Approval fatigue vs. over-grant** | Claude (explicit), Cursor, Cline | Only "once" or "always" | Session / project / org scopes + plan approval |
| **Context loss on compaction** | Claude, Roo | Narrative summary discards decisions | Structured handoff artefact |
| **Token burn in code search** | Cline, Roo | Full files into context | Repo map + symbol index, retrieval instead of dumps |
| **MCP flakiness** | Roo (explicit), all | Long-lived subprocess, no health model | Health monitor + one-click repair |
| **Polish/perf** | Freebuff, Claude Desktop | Electron/UI debt | Tauri + measured budgets |

---

## 6. Ranked gap analysis — the ten QoL features worth building

Scored 1–5. **Priority = impact ÷ effort.** These are ordered by that ratio, not by how exciting they are.

| # | Feature | Impact | Effort | Why it wins | Nobody has it |
|---|---|:--:|:--:|---|:--:|
| 1 | **Skill package manager** — GitHub search + paste URL, read `SKILL.md` rendered, file tree, requirements, one-click install into a canonical library, then materialize into `.agents/skills` + per-tool paths | 5 | 3 | The origin idea; Freebuff #1041 proves demand; directory layer already exists (16.5K skills) so only the install UX is unowned | ✅ |
| 2 | **Verified completion** — every "done" gated on the project's check command; show the command, exit code and output inline | 5 | 2 | Cheapest fix for the most damaging trust complaint (Codex #6439, Roo #2042) | ✅ |
| 3 | **Session-scoped approvals** — once / this session / this project / always, plus batched approval of a proposed action plan | 5 | 2 | Directly requested twice in Claude's tracker; a one-line model change from "once" | ✅ |
| 4 | **Why-didn't-my-skill-fire panel** — per turn, list skills considered, activated, and the reason for rejection; allow pinning a skill to force it | 4 | 3 | The HN quote is the whole product argument; deterministic pinning solves it outright | ✅ |
| 5 | **Live cost meter with pre-flight estimate** — tokens/$ per session and per subagent, hard cap, "continue on your API key" | 5 | 3 | Highest-emotion cluster across all five products | ⚠️ partial |
| 6 | **Pre-install inspection** — show exactly what a skill will read, write, execute and network; diff against an existing install | 4 | 3 | Skills run scripts; the third-party directory already advertises "security audited", so audits are expected | ✅ |
| 7 | **Structured compaction handoff** — persist decisions, failed approaches, project rules and next action as a file, not a summary | 4 | 3 | Claude #13642 was closed *not planned* — an open wound | ✅ |
| 8 | **Repo map + retrieval search** — index once, give the agent symbols and windows instead of whole files | 5 | 4 | Cline/Roo token-burn complaints are all search-shaped | ⚠️ partial |
| 9 | **MCP health monitor + repair** — per-server status, restart, tool-level enable/disable | 3 | 2 | Roo #1856 is a recurring, cheap-to-fix failure | ⚠️ partial |
| 10 | **Cross-tool interop** — one install, visible in Claude Code / Codex / Cursor / Cline | 4 | 1 | Nearly free once #1 exists; makes the app valuable to people who never use its agent | ✅ |
| 11 | **Clone detection + canonical pick** — cluster near-duplicates, show the best maintained, disclose the rest | 4 | 3 | GitHub is full of forks and reformats; nobody surfaces it. Spec: [skill-intelligence.md](skill-intelligence.md) | ✅ |
| 12 | **Evidence-first ranking** — measured vs. advertised savings, "unsubstantiated" tags | 4 | 2 | JetBrains measured three token-saver skills: −8.5%, +7.6% (worse) and −15% against −65%, −60–90% and −54% advertised. Highest credibility per hour in the build | ✅ |

**Seed catalogue (first run):** two packages are specified to ship curated — **ECC** (`affaan-m/ECC`, a 311-skill bundle with agents, hooks and security tooling; 1.2M skills.sh installs) and **Ponytail** (a token-saving skill; advertised −54% code, measured −15%). Details and the full scan/safety/dedup design are in [skill-intelligence.md](skill-intelligence.md). ECC also validates the bundle model and is a partial competitor: it installs across six harnesses from a CLI, with no browsable catalogue, no per-project enablement and no duplicate handling.

**Deliberately excluded from v1:** publishing/uploading packages, our own marketplace backend, accounts/social, ratings, team collaboration.

**On your reference links:**
- `skills.browseract.com` — 16,494 skills / 239M installs aggregated from Skills.sh + BrowserAct, filtered by category, target site and security audit. **Treat this as evidence and a possible seed catalogue, not as a competitor to clone.** It is a website; it cannot *enable* a skill in a project.
- `freepublicapis.com` (812 APIs, health-tested daily), `public-apis/public-apis`, `free-apis.github.io` — genuine value is as a **connector catalogue** for MCP integrations later, not core. Most entries are toys; licences and rate limits vary wildly. Do not put this in v1.

---

## 7. Positioning

```
                        Deep supervision
                              │
            Claude Code ●     │     ● Codex
                    (panes)   │   (worktrees, PR review)
                              │
        In-editor ────────────┼──────────── Standalone app
                              │
              Cursor ●        │        ● Freebuff  (parallel tabs)
                              │        ★ Us (own the package layer)
                              │
                        Light supervision
```

The axis that matters is **where the product spends its effort**: Cursor and Claude invest in the *agent loop*; Codex and Freebuff invest in *parallelism*; nobody invests in **the layer between the model and the ecosystem** — installing, inspecting, enabling and explaining the things that extend it.

**One-sentence position:** *the app that makes your agent's capabilities installable, attributable and affordable.*

---

## 8. So what — implications for the build

1. **Do not out-feature them.** Match chat/edits/diffs/terminal/worktrees and stop. Every extra parity feature is a month not spent on rows 1–5 of the gap table.
2. **Lead with the package manager, but ship #2 (verified completion) in the same milestone.** It is two days of work and it is the complaint every user of every competitor has felt.
3. **`.agents/skills/` is the interop bet.** Four of six tools read it. Canonical store + materialization gives us "works everywhere" for near-zero marginal cost.
4. **Cost transparency is a feature, not billing UI.** It needs to be on the same screen as the agent, always visible, with a hard stop.
5. **Your original instinct was half right.** Speed is not the differentiator (Tauri gives it to us cheaply); *accountability* is. Speed is the floor — keep the three measured budgets from the spec.

---

## 9. Limits of this research

- **Reddit is not directly readable.** Firecrawl's policy refuses the domain; all Reddit material here is search-result snippets and is tagged ⚠️.
- **No hands-on session with any product.** This is documentation + public reports, not measured UX. Screenshots and time-to-first-action still need doing.
- **Pricing is a moving target.** Cursor's tiers and Freebuff's hours both changed within the last year; treat all prices as a snapshot dated 20 Oct 2026.
- **Architecture notes are partial.** Permission models, plugin cache layout and checkpoint mechanics are documented above; agent-loop internals and context-compaction implementations are *not* reverse-engineered beyond what docs state (`❓` where relevant).
- **Roo Code is shut down**, so complaint data is historical.

---

## 10. What Phase 0 still owes

| Deliverable | Status |
|---|---|
| (a) Feature + UX teardown | ✅ this document |
| (b) Architecture notes | ⚠️ partial — permission/plugin/checkpoint layouts only |
| (c) Ranked gap analysis | ✅ §6 |
| (d) Clickable prototype (package browser, install flow, chat + diff, session switcher) | ❌ not started |

**Recommended next:** build (d) — the prototype — before any production code. The install flow is the whole product in miniature, and it is the cheapest thing to get wrong expensively.

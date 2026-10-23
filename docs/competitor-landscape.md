# Competitor landscape — features, 22 Oct 2026

Research pass for round 4 of the grilling. Three questions: what do multi-agent orchestrators ship,
what do coding IDEs and CLIs ship, and what does the skills/MCP ecosystem ship. Sources are linked per
claim; prices and counts are what the vendor pages said on the day, not audited numbers.

**Read this first:** the category is consolidating. Vibe Kanban's repo now says it is sunsetting;
Stravu's Crystal was deprecated in February 2026 and replaced by Nimbalyst; Roo Code's repository was
scheduled to archive in May 2026; Terragon's hosted service is shut down and its repo is a snapshot.
Copy features from live products, not from the ones with the best screenshots in old blog posts.

---

## 1. Multi-agent orchestrators

| Product | Form | Worktrees | Board | Diff review | Merge / conflicts | Approvals | Cost | Remote | Notes |
|---|---|---|---|---|---|---|---|---|---|
| [Vibe Kanban](https://github.com/BloopAI/vibe-kanban) | local web UI via npx | ✓ | ✓ | ✓ inline comments | ✓ PR + merge | ✓ profiles | — | cloud mode | sunsetting |
| [Conductor](https://www.conductor.build/docs) | Mac desktop | ✓ | — | ✓ comments | ✓ PR/merge | ◐ | ◐ | Pro $50/mo | cloud + mobile in Pro |
| [crystl.dev](https://crystl.dev/) | Mac terminal | ✓ | — | ◐ | ✓ | ✓ floating cards | ✓ per-shard tokens | ✓ iPhone | free + Guild $18/mo |
| [Claude Squad](https://github.com/smtg-ai/claude-squad) | terminal TUI | ✓ | — | ✓ preview | ◐ manual | ◐ | — | — | tmux |
| [Agent Deck](https://github.com/asheshgoplani/agent-deck) | CLI/TUI + web | ◐ optional | — | ◐ | ✓ finish | ◐ | ✓ | ✓ | skill pool, MCP manager |
| [Sculptor](https://imbue.com/product/sculptor) | desktop (Mac/Linux) | ✓ | — | ✓ | ✓ | ? | ◐ | — | no Windows |
| [Superset](https://superset.sh/) | desktop + CLI | ✓ | ◐ | ✓ | ✓ | ◐ | ◐ | ✓ Pro | no Windows |
| [Uzi](https://github.com/devflowinc/uzi) | Go CLI | ✓ | — | ◐ | ✓ rebase | ◐ auto | — | — | tmux, ports |
| [workmux](https://workmux.raine.dev/guide/) | CLI | ✓ | — | ◐ | ✓ | — | — | — | tmux |
| [Container Use](https://github.com/dagger/container-use) | CLI/MCP | ✓ containers | — | — | git | ◐ | — | — | isolation, not UX |
| [Omnara](https://github.com/omnara-ai/omnara) | web/CLI | ◐ | — | — | ◐ | ✓ | ✓ | ✓ | durable agents, not worktrees |
| [Terragon](https://github.com/terragon-labs/terragon-oss) | hosted | ◐ | ◐ | ◐ | PR handoff | ◐ | — | ✓ | shut down |
| [Crystal (Stravu)](https://github.com/stravu/crystal) | desktop | ✓ | ? | ? | ? | ? | ? | — | deprecated |

**What this tells you.** Worktrees, a status view and *some* path to review changes are table stakes.
A board tied to execution is rare (Vibe Kanban). Cross-vendor approval cards are rare (crystl.dev).
Per-shard token display exists but hard enforcement is rarer still. **Nobody in this list advertises
advisory file claims or an author-attributed merged diff** — the two things Rigel's Phase 5 was built
around.

**Also rare:** native Windows. Conductor, crystl.dev and Sculptor are Mac/Linux; Superset says Windows
is unavailable; Agent Deck reaches Windows through WSL. Rigel's Windows-first desktop position is
genuinely under-occupied.

---

## 2. Coding IDEs and CLIs

| Product | Parallel / background | Checkpoints | Rules / memory | MCP | Permissions | Cost controls | Extras |
|---|---|---|---|---|---|---|---|
| [Cursor](https://cursor.com/docs) | 8 parallel, cloud VMs | ◐ | ✓ | ✓ + skills | ✓ sandbox + hooks | spend limit, plan usage | browser, plan mode |
| [Windsurf/Cascade](https://docs.windsurf.com/windsurf/cascade) | multiple Cascades | ✓ named checkpoints | ✓ memories | ✓ | ✓ | ◐ prompt credits | 40 tool calls/prompt |
| [Zed](https://zed.dev/docs/ai/agent-panel) | threads + worktrees | ✓ restore checkpoint | ✓ | ✓ forwarded over ACP | ✓ per-tool | token use (native) | hosts Claude/Codex/Gemini via ACP |
| [Cline](https://docs.cline.bot/features/subagents) | subagents (CLI/Desktop) | ✓ | ✓ | ✓ | Plan/Act | session cost | subagents get no MCP |
| [Kilo Code](https://kilo.ai/articles/roo-to-kilo-migration-guide) | Agent Manager + worktrees | ✓ per message | ✓ AGENTS.md | ✓ | allow/ask/deny | gateway pricing | marketplace, local models |
| [Claude Code](https://code.claude.com/docs/en/costs) | subagents, cloud sessions | ◐ | ✓ CLAUDE.md | ✓ | ✓ | `--max-budget-usd`, `/usage` | skills, hooks |
| [Gemini CLI](https://geminicli.com/docs/cli/skills/) | plan subagents | — | ✓ GEMINI.md | ✓ | ✓ | quotas, `/stats model` | skills from git |
| [Codex CLI](https://learn.chatgpt.com/docs/codex/cli) | workspace agents (beta) | ? | ✓ | ✓ | ✓ | ? | plugin browser |
| [Aider](https://aider.chat/docs/usage.html) | — | ✓ `/undo` | ✓ | — | — | — | any model incl. local |
| [OpenHands](https://docs.openhands.dev/overview/introduction) | cloud + local | ? | ✓ | ✓ | ✓ analyzer | budget mgmt (cloud) | collaboration |
| [Kiro](https://kiro.dev/powers/) | ? | ? | steering | ✓ | hooks | — | Powers catalog |
| [Trae](https://docs.trae.ai/ide/agent) | ? | ? | ? | ? | ? | ? | docs unreadable |

**Commodity now:** multi-file edits, a persistent rules file, MCP, a plan mode, and *some* approval
surface. **Still differentiating:** true concurrent execution with isolation; one panel hosting several
vendors' agents (Zed via ACP — but each keeps its own auth and bill); local models (Aider, Kilo, Zed);
hard budget caps (`--max-budget-usd` is the clearest shipped example); curated catalogs (Kilo, Kiro,
Codex plugins).

**Marketing to distrust:** "multi-agent" covers cloud runs, worktree sessions, *and* read-only research
subagents. "Budget controls" usually means a dashboard or a provider limit, not a per-task ceiling.
"Marketplace" usually means a catalog, not a reviewed artifact.

---

## 3. Skills and MCP ecosystem

| Project | What it is | Claimed size | Install | Scans | Pinning |
|---|---|---|---|---|---|
| [skills.sh](https://www.skills.sh/) / `npx skills` | Vercel directory + CLI | 60k+ audited | `npx skills add` | ✓ Gen/Socket/Snyk badges | refs, no lockfile |
| [SkillsMP](https://skillsmp.com/) | GitHub SKILL.md aggregator | 3,000,000+ | copy/clone | — | — |
| [SkillHub.club](https://www.skillhub.club/) | hosted marketplace | 154.5K | npx CLI, desktop | AI signals, gated packages | creator versions |
| [Claude plugins](https://code.claude.com/docs/en/plugins/overview) | marketplace manifests | — | `/plugin` | guidance only | git refs |
| [Official MCP Registry](https://modelcontextprotocol.io/registry/about) | metadata index | — | API, not an installer | namespace only | package versions |
| [Smithery](https://smithery.ai/) | MCP directory + CLI | thousands | `smithery mcp add` | MCP-Scan integration | package versions |
| [Glama](https://glama.ai/) | MCP directory | 108,738 servers | config copy, gateway | builds/runs in microVMs | git history |
| [mcp.so](https://mcp.so/) | community directory | 1,000+ | copy config | — | upstream |

**The ecosystem's own complaints**, in its issue trackers: skills have no dependency or lockfile
standard ([agentskills#100](https://github.com/agentskills/agentskills/issues/100),
[manifest RFC](https://github.com/agentskills/agentskills/discussions/210)); MCP installation is
copy-paste JSON per client
([MCP discussion #1590](https://github.com/modelcontextprotocol/modelcontextprotocol/discussions/1590));
registry safety evidence is unscanned or unversioned
([registry#1273](https://github.com/modelcontextprotocol/registry/issues/1273)). That is exactly the
gap a local index with dependencies, pins and trust arithmetic sits in.

---

## 4. Where Rigel already leads

1. **Hard per-agent budgets** (D10/D42) — the closest competitor is Claude Code's `--max-budget-usd`.
2. **Advisory claims plus conflict detection** (D5, P5.1–P5.4) — nobody advertises this.
3. **An attributed merged diff** (D16, P5.6) — branch diffs are common, author-per-hunk is not.
4. **Trust arithmetic for skills** (P6.3) — registries show popularity or a scan badge, not arithmetic.
5. **Dependency detection with the source line** (P6.2) — the skills format has no dependency story at all.
6. **Windows-native desktop** — the category is overwhelmingly Mac.

## 5. Gaps, ranked by value ÷ effort

| # | Gap | Who has it | Effort | Verdict |
|---|---|---|---|---|
| 1 | Inline comments on diff hunks, routed to the author agent | Conductor, Vibe Kanban, Cursor | S | build |
| 2 | Per-turn checkpoint and restore (not only pre-destructive) | Windsurf, Zed, Kilo | S | build |
| 3 | Desktop notification on approval / finish / failure | crystl, Superset | XS | build |
| 4 | Rules interop: read `AGENTS.md`/`CLAUDE.md`, export Rigel memory | everyone | S | build |
| 5 | Cross-session spend dashboard and per-task caps | Claude Code, Agent Deck | S | build |
| 6 | skills.sh import with its audit badge | skills.sh | S | build |
| 7 | Agent role presets (reviewer, tester, implementer) | Vibe Kanban, crystl | S | build |
| 8 | Session export as a readable bundle | — | XS | build |
| 9 | Shelf search as an MCP tool for any agent | — | XS | build |
| 10 | PR creation from the Diff tab via `gh` | Vibe Kanban, Conductor | M | build |
| 11 | Plan mode with a reviewable plan artifact | Cline, Gemini, Cursor | M | build |
| 12 | Task board: tasks bound to agent + worktree | Vibe Kanban, Kilo | L | after 0.1 |
| 13 | MCP catalog with trust cards | Kilo, Roo, Glama | L | after 0.1 |
| 14 | Lockfile export/import for skills | nobody (RFC only) | M | after 0.1 |
| 15 | Mobile read-only view over the LAN room | crystl, Superset Pro | L | after 0.1 |
| 16 | Remote approvals from a phone | crystl | M | after 0.1 |
| 17 | Issue import (GitHub/Linear) | Vibe Kanban | M | after 0.1 |
| 18 | Lifecycle hooks | Claude Code, Kiro | M | after 0.1 |
| 19 | Merge queue with ordered landings | Vibe Kanban | M | after 0.1 |
| 20 | Container sandbox execution mode | Container Use, Cursor | L | no |

Feature questions Q104–Q123 in [grilling-round-4.md](grilling-round-4.md) put each of these to you as
build / defer / drop, with the recommendation and the reason.

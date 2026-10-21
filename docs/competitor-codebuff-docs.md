# Codebuff docs teardown — 21 Oct 2026

**Scope:** the public documentation at `codebuff.com/docs` plus the pricing page: quick start, FAQ,
modes, how-it-works, models, skills, agent reference, custom agents, SDK, troubleshooting, privacy.
No hands-on session, no bundle inspection.
**Method:** every page below was fetched directly. Claims the docs make about their own performance
are recorded as claims, not facts.
**Confidence:** ✅ read on the page · ⚠️ implied by the docs but not stated · ❓ could not verify
**Related:** [phase-0-teardown.md](phase-0-teardown.md) covers the market and complaint clusters;
this document is narrower — what their docs reveal about their strategy, and where it leaves an opening.

---

## 1. Thesis

Codebuff's documentation is not selling a terminal agent. It is selling an **agent framework**:
`@codebuff/sdk`, TypeScript agent definitions in `.agents/`, an Agent Store, and a CLI whose main job
is to be the shop window. The docs spend their surface area on *authoring agents* (overview,
customizing, creating, reference, troubleshooting, a publishing walkthrough) and comparatively little
on the agent's guarantees: cost, containment, recovery, and failure modes.

That is the opening. Codebuff is optimising for **agent authors and autonomy** ("No permission
prompts… step away and come back to finished work"). Its weakest documentation is exactly where
Rigel's product is strongest: **what a turn costs, what it changed, and how it recovers**. Do not
out-framework them. Become the runtime that makes their agents and skills accountable.

---

## 2. Docs map

| Route | What it documents | What it reveals |
|---|---|---|
| [/help/quick-start](https://www.codebuff.com/docs/help/quick-start) | `npm i -g codebuff`, run in a repo, `/init` | Distribution is npm; `/init` is the on-ramp to custom agents |
| [/help/faq](https://www.codebuff.com/docs/help/faq) | models, data use, instructions, ignore files | Data is used for ads and (conditionally) model training; isolation advice is "use the Dockerfile" |
| [/tips/modes](https://www.codebuff.com/docs/tips/modes) | Default, Max, Plan, Lite | Modes are the product's pricing/power dial |
| [/advanced/how-does-it-work](https://www.codebuff.com/docs/advanced/how-does-it-work) | orchestrator + 7 subagents, best-of-N | The concrete architecture — the most useful page in the set |
| [/advanced/what-models](https://www.codebuff.com/docs/advanced/what-models) | per-role model table | Task-specialised routing is real, not just marketing |
| [/tips/what-makes-codebuff-unique](https://www.codebuff.com/docs/tips/what-makes-codebuff-unique) | speed, tree discovery, parallel editing, compaction, agent framework, ads | The whole positioning in one page; also the least verifiable |
| [/agents/overview](https://www.codebuff.com/docs/agents/overview) | built-in roles, `spawnableAgents` | Multi-agent is a first-class, user-composable primitive |
| [/agents/customizing-agents](https://www.codebuff.com/docs/agents/customizing-agents) | `.agents/*.ts` definitions | Lock-in is a TypeScript module using their types |
| [/agents/creating-new-agents](https://www.codebuff.com/docs/agents/creating-new-agents) | generator-based `handleSteps` | Deterministic orchestration is their strongest idea |
| [/agents/agent-reference](https://www.codebuff.com/docs/agents/agent-reference) | fields, 17 tools, prompt files, schemas | A real extension API; the tools list is buried here, not its own page |
| [/agents/troubleshooting-agent-customization](https://www.codebuff.com/docs/agents/troubleshooting-agent-customization) | load errors, spawn IDs, slow agents, credits | The operational reality behind the framework |
| [/walkthroughs/creating-your-first-agent](https://www.codebuff.com/docs/walkthroughs/creating-your-first-agent) | init → review agent → publish | The Agent Store is the retention loop |
| [/tips/skills](https://www.codebuff.com/docs/tips/skills) | `SKILL.md`, locations, `/skill:` | Standard Agent Skills layout; no install/browse flow |
| [/advanced/sdk](https://www.codebuff.com/docs/advanced/sdk) | API key, `run`, `previousRun`, custom agents/tools | A genuine CI/automation surface |
| [/advanced/troubleshooting](https://www.codebuff.com/docs/advanced/troubleshooting) | install, cache, chat storage | Chats are local files; no documented resume command |
| [/advanced/large-projects](https://www.codebuff.com/docs/advanced/large-projects) | `--cwd`, local knowledge files, ignores | They admit context selection is a user problem at scale |
| [/advanced/privacy](https://www.codebuff.com/docs/advanced/privacy) | data handling | Repo content reaches Freebuff and providers |
| [/advanced/claude-code-comparison](https://www.codebuff.com/docs/advanced/claude-code-comparison) | vs Claude Code | They compete on speed/price, concede Anthropic's controls |
| [/pricing](https://www.codebuff.com/pricing) | $100/1×, $200/2.5×, $500/7×; 1¢/credit | Credits are post-hoc; no free allowance stated |

Missing pages worth noting: a CLI reference, a tool reference, a cost/limits page with numbers, a
security/permission model, failure-mode documentation, and an agent versioning/deprecation policy
(versioned IDs exist, the policy does not).

---

## 3. The product the docs describe

**Loop.** Tree-sitter builds a symbol tree → a fast model (Grok 4.1 Fast on the "unique" page) picks up
to 12 relevant files → Gemini Flash summarises them → the orchestrator reads summaries and spawns
subagents → editors write → reviewers check in parallel with typechecks/tests → commands run locally.
The orchestrator is named **Buffy** in their docs and is described as running on Claude Opus 4.7
(the modes page says Opus 5 — see §5).

**Roles.** Orchestrator (`base2`, `base2-lite`, `base2-max`, `base2-plan`) plus file-picker, code
searcher, researcher, thinker, editor, reviewer, basher. Max spawns multiple editors with different
strategies and a selector that picks and merges the best; their docs claim three parallel attempts
sharing the prompt cache.

**Modes.** Default (one editor, review, checks), Max (best-of-N, multiple reviewers), Plan (context +
questions + `<PLAN>`, no writes), Lite (cheaper model). Switch with `Shift+Tab` or `/mode:`.

**Tools.** 17 documented: `add_subgoal`, `browser_logs`, `code_search`, `create_plan`, `end_turn`,
`find_files`, `read_docs`, `run_file_change_hooks`, `read_files`, `run_terminal_command`,
`spawn_agents`, `str_replace`, `think_deeply`, `update_subgoal`, `web_search`, `write_file`,
`set_output`. Two facts matter more than the count: they can **run terminal commands**, and they can
**spawn agents** at arbitrary depth.

**Customisation.** Agent definitions are TypeScript modules with model (any OpenRouter string), tools,
prompts (inline or file refs), output modes (`last_message`, `all_messages`, `structured_output` +
JSON Schema), inherited history, spawn lists, and generator-based `handleSteps` that can interleave
tool calls and LLM steps. Local agents live in `.agents/`; published agents use
`publisher/name@version`.

**Skills.** Standard `SKILL.md` under `~/.claude/skills`, `~/.agents/skills`, `.claude/skills`,
`.agents/skills` (project wins), invoked by `/skill:<name>` or the `skill` tool. Reload requires a
restart. There is no browse, inspect, install or "why didn't it fire" surface.

**SDK.** `@codebuff/sdk`: API key, `cwd`, `client.run({ agent, prompt, handleEvent, previousRun,
projectFiles, knowledgeFiles, agentDefinitions, customToolDefinitions, maxAgentSteps })`, event types
(`agent_start/finish`, `tool_call/result`, `text`, `error`), `RunState` for continuation. Default step
cap ≈20. This is the strongest part of their distribution story: the same agents are callable from CI.

**Money.** Strong subscriptions at $100 (1×), $200 (2.5×), $500 (7×) per month; pay-as-you-go at
1¢/credit, where "credits are consumed based on task complexity" and "you'll see how many credits
each task consumes". No pre-run estimate, no spend cap, no per-subagent bill in the docs. The free
path is ad credits: optional ads above the input box, impressions convert to credits, and the FAQ
says usage data trains ad models. That is a deliberate trade — free access in exchange for attention
and data — and it is incompatible with the "your code is not ad inventory" position.

---

## 4. Where the docs are strong

1. **Architecture is concrete.** "Tree-sitter map → up to 12 files → summaries → orchestrator" is
   copyable engineering, not a vibe. Compare most competitors' "our agent understands your codebase".
2. **The extension surface is real.** 17 tools, typed agent definitions, deterministic generator
   control flow, custom Zod tools in the SDK, versioned published agents. Power users can build things
   the vendor did not imagine — and that is the moat.
3. **Modes map to budgets.** Default/Max/Plan/Lite is a clearer mental model than a model picker; it
   lets a user trade cost for parallelism with one keystroke.
4. **The SDK page is written for a job.** CI review, test generation, batch refactors, extensions — each
   with code. It tells you exactly what to build with it.
5. **Performance claims are tied to a method.** BuffBench, 175 real commits, AI judge, "ship what
   wins". The numbers are unverified, but the *loop* (eval → configure → ship) is credible and is the
   kind of thing a competitor must answer with its own reproducible harness.

---

## 5. Where the docs are weak — and what that predicts

- **Model versions disagree.** Modes says Claude **Opus 5**; how-it-works and the FAQ say **Opus
  4.7**. The unique page says the file picker is **Gemini 2.0 Flash**; the model page says **Gemini
  3.1 Flash Lite**. This is documentation drift in the pages that define what you are paying for.
- **Mode names disagree.** Quick start mentions "Free, Max, or Plan"; the modes page lists Default,
  Max, Plan, Lite; pricing states no free allowance at all. A user cannot answer "what do I get for
  free?" from the docs.
- **Cost is described, never quantified.** "Credits vary by task complexity" is the opposite of a
  pre-flight estimate. There is no page with per-task or per-mode cost examples, no cap, no budget
  alert, and no per-subagent attribution — despite Max running several editors and reviewers.
- **No containment model.** "No permission prompts" is a feature here. The only isolation advice is
  "use the Dockerfile". There is no approval scope, no diff gate, no command allow-list, no statement
  of what the agent may read outside the repo beyond the privacy FAQ's exceptions.
- **No failure-mode docs.** Nothing on a stale edit, a partial write, a killed process, an interrupted
  session, or how local chat history is reconciled. Chats are stored under
  `~/.config/manicode/projects/...`, but no resume command is documented.
- **No CLI reference.** Flags such as `--cwd` appear inside an advanced page; there is no single
  authoritative surface, which makes automation guesswork.
- **Skills are files, not a product.** Four search paths and a restart are the whole lifecycle. No
  install, no inspection, no provenance, no trigger explanation — the same gap identified in
  [phase-0-teardown.md](phase-0-teardown.md) §6.

Prediction: their velocity is in the framework/agent-store direction, and their guarantees lag. A
competitor that publishes exact cost semantics, failure modes and a containment model can win the
buyer who has been burned by an autonomous agent — without matching the framework.

---

## 6. Head-to-head with Rigel today

✅ present · ⚠️ partial · ❌ absent (docs/implementation as of this snapshot)

| Capability | Codebuff | Rigel | Note |
|---|:--:|:--:|---|
| Streamed chat + edits | ✅ | ✅ | Parity |
| Tool surface | ✅ 17 | ⚠️ 8 | Rigel: read/list/edit/write, skills, budget, spawn — **no terminal, code search, web, hooks** |
| Runs terminal/tests | ✅ | ❌ | The single biggest capability gap; blocks verified completion |
| Repo map / smart file selection | ✅ | ❌ | Codebuff's tree-sitter + small-model picker is ahead of Rigel's `list_files`/`read_file` |
| Parallel editors / best-of-N | ✅ Max | ❌ | No parallel implementations; subagent children run one at a time |
| Subagents / nesting | ✅ unlimited | ⚠️ | `spawn_agents`: 4 read-only children per turn on an 8-model cheap allowlist ($0.05–$0.75/M input), one level deep; the parent sees final answers, not child transcripts |
| Custom agents | ✅ TS + store | ❌ | Rigel has fixed roles |
| SDK / headless automation | ✅ | ⚠️ | Rigel has CLI `--json` and an MCP server, no SDK/event contract |
| Skills catalogue + provenance | ❌ | ✅ | Rigel indexes, grades, searches; harness install is still Phase 6 |
| Skill pinning / provenance | ❌ | ✅ | Session hash pins; grade/evidence in the index |
| Cost meter | ⚠️ credits, post-hoc | ✅ µ$ per agent/turn | Rigel shows the real number |
| Pre-turn estimate | ❌ | ✅ | `turn_estimate` + composer quote |
| Hard cap + stop within one turn | ❌ | ✅ | Reserve/account/interrupt, tested |
| Patch journal / replay / resume | ⚠️ local history | ✅ | CAS writes, write-ahead journal, `session resume` |
| Approvals / containment | ❌ (by design) | ⚠️ | Planned; the schema has approvals, UI does not gate yet |
| BYO key / local-first | ❌ service + providers | ✅ | OS keychain, offline index |
| Desktop supervision surface | ❌ terminal | ✅ | Rigel's roster, transcript, raw toggle, patch pane |
| Verified "done" | ⚠️ claims review + tests | ❌ | Both are weak; Rigel can own this once commands exist |

**Reading:** Codebuff is ahead on the agent's *capability* (execution, retrieval, parallelism,
extensibility). Rigel is ahead on the agent's *accountability* (bill, cap, journal, replay, skills
provenance, locality). Beating them means widening the second column while closing only the first
column's load-bearing gaps — terminal execution and retrieval — not matching all 17 tools.

---

## 7. How Rigel wins — five asymmetric moves

### 7.1 Make execution safe, then make "done" provable

Add `run_terminal_command` behind a policy (allow-list, cwd confinement, approval per command class,
timeout, captured output). Then gate the turn's terminal status on the project's check command:
record command, exit code, output hash and duration as items, and let "completed" require green.
Codebuff's docs promise review and tests; Rigel can *prove* them per turn, with the evidence in the
transcript and in replay. Phase 4, before adapters. Success measure: a fixture turn that edits a
failing test and ends with `cargo test` exit 0, plus a turn whose check fails and is reported failed.

### 7.2 Close the retrieval gap with a repo map, and measure it

Build a tree-sitter symbol map and a small-model file selector, then feed summaries instead of whole
files. This is Codebuff's best engineering idea and Rigel currently has nothing equivalent; without
it, per-turn cost and latency will lose to them on any non-trivial repo. Phase 4. Success measure:
≥50% fewer input tokens on a find-and-fix task at equal pass rate, measured on the same fixture and
published next to the p3 bill.

### 7.3 Sell the per-agent bill, not the per-task credit

Codebuff charges credits per task; Max hides which of three editors spent them. Rigel already stores
`agents.spend_micros`. Surface it: roster cost per agent, per-agent stop, and a per-agent circuit
breaker (e.g. an agent stops at 40% of the session cap). "Kill the expensive subagent without killing
the session" is a control no competitor documents. Phase 5, when claims/parallel agents land.

### 7.4 Absorb their ecosystem instead of fighting the Agent Store

Read `AGENTS.md`, `knowledge.md`, `.agents/skills/`, `.claude/skills/` and Codebuff agent definitions
as importable, read-only manifests; materialise Rigel-installed skills into `.agents/skills` so they
work in Codebuff too. Rigel then becomes the install/verify/provenance layer *for* Codebuff users
rather than a replacement they must migrate to. Phase 4 for skills/knowledge import, Phase 6 for
agent manifests. Success measure: a Codebuff-authored skill is installed in Rigel, hash-pinned, loaded
in a turn, and visible to Codebuff from the same project.

### 7.5 Publish a headless contract before their SDK becomes the default

Add a documented `rigel run --json` event stream (turn started, deltas, items, finished with usage),
a `turn_estimate` equivalent, and a cap flag, plus an MCP tool for the same. Their SDK's CI examples
are the demand signal; a GitHub Action that reviews a PR under a $0.50 hard cap is a direct answer
with a guarantee they do not offer. Phase 4. Success measure: an action runs a review, prints the
bill, and refuses a second run when the cap is exhausted.

### 7.6 Documentation as a product surface

Ship the four pages Codebuff does not have: **cost semantics** (µ$, reserve, per-step billing, cap),
**failure modes** (STALE_BASE, budget exceeded, crash-after-journal, interrupted session), **security
and approvals** (what the agent can read/write/execute, and where it stops), and a **reproducible
benchmark** (same tasks, same models, published pass rate *and* cost, no AI-judge hand-waving). This
is cheap, it forces the product to be honest, and it is the page a burned buyer reads before
switching.

---

## 8. What to copy, what not to

**Copy**

1. The tree-sitter → small-model selection → summaries pipeline (the strongest technical claim).
2. Prompt-cache sharing across parallel attempts, once best-of-N exists; their cost argument depends
   on it.
3. Typed agent definitions with deterministic generator control flow. It is the highest-ceiling
   customisation model in the market.
4. The SDK event vocabulary (`agent_start`, `tool_call`, `text`, `error`, `RunState` continuation).
5. The modes page pattern: one table that maps a mode to model, parallelism and review.

**Do not copy**

1. Permissionless autonomy as the default. It is their brand and their largest liability.
2. Unlimited agent nesting without per-agent budgets; it turns a cost bug into an outage.
3. Credit opacity. "You'll see how many credits each task consumes" is a post-mortem, not a quote.
4. Ad-funded usage that trains ad models on code context. It forecloses the privacy-sensitive buyer.
5. Unverifiable speed multipliers. Answer with a harness, not a percentage.

---

## 9. Positioning

Codebuff's own docs draw the line for us: **they optimise for what the agent can do; Rigel optimises
for what the user can verify.** Their framework is the thing to interoperate with, not to clone.

One sentence:

> **Rigel runs the agents and skills you already have — with a bill you can see before you press
> Enter, a cap that stops the turn, and a journal that proves what changed.**

The first half is the bridge that makes Codebuff users try Rigel. The second half is the reason they
stay.

---

## 10. Sources and limits

Primary pages read on 21 Oct 2026: quick start, FAQ, modes, how-does-it-work, what-makes-it-unique,
skills, agent reference, SDK, pricing. The agent overview/customizing/creating pages and the
privacy/troubleshooting pages were read via the research pass and are cited by URL above.

Limits:

- **No hands-on run.** This is documentation analysis, not measured UX. Their speed and quality claims
  are recorded, not reproduced.
- **Pricing and models move.** Treat $100/$200/$500, 1¢/credit and every model name as a snapshot.
- **The free tier is genuinely unclear.** Quick start says "Free"; the modes page says "Lite"; pricing
  states no free allowance. Marked ❓ rather than guessed.
- **Their internals are only what the docs disclose.** Nothing here is reverse-engineered from the
  binary; open-source claims are taken at face value (Apache 2.0, `github.com/CodebuffAI/codebuff`).

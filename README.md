# Rigel

A local index for agent skills: one catalogue across GitHub and the marketplaces, with duplicate
detection, a static safety scan, an honest measured-vs-advertised column, and an install matrix per agent
harness.

The claim it is built around: **the number on the package is usually not the number you get.** JetBrains
measured three token-saving skills against their advertised claims — one delivered a third of its claim,
one made things worse. Star counts and install numbers surface none of that.

## Status

Phase 0 through **Phase 3 — the native agent, chat and the bill — are complete**. The desktop shell
searches a local SQLite index over skill bodies stored as content-addressed files and works with the
network off; the Session surface runs a native agent with streamed tool calls, a persisted transcript,
replay, a compare-and-swap patch journal, a pre-turn cost estimate and a cap that stops work
within one turn. See the ticked
checklist in [handoff.md](docs/handoff.md) for what each item means.

The index is not the endpoint. The direction, settled in the interview: **one place where several
agents work on the same code together**, with this catalogue as the capability layer they call into.
That shifts the product's spine from grading an index to sandboxing what an agent actually invokes —
see [interview-answers.md](docs/interview-answers.md) for the answers and the seven collisions they
leave behind.

## Run it

```bash
cd prototype
npm install
npm run dev
```

Open the printed localhost URL. `Ctrl+1` packages, `Ctrl+2` discover, `Ctrl+3` targets, `Ctrl+K`
command palette.

## The Rust core

The workspace at the repository root is the app itself: `ui/` is the renderer, `crates/` the Rust
core and CLI, and `crates/rigel-app` the Tauri 2 shell. `prototype/` remains as the visual reference.

```bash
cargo build --workspace
cargo test --workspace
cargo run -q -p rigel-cli -- doctor --json
cargo run -q -p rigel-cli -- session start --project .

# the shelf: walk a repo once, then search it offline forever
cargo run -q -p rigel-cli -- skills index anthropics/skills
cargo run -q -p rigel-cli -- skills search "pdf" --json
cargo run -q -p rigel-cli -- skills get github:anthropics/skills:skills/pdf/SKILL.md
cargo run -q -p rigel-cli -- skills status

# the native agent: one turn, billed to the session
# OmniRoute is the default provider; a turn starts `omniroute serve` if it is down.
printf '%s' "$OMNIROUTE_KEY" | cargo run -q -p rigel-cli -- keys set omniroute
cargo run -q -p rigel-cli -- session start --project . --budget-cents 25
cargo run -q -p rigel-cli -- agent run --project . --prompt "read README and fix the typo"
# a shell command is printed for consent first; `--approval allow` pre-consents, `--json` denies
cargo run -q -p rigel-cli -- agent run --project . --prompt "run the tests" --approval ask
# Google's models through the Gemini API (a key from AI Studio; default gemini-4-argon)
printf '%s' "$GEMINI_API_KEY" | cargo run -q -p rigel-cli -- keys set gemini
cargo run -q -p rigel-cli -- agent run --provider gemini --prompt "say pong"
# one ACP turn against Cursor or Cline; approvals prompt on stdin
# (needs its CLI and its own key: CURSOR_API_KEY or CLINE_API_KEY)
cargo run -q -p rigel-cli -- agent acp --project . --prompt "say pong"
cargo run -q -p rigel-cli -- agent acp --harness cline --project . --prompt "say pong"
cargo run -q -p rigel-cli -- snapshot list
cargo run -q -p rigel-cli -- snapshot capture --project .
cargo run -q -p rigel-cli -- snapshot restore <snapshot-id> --project .
cargo run -q -p rigel-cli -- patch list --project .
cargo run -q -p rigel-cli -- session resume --project . --id <session-id>

cd ui && npm install && npm run dev   # renderer in a browser, sample rows
bash scripts/dev.sh                    # desktop shell, secrets from Infisical (INFISICAL.md)
cargo run -p rigel-app                 # the same, without Infisical: keychain or shell env
cd prototype && npm run dev            # visual reference (Phase 0 browser app)
```

The index lives at `%APPDATA%\Rigel` (`~/.local/share/rigel` elsewhere): `index.db`, plus `blobs/`
holding one file per distinct skill body, addressed by sha256. `RIGEL_INDEX_DIR` or `--data-dir`
points both the CLI and the window at a different one, which is how the tests avoid touching yours.

`skills index` is the only skill command that uses the network. Bodies are fetched from
`raw.githubusercontent.com`, which does not count against GitHub's API quota, so a 500-skill repo
costs two API calls. `RIGEL_GITHUB_TOKEN` (or `GITHUB_TOKEN`) raises the limit from 60 to 5,000
requests/hour if you have one.

`migrate` creates `<project>/.rigel/rigel.db` and writes a `.gitignore` for it, so a session log can
never be committed by accident. The schema is numbered SQL in `migrations/`, applied one transaction at
a time by `rigel_core::migrate`.

On Windows, if the checkout sits under a deep path, set `CARGO_TARGET_DIR=C:/rigel-target` before
building: `link.exe` fails with `LNK1104` once output paths pass 260 characters.

The prototype's Discover view talks to two marketplaces that send no CORS headers, so it uses a
**dev-only** proxy (`/__hub`, see `prototype/vite.config.ts`). The Rust indexer does not need one:
`rigel skills sync skillsmp|skillhub` talks to them server-side (`crates/rigel-core/src/skills/hub.rs`).

## What works

| Surface | State |
|---|---|
| Shelf: search, rows, detail, evidence table, variants | **real**: SQLite FTS5 over `index.db`, offline, `skills.search`/`skills.load` over IPC |
| Shelf: static review | **real**: six capability rules with `file:line` evidence, grade from the same scan the prototype used |
| Shelf: source health | **real**: `ok` / `stale` (with last-sync date) / `broken`, read without a network call |
| Shelf: empty state | offers "Index repository" when no source has ever synced |
| GitHub source | **real**: parse `owner/repo` or any GitHub URL, one tree call, raw bodies, ETag cache |
| SkillsMP / SkillHub | **real** via `rigel skills sync`; metadata only, no bodies (they have none to give) |
| Duplicates | **real**: SimHash ≤ 3 bits, clusters computed after every ingest, variants listed under the row |
| Evidence | **real**: advertised percentages from the description, `unmeasured` until a measurement exists |
| CLI | `rigel skills search|get|index|status|sync|sweep|stats|import|gc`, all with `--json`, §7.2 exit codes |
| Native agent | **real**: OmniRoute (local router, autostarted; default `gpt-5.6-sol-medium`), OpenRouter and Gemini (`gemini-4-argon` by default) streaming with retries, a deterministic scripted model, ten tools with path-checked reads and writes, skills pinned per session |
| Commands | **real**: `run_command` shows the exact text, cwd and timeout before anything runs; cwd is jailed to the project, output is capped at 64 KB per stream, the process is killed at its timeout, and the exit code is recorded as a `command` item. `--approval ask\|allow\|deny`; `--json` defaults to deny |
| Destructive commands | **real**: a deterministic classifier maps a command to policy rows — `delete-outside-project`, `recursive-remove`, `force-push`, `git-reset`, `registry-write` — before approval; the prompt names the rows and the matched token, and the `command` item records them |
| Snapshots | **real**: before an approved destructive command, `git stash create` object (kept alive by `refs/rigel/snapshots/<id>`) plus raw copies of changed and untracked files; a non-git tree or a repository with no commit is copied whole; 2 000 files / 200 MB cap, refusing above it. `rigel snapshot capture\|list\|restore` |
| Deliberation | **real**: a destructive command first gets one round of three read-only advisors (advocate, opponent, referee) on the parent's model, billed to its own turn and shown before the approval prompt; the answers are advisory and cannot allow, deny or alter the gate. Scripted runs record that advice was not gathered instead of consuming their fixed steps |
| Egress | **real**: `fetch_url` asks per origin with `once` / `session` / `always` scopes (`always` persists in `<index dir>/approved-origins.json`), re-approves a redirect that crosses origins, reads only text-like content types, caps the body at 256 KB, and records the page as a `web_search` item with `untrusted: true` |
| Subagents | **real**: `spawn_agents` runs up to 4 read-only children concurrently per turn on an eight-model cheap allowlist (GPT-5/5.4 nano, Gemini Flash-Lite/3 Flash, DeepSeek V4.1 Flash, Qwen3 Coder, Haiku 5.5, GPT-5.4 mini); each child is its own agent row and bill, nested under the parent turn. The allowlist is OpenRouter-shaped, so the tool is hidden on the Gemini provider rather than handing it ids Google rejects |
| The bill | **real**: micro-dollar accounting from provider usage, local price fallback, 80% warning, cap enforced within one turn, `reconcile` |
| Session surface | **real**: roster, role-gutter transcript, streaming text, collapsed tool lines, patch diffs, raw/debug toggle, inline 80% prompt, a live model and effort picker fed by the provider catalogue (provider stays in Settings), and a Swarm button that hands the typed task to every agent in the chat room |
| Patches | **real**: CAS check, write-ahead journal, fsync + rename, idempotent apply, revert once applied |
| Resume | **real**: tree hash vs checkpoint, running turns closed as `cancelled`, journal reconciled without applying, forced continue is an intervention |
| Memory | **real**: committed `MEMORY.md` + topics per agent, absolute-path/secret validator, index rebuild |
| MCP | **real**: stdio server scoped to `--session` and `--agent`, sharing the same tools |
| Codex harness | **real**: `rigel codex probe` and one turn through `codex app-server`, with `codex exec --json` as the tested fallback; Rigel writes the router `-c` overrides itself (verified against Codex 0.162.1) |
| Cursor/Cline harness | **real protocol**: `rigel agent acp --harness cursor|cline` drives one ACP turn, records session updates and both sides of every permission request, and refuses a subscription-only identity; each harness takes its own key (`CURSOR_API_KEY`, `CLINE_API_KEY`) and is probed separately. The approval surface is the CLI prompt until P4.10. Verified against a scripted ACP agent — neither CLI is installed on this machine |
| Prototype migration | `rigel skills import --from` + [the export snippet](docs/migrating-prototype-index.md) |
| Discover / Targets / Dependencies (prototype) | still the Phase 0 browser app; Phase 6 ports them |

## Docs

- [qa/p2.md](docs/qa/p2.md) — the Phase 2 exit test, with recorded results
- [qa/p3.md](docs/qa/p3.md) — the Phase 3 exit test: killed turn, resume, idempotent patch, the cap
- [migrating-prototype-index.md](docs/migrating-prototype-index.md) — moving the `skilldb:` rows into SQLite
- [phase-0-teardown.md](docs/phase-0-teardown.md) — competitor analysis
- [competitor-codebuff-docs.md](docs/competitor-codebuff-docs.md) — what Codebuff's docs reveal, and where Rigel beats it
- [skill-intelligence.md](docs/skill-intelligence.md) — discovery, fingerprinting, safety, ranking
- [design-prompt.md](docs/design-prompt.md) — the visual brief this UI follows
- [handoff.md](docs/handoff.md) — **the build plan**: architecture, schemas, interfaces, and a tickable checklist (P0.1–P7.9)
- [interview-answers.md](docs/interview-answers.md) — 60 answered questions, the contradictions, and the ChatGPT handoff
- [phase-1-plan.md](docs/phase-1-plan.md) — the earlier index-only plan; superseded by the handoff
- [grilling-50.md](docs/grilling-50.md) — 66 questions to stress-test the plan

## Scripts

- `node scripts/export-installed-skills.mjs` — snapshot installed skill names into the prototype
- `node scripts/verify-ecc.mjs` — verify ECC across every harness; non-zero on failure
- `node scripts/fix-cursor-hooks.mjs` — repair Cursor's project-relative hook paths
- `node scripts/handoff-status.mjs` — checklist progress per phase; fails if the §11 summary table is stale
- `bash scripts/dev.sh` — local dev: the desktop shell under `infisical run --env=dev`, with the setup steps in [INFISICAL.md](INFISICAL.md)
- `bash scripts/p3-exit-test.sh` — the Phase 3 exit test; needs a built `rigel` and `python`
- `scripts/fixtures/p4-subagents.json` — a scripted parent that delegates to two cheap readers; `rigel agent run --script` prints their bills

## Limits, stated plainly

- The safety grade is a static scan of one file. It does not execute anything and cannot prove a skill
  is harmless. `A` is unreachable from a static scan and stays reserved until Phase 6 weighs
  declared-vs-actual tool use and provenance.
- **Freshness is a ten-minute window.** Re-indexing inside it makes zero HTTP requests; after it, each
  URL is revalidated with `If-None-Match`, and `--force` bypasses the window entirely. A repo that
  changed while cached is only seen after the window expires or with `--force`.
- The GitHub path uses unauthenticated quota (60 requests/hour) unless `RIGEL_GITHUB_TOKEN` is set.
  Six hundred skills cost two API calls, so the quota is rarely the limit; the raw body fetches are.
- Skill bodies live on disk in `blobs/`, not in SQLite. Deleting that directory forces a re-fetch; the
  rows will report `body: not fetched` until one happens. `rigel skills gc` only removes blobs no row
  or cache entry names.
- **Subagents are cheap, read-only and concurrent.** The allowlist is eight models, all at
  $5/M output or less: `openai/gpt-5-nano` (default), `openai/gpt-5.4-nano`,
  `google/gemini-2.5-flash-lite`, `deepseek/deepseek-v4.1-flash`,
  `qwen/qwen3-coder-30b-a3b-instruct`, `anthropic/claude-haiku-5.5`,
  `google/gemini-3-flash-preview` and `openai/gpt-5.4-mini`. A parent cannot name anything outside
  it, so it cannot point a child at an expensive model to dodge its own cap. Prices and context
  windows were read from OpenRouter's live model list on 21 Oct 2026 and are mirrored in
  `pricing.rs`, so a provider that omits cost is still billed. Children can read, list and search
  but not write or spawn, so nesting is one level deep. Up to four children per turn run at the
  same time: they are registered before any starts, then polled together, and their results come
  back in the order the parent asked for them, not finish order. `BudgetService::account` takes a
  writer lock, so two children reporting usage at once cannot lose a charge. A child's turn token
  is a child of the parent's, so stopping the parent — or a budget kill — stops the whole fan-out;
  a stop that lands mid-stream still charges the step and closes the turn `cancelled`. Four steps
  each, and a child's turn is not counted as a task. The cap still ends a running child after the
  step that crosses it, which means a fan-out near the cap can overshoot by one step per running
  child.
- Clustering uses one signal (content SimHash), not the three the design calls for. MinHash and tree
  signatures are still open.
- `prototype/src/installed.json` contains the names of skills present on the machine that generated it.

## Naming

Rigel, named for the bright anchor star in the mark. The Phase 0 document is titled Skill Index, and
the prototype still uses `skilldb:` keys; both are now historical. Phase 2 moved that data into
`index.db` — see [the migration note](docs/migrating-prototype-index.md) — and nothing in `crates/`
or `ui/` reads those keys.

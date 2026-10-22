<div align="center">

# Rigel

**One catalogue for every agent skill — and an honest column for what each one actually does.**

A local index across GitHub and the skill marketplaces, with duplicate detection, a static safety
scan, measured-vs-advertised evidence, and an install matrix per agent harness.

[![ci](https://github.com/OkVurb/Rigel/actions/workflows/ci.yml/badge.svg)](https://github.com/OkVurb/Rigel/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/rust-stable-DEA584?logo=rust&logoColor=white)
![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-blue)
![Skills indexed](https://img.shields.io/badge/skills%20indexed-9%2C996%20metadata%20rows-4c1)
![Search](https://img.shields.io/badge/search-SQLite%20FTS5%20·%20offline-0969da)

</div>

---

The claim it is built around: **the number on the package is usually not the number you get.** JetBrains
measured three token-saving skills against their advertised claims — one delivered a third of its claim,
one made things *worse*. Star counts and install numbers surface none of that, so Rigel puts the
advertised column next to the measured one and leaves the measured one blank until something was
actually run.

## What it does

**Rigel** — *a shelf your agents can pull capabilities from without the network or a marketplace login.*

- **The shelf** — one local index over both skill marketplaces and any GitHub repo, searchable
  offline by SQLite FTS5. Skill bodies are stored as content-addressed files on disk (one file per
  distinct body, addressed by sha256), so `rigel skills search "pdf"` answers instantly with the
  network off.
- **The sweep** — walks both marketplaces across many pages in one command. Its first run collapsed
  **15,741 metadata rows into 9,996 unique skills** by dropping 825 cross-marketplace duplicates on
  ingest. See [the sweep QA record](docs/qa/index-sweep-2026-10-21.md).
- **The review** — a static safety scan per skill: six capability rules with `file:line` evidence.
  Advertised token savings are marked `unmeasured` until a measurement exists.
- **The engineer** — a native agent surface (Session) with streamed tool calls, a persisted transcript,
  replay, a compare-and-swap patch journal, and a pre-turn cost estimate with a budget cap that stops
  work within one turn. **Phase 0 through Phase 3 are complete**; the ticked checklist is in
  [handoff.md](docs/handoff.md).
- **The room** — a Chat tab over a locally hosted agentchattr server: several CLI agents in one
  channel, each handed Rigel's own MCP tools (skills, memory, patch proposals) scoped to its agent
  row, with the session token held in Rust and never in the window. Verified end to end with Codex;
  Claude's interactive tool gating is recorded honestly in
  [the QA record](docs/qa/agentchattr-integration.md).

## Quick start

The Rust workspace at the repository root *is* the app now. `prototype/` is kept as the visual
reference; `ui/` is the desktop renderer, `crates/rigel-app` the Tauri 2 shell.

```bash
cargo build --workspace
cargo test --workspace
cargo run -q -p rigel-cli -- doctor --json

# the shelf: walk a repo once, then search it offline forever
cargo run -q -p rigel-cli -- skills index anthropics/skills
cargo run -q -p rigel-cli -- skills search "pdf" --json
cargo run -q -p rigel-cli -- skills get github:anthropics/skills:skills/pdf/SKILL.md
cargo run -q -p rigel-cli -- skills status

# the sweep: grow the catalogue across the marketplaces (metadata only)
cargo run -q -p rigel-cli -- skills sweep --skillhub-pages 150 --skillsmp-pages 1
cargo run -q -p rigel-cli -- skills stats

# the engineer: one turn, billed to the session
# OmniRoute is the default provider; a turn starts `omniroute serve` if it is down.
printf '%s' "$OMNIROUTE_KEY" | cargo run -q -p rigel-cli -- keys set omniroute
cargo run -q -p rigel-cli -- session start --project . --budget-cents 25
cargo run -q -p rigel-cli -- agent run --project . --prompt "read README and fix the typo"
cargo run -q -p rigel-cli -- patch list --project .

cargo run -p rigel-app                 # desktop shell; builds ui/dist first
cd ui && npm install && npm run dev    # renderer in a browser, sample rows
cd prototype && npm run dev            # visual reference (Phase 0 browser app)
```

The index lives at `%APPDATA%\Rigel` (`~/.local/share/rigel` elsewhere): `index.db`, plus `blobs/`
holding one file per distinct skill body, addressed by sha256. `RIGEL_INDEX_DIR` or `--data-dir` points
both the CLI and the window at a different one, which is how the tests avoid touching yours.

## Where the 10,000 came from

`skills sweep` is documented in
[docs/qa/index-sweep-2026-10-21.md](docs/qa/index-sweep-2026-10-21.md), with the raw numbers:

| Stage | Rows |
|---|---|
| SkillHub list pages (100 cards/request) | 14,741 |
| SkillsMP search pages (50/query, 20 queries) | 1,000, then rate-limited |
| Cross-marketplace duplicates dropped | −825 |
| **Unique skills in the index** | **9,996** |

Everything was unauthenticated and metadata-only. `skills index <owner/repo>` remains the only
command that fetches bodies; `skills sweep` ingests catalogue rows so the shelf knows what exists
before anyone decides what to pin. SkillHub paged cleanly across 150 requests; SkillsMP returns
`429` after a handful of pages, which is why it contributes fewer rows — the QA record has the
detail. Clusters are recomputed from SimHash exactly once per sweep; the dedupe pass then collapses
rows with identical source and slug.

## What's real

| Surface | State |
|---|---|
| Shelf: search, rows, detail, evidence table, variants | **real**: SQLite FTS5 over `index.db`, offline, `skills.search`/`skills.load` over IPC |
| Shelf: static review | **real**: six capability rules with `file:line` evidence, grade from the same scan the prototype used |
| Shelf: source health | **real**: `ok` / `stale` (with last-sync date) / `broken`, read without a network call |
| SkillHub / SkillsMP browsability | **real** via `skills sync` and `skills sweep` — metadata-only, no bodies to fetch |
| Duplicates | **real**: SimHash ≤ 3 bits, clusters computed after every ingest, variants listed under the row |
| Evidence | **real**: advertised percentages from the description, `unmeasured` until a measurement exists |
| CLI | `rigel skills search|get|index|status|sync|sweep|stats|import|gc`, all with `--json` |
| Native agent | **real**: OmniRoute streaming with retries, a deterministic scripted model, eight tools with path-checked reads and writes, skills pinned per session |
| Subagents | **real**: up to 4 read-only children per turn on a cheap-model allowlist, each its own agent row and bill |
| The bill | **real**: micro-dollar accounting from provider usage, local price fallback, 80% warning, cap enforced within one turn |
| Patches | **real**: CAS check, write-ahead journal, fsync + rename, idempotent apply, revert |
| Resume | **real**: tree hash vs checkpoint, running turns closed as `cancelled`, journal reconciled |
| MCP | **real**: stdio server scoped to `--session` and `--agent` |
| Codex harness | **real**: `rigel codex probe` and one turn through `codex app-server` |

## Docs, in reading order

1. [handoff.md](docs/handoff.md) — **the build plan**: architecture, schemas, interfaces, and a
   tickable checklist (P0.1–P7.9)
2. [interview-answers.md](docs/interview-answers.md) — 60 answered questions, the contradictions,
   and the ChatGPT handoff
3. [skill-intelligence.md](docs/skill-intelligence.md) — discovery, fingerprinting, safety, ranking
4. [grilling-50.md](docs/grilling-50.md) — 66 questions to stress-test the plan
5. [qa/local-model.md](docs/qa/local-model.md) — which local model backs the agent, with timings
6. [qa/index-sweep-2026-10-21.md](docs/qa/index-sweep-2026-10-21.md) — the 10k catalogue sweep
7. [qa/p2.md](docs/qa/p2.md) — the Phase 2 exit test, with recorded results
8. [qa/p3.md](docs/qa/p3.md) — the Phase 3 exit test: killed turn, resume, idempotent patch, the cap
9. [migrating-prototype-index.md](docs/migrating-prototype-index.md) — moving the `skilldb:` rows into SQLite
10. [phase-0-teardown.md](docs/phase-0-teardown.md) — competitor analysis
11. [competitor-codebuff-docs.md](docs/competitor-codebuff-docs.md) — what Codebuff's docs reveal, and where Rigel beats it
12. [design-prompt.md](docs/design-prompt.md) — the visual brief this UI follows
13. [phase-1-plan.md](docs/phase-1-plan.md) — the earlier index-only plan; superseded by the handoff
14. [qa/agentchattr-integration.md](docs/qa/agentchattr-integration.md) — the chat-room integration record: what works, the MCP verification pass, and the Claude approval trap
15. [multi-agent-channels-draft.md](docs/multi-agent-channels-draft.md) — the phase 5/6 channels design, built from what the room proved

The index is not the endpoint. The direction, settled in the interview: **one place where several
agents work on the same code together**, with this catalogue as the capability layer they call into.

## Scripts

- `node scripts/export-installed-skills.mjs` — snapshot installed skill names into the prototype
- `node scripts/verify-ecc.mjs` — verify ECC across every harness; non-zero on failure
- `node scripts/fix-cursor-hooks.mjs` — repair Cursor's project-relative hook paths
- `node scripts/handoff-status.mjs` — checklist progress per phase; fails if the §11 summary table is stale
- `bash scripts/p3-exit-test.sh` — the Phase 3 exit test; needs a built `rigel` and `python`
- `bash scripts/stress-concurrency.sh` — the CI concurrency stress run
- `scripts/fixtures/p4-subagents.json` — a scripted parent that delegates to two cheap readers; `rigel agent run --script` prints their bills

## Limits, stated plainly

- The safety grade is a static scan of one file. It does not execute anything and cannot prove a skill
  is harmless. `A` is unreachable from a static scan and stays reserved until Phase 6 weighs
  declared-vs-actual tool use and provenance.
- **Freshness is a ten-minute window.** Re-indexing inside it makes zero HTTP requests; after it, each
  URL is revalidated with `If-None-Match`, and `--force` bypasses the window entirely. A repo that
  changed while cached is only seen after the window expires or with `--force`.
- The GitHub path uses unauthenticated quota (60 requests/hour) unless `RIGEL_GITHUB_TOKEN` /
  `GITHUB_TOKEN` is set — that raises it to 5,000/hour. Six hundred skills cost two API calls, so the
  quota is rarely the limit; the raw fetched bodies are.
- Skill bodies live on disk in `blobs/`, not in SQLite. Deleting that directory forces a re-fetch; the
  rows will report `body: not fetched` until one happens. `rigel skills gc` only removes blobs no row
  or cache entry names.
- **Subagents are cheap, read-only and concurrent.** The allowlist is eight models, all at $5/M output
  or less; a parent cannot name anything outside it, and children can read and search but not write
  or spawn. Prices are mirrored in `pricing.rs` so a provider that omits cost is still billed.
- Clustering uses one signal (content SimHash), not the three the design calls for. MinHash and tree
  signatures are still open.
- The sweep's row counts reflect unauthenticated access on the day it ran; SkillsMP rate-limits after
  a few pages, so a re-run's SkillsMP share varies with its mood.
- `prototype/src/installed.json` contains the names of skills present on the machine that generated it.

## Naming

Rigel — named for the bright anchor star in the mark. The Phase 0 document was titled *Skill Index*,
and the prototype still uses `skilldb:` keys; both are historical now. Phase 2 moved that data into
`index.db` — see [the migration note](docs/migrating-prototype-index.md) — and nothing in `crates/`
or `ui/` reads those keys.

## License

No license file yet — all rights reserved until the author picks one. Ask before reusing.

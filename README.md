# Rigel

A local index for agent skills: one catalogue across GitHub and the marketplaces, with duplicate
detection, a static safety scan, an honest measured-vs-advertised column, and an install matrix per agent
harness.

The claim it is built around: **the number on the package is usually not the number you get.** JetBrains
measured three token-saving skills against their advertised claims — one delivered a third of its claim,
one made things worse. Star counts and install numbers surface none of that.

## Status

Phase 0 and Phase 1 are complete; **Phase 2 — the shelf, real — is complete**. The desktop shell
searches a local SQLite index over skill bodies stored as content-addressed files, and works with the
network off. See the ticked checklist in [handoff.md](docs/handoff.md) for what each item means.

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

cd ui && npm install && npm run dev   # renderer in a browser, sample rows
cargo run -p rigel-app                 # desktop shell; builds ui/dist first
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
| CLI | `rigel skills search|get|index|status|sync|stats|import|gc`, all with `--json`, §7.2 exit codes |
| Prototype migration | `rigel skills import --from` + [the export snippet](docs/migrating-prototype-index.md) |
| Discover / Targets / Dependencies (prototype) | still the Phase 0 browser app; Phase 6 ports them |

## Docs

- [qa/p2.md](docs/qa/p2.md) — the Phase 2 exit test, with recorded results
- [migrating-prototype-index.md](docs/migrating-prototype-index.md) — moving the `skilldb:` rows into SQLite
- [phase-0-teardown.md](docs/phase-0-teardown.md) — competitor analysis
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
- Clustering uses one signal (content SimHash), not the three the design calls for. MinHash and tree
  signatures are still open.
- `prototype/src/installed.json` contains the names of skills present on the machine that generated it.

## Naming

Rigel, named for the bright anchor star in the mark. The Phase 0 document is titled Skill Index, and
the prototype still uses `skilldb:` keys; both are now historical. Phase 2 moved that data into
`index.db` — see [the migration note](docs/migrating-prototype-index.md) — and nothing in `crates/`
or `ui/` reads those keys.

# Skill Index

A local index for agent skills: one catalogue across GitHub and the marketplaces, with duplicate
detection, a static safety scan, an honest measured-vs-advertised column, and an install matrix per agent
harness.

The claim it is built around: **the number on the package is usually not the number you get.** JetBrains
measured three token-saving skills against their advertised claims — one delivered a third of its claim,
one made things worse. Star counts and install numbers surface none of that.

## Status

Phase 0 prototype. Browser-only React app with mock data for the package list, and **real** GitHub,
SkillsMP and SkillHub fetching in the Discover view. No Tauri shell yet, no SQLite yet.

## Run it

```bash
cd prototype
npm install
npm run dev
```

Open the printed localhost URL. `Ctrl+1` packages, `Ctrl+2` discover, `Ctrl+3` targets, `Ctrl+K`
command palette.

The Discover view talks to two marketplaces that send no CORS headers, so it uses a **dev-only** proxy
(`/__hub`, see `prototype/vite.config.ts`). It is allow-listed to `skillsmp.com` and
`skills.browseract.com`, and exists only on the dev server. The production build shows a clear error
instead.

## What works

| Surface | State |
|---|---|
| Package list, detail, variants, evidence table | mock data, real interactions |
| Discover → GitHub | live: repo tree, frontmatter parse, static scan, download |
| Discover → SkillsMP | live via proxy, 50 anonymous requests/day |
| Discover → SkillHub | live via proxy, HTML parsed |
| Discover → All | merges every source, marks on-disk duplicates |
| Discover → Dependencies | 20 real tools with winget/brew commands and script download |
| Targets | this machine's measured harness state, from `scripts/verify-ecc.mjs` |
| Local index | `localStorage` metadata store; SQLite is the Phase 1 replacement |
| Agent surface | copy/download a plain-text brief; `⤓ index` exports the whole catalogue |

## Docs

- [phase-0-teardown.md](docs/phase-0-teardown.md) — competitor analysis
- [skill-intelligence.md](docs/skill-intelligence.md) — discovery, fingerprinting, safety, ranking
- [design-prompt.md](docs/design-prompt.md) — the visual brief this UI follows
- [phase-1-plan.md](docs/phase-1-plan.md) — SQLite, Rust indexer, CLI, dependency manager
- [grilling-50.md](docs/grilling-50.md) — 66 questions to stress-test the plan

## Scripts

- `node scripts/export-installed-skills.mjs` — snapshot installed skill names into the prototype
- `node scripts/verify-ecc.mjs` — verify ECC across every harness; non-zero on failure
- `node scripts/fix-cursor-hooks.mjs` — repair Cursor's project-relative hook paths

## Limits, stated plainly

- The safety grade is a static scan of one file. It does not execute anything and cannot prove a skill
  is harmless.
- The GitHub path uses unauthenticated quota (60 requests/hour). Indexes are cached for an hour.
- `prototype/src/installed.json` contains the names of skills present on the machine that generated it.

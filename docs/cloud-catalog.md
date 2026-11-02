# The shared catalogue

Rigel's shelf used to be built entirely on the user's machine: crawl GitHub, parse marketplaces, write
`index.db`. That is fine for a machine that already has the index and miserable for a fresh install,
which has to spend GitHub quota before it can show anything.

The Realtime Database now carries the **metadata** of the catalogue, so a new install can seed itself
and a browser (or the web reader) can browse skills and MCP servers without a local index at all.

Bodies are deliberately **not** published. A skill body is the expensive part — the current index holds
1,395 bodies across 66 MB of blobs — and shipping them would cost both storage and the safety story
(Rigel shows what a skill contains *because* it fetched it itself). What goes up is a row you can
search: name, description, source, repo, grade, cluster.

## Endpoints

Public read, no auth, no key:

```
https://rigel-25128-default-rtdb.firebaseio.com/catalog/v1/meta.json
https://rigel-25128-default-rtdb.firebaseio.com/catalog/v1/skills/<key>.json
https://rigel-25128-default-rtdb.firebaseio.com/catalog/v1/mcps/<key>.json
https://rigel-25128-default-rtdb.firebaseio.com/catalog/v1/skills.json?shallow=true   # key list
```

`<key>` is `sha256(id)[:20]` — RTDB forbids `.`, `#`, `$`, `[`, `]` and `/` in keys, and skill ids
contain all of those (`github:owner/repo:skills/foo/SKILL.md`).

Writes are admin-only. `database.rules.json` grants `.read` on `/catalog` and denies every write, so
only the CLI's credentials (which bypass rules) can publish.

## Row shapes

Short field names keep ~50k rows from becoming tens of megabytes of repeated keys.

**Skill** (`catalog/v1/skills/<key>`)

| Field | Meaning |
|---|---|
| `i` | full skill id (`github:owner/repo:path` or `skillhub:slug`) |
| `n`, `d` | name, description (clipped to 300 chars) |
| `s` | source: `github`, `skillhub`, `skillsmp`, `local` |
| `r`, `p`, `u` | repo, path, canonical URL |
| `g`, `c` | grade and cluster id (empty until scanned) |
| `st`, `in`, `tk` | stars, installs, tokens |
| `t` | last seen (epoch ms) |

**MCP server** (`catalog/v1/mcps/<key>`)

| Field | Meaning |
|---|---|
| `i` | catalogue id (`registry:<name>`, `smithery:<qualified>`, `awesome:<owner/repo>`) |
| `n`, `d` | display name, description |
| `r`, `h` | repository (`owner/repo`), homepage |
| `s`, `src` | primary source, all sources that listed it |
| `tr`, `ins`, `v` | transport (`streamable-http`, `package`, `remote`), install hint, version |
| `ver`, `uses` | registry-verified flag, Smithery use count |

## Refreshing it

```bash
node scripts/fetch-mcp-catalog.mjs              # merge the three MCP sources into data/mcp-catalog.json
node scripts/publish-catalog.mjs --dry-run      # show what would go up
node scripts/publish-catalog.mjs                # skills + MCP servers
node scripts/publish-catalog.mjs --skills-only  # after a sweep
```

`publish-catalog.mjs` writes in chunks of 800 through `firebase database:update`, so a mid-run failure
leaves the earlier chunks in place and a re-run is idempotent — every row is keyed by its own hash.

## MCP sources and their terms

| Source | Rows pulled | Licence / terms |
|---|---|---|
| `registry.modelcontextprotocol.io` | 40,000 (400 pages, capped) | registry code Apache-2.0; no explicit data licence found |
| `registry.smithery.ai` | 500 (pagination stopped early — a bug worth fixing) | terms not stated on the page read |
| `punkpeye/awesome-mcp-servers` | 4,632 | **MIT** |

Every row keeps `s`/`src`, so a UI can attribute a listing and link back. Glama (≈114k) was **not**
used: its directory API requires a key and its licence requires attribution plus a per-listing
backlink. Nothing here is generated — a row exists only because a source listed it.

## The honest size question

The request was 300,000 skills. The best independent census available says the whole public universe is
about **238,000 unique skills** — a March 2026 crawl collected 349,664 rows across three marketplaces
plus GitHub and de-duplicated them to 238,180, i.e. roughly a third were copies. So 300k is above the
number of distinct skills that exist, and any file with 300,000 rows would have to be mostly invented.

What is real today:

- **11,580 skills** in `index.db` (11,429 at publish time), 1,395 with bodies, 1,322 clusters.
- Growth is quota-bound, not code-bound: a SkillHub sweep added 151 net-new rows from 5 pages before
  SkillsMP rate-limited at 50 requests/day. Indexing repositories costs 2 GitHub API calls each at
  60/hour unauthenticated — one repo roughly every two minutes — or 5,000/hour with a token.
- Reaching 100k+ means a token, a multi-hour crawl, and ~35% of the raw rows collapsing as duplicates.

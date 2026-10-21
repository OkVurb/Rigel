# Phase 1 plan — from prototype to a local index

The prototype answers one question: does the interface make the trade-offs legible? It does. Phase 1
makes the numbers real and keeps them on disk.

Order matters. Each step is useful on its own, and each unblocks the next.

---

## Step 0 — freeze the contract (half a day)

The prototype already defines the record shapes. Before moving them into Rust, write them down:

| Record | Fields | Why it is stable |
|---|---|---|
| `Skill` | id, name, description, source, path, url, grade, tokens | everything the list needs |
| `Target` | id, name, root, skillsPath, hooks, skipped[] | harness materialisation |
| `Dependency` | id, name, provides[], commands[], docs | install surface |
| `Evidence` | metric, measured, advertised, source, date | accountability claim |

Acceptance: a `schema.md` that the Rust structs and the TypeScript types both cite.

---

## Step 1 — SQLite instead of localStorage (2–3 days)

`prototype/src/db.ts` is a deliberate stand-in. Phase 1 replaces it with one file:

```
~/.local/share/skill-index/index.db
  skills(id, name, description, source, url, grade, tokens, saved_at)
  skills_fts            -- FTS5 over name + description
  targets(id, root, skills_path, hooks, skipped_json, checked_at)
  dependencies(id, name, provides_json, commands_json, docs)
  installs(skill_id, target_id, installed_at, files_json)
```

- Migrations are numbered SQL files; no ORM.
- `skills_fts` is what makes "inquire in text" fast at 10k rows.
- Bodies stay on disk as content-addressed blobs (`blobs/<sha256>`), never in SQLite.

Acceptance: importing 500 skills, restarting the app, and querying offline returns identical rows.

**Why not IndexedDB:** it does not survive a Tauri reinstall, has no FTS, and cannot be inspected with
`sqlite3` — which is the debugging story that matters when an agent misbehaves.

---

## Step 2 — the Rust indexer (1 week)

Three commands, one binary:

```
skill-index index <repo|url>     # fetch tree, write metadata rows, fetch bodies lazily
skill-index verify               # per-target structure + hook target check
skill-index query "pdf editing"  # FTS5, JSON out
```

- GitHub: one tree call per repo, bodies from `raw.githubusercontent.com`. Same quota logic the
  prototype proved: 60 req/hr unauthenticated, 5,000 with a token.
- Optional `GH_TOKEN` from the OS keychain, never a config file.
- Marketplace connectors (SkillsMP, SkillHub) live behind a trait so a dead source is a config change.

Acceptance: `skill-index index anthropics/skills` twice — the second run makes zero API calls.

---

## Step 3 — the agent surface (3–4 days)

This is the part that turns a browser into infrastructure.

```
skill-index ask "is there a skill for OCR?"   # ranked rows + why
skill-index brief <id>                        # the agent brief, as text
skill-index get <id> --to ./skills            # writes SKILL.md + siblings
skill-index deps --missing                    # binaries this skill expects
```

- `ask` returns names, grades, sources and a one-line reason — not prose.
- `brief` is exactly what the detail pane's copy button produces today.
- `get` is a plain filesystem write; no package manager, no daemon.

Acceptance: an agent with only the CLI can discover, inspect, download and verify a skill in three
turns, offline after indexing.

---

## Step 4 — dependencies and terminals (1 week)

The Dependencies tab becomes real:

- Detect installed binaries (`which`/`where`) and versions.
- Per-skill requirement inference: `allowed-tools` plus a scan of command lines in the body.
- One-click install through winget/brew/apt **with the command shown before it runs** — never a hidden
  `execSync`.
- Terminal profiles: enumerate PowerShell, Git Bash, WSL distros and SSH targets; let a command target a
  specific profile. This is what makes "the agent is missing a tool" recoverable rather than a dead end.

Acceptance: a fresh VM reaches a green run of the sample skill after only clicks inside the app.

---

## Step 5 — trust and ranking (ongoing)

- Run the clone fingerprinting from `skill-intelligence.md` over the index, not per-query.
- Keep duplicate clusters visible; never silently collapse them.
- Ship no curated package without a measured row or an explicit "unmeasured" label.
- Re-verify measured claims quarterly; store the source URL and date next to every number.

Acceptance: the top result for ten arbitrary queries is defensible in one sentence each.

---

## Step 6 — distribution (after 5)

- Tauri shell signed for Windows and macOS; SQLite and blobs in the app data dir.
- Update checks read a signed manifest; no silent skill updates.
- Export/import of the index as a single archive, so a team can share a reviewed set.

---

## Risks, ranked

1. **GitHub quota** — solved by tree-once + raw bodies + ETag caching; measured in Phase 0.
2. **Marketplace drift** — SkillsMP and SkillHub are scrapers at heart. Treat them as optional
   connectors; the app must be fully usable with GitHub alone.
3. **Static review overreach** — the grade is a signal, not a guarantee. The wording rule from the
   design brief ("no dangerous capabilities detected") must survive into the CLI output.
4. **Scope creep into a package manager** — this indexes and explains; it does not own installation.

---

## What is deliberately not in Phase 1

- Windows/macOS code signing
- A hosted index or accounts
- Auto-updating skills
- Any always-on background service

Each of those changes the trust story, and none of them is needed to prove the product.

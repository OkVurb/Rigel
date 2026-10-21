# Moving the prototype's index into SQLite (P2.9)

The Phase 0 prototype kept its index in `localStorage` under two keys:

| Key | Contents |
|---|---|
| `skilldb:entries:v1` | up to 2,000 metadata rows — the shelf's list |
| `skilldb:catalogue:v1` | packages discovered live in the Discover view |

Phase 2 moves that data into `index.db` once, and the new app never writes those keys again.

## Why it is not automatic

`localStorage` belongs to an origin. The prototype ran on `http://localhost:5173`; the desktop app
runs on `tauri://localhost`. A Tauri window cannot read another origin's storage, and pretending
otherwise would mean either shipping a hidden browser or asking the user to paste JSON into a
webview. Both are worse than one paste into a console they are already looking at.

## The export (once, in the prototype's tab)

Open the prototype (`cd prototype && npm run dev`), open devtools, and run:

```js
const entries = JSON.parse(localStorage.getItem("skilldb:entries:v1") || "[]");
const blob = new Blob([JSON.stringify(entries, null, 2)], { type: "application/json" });
const link = document.createElement("a");
link.href = URL.createObjectURL(blob);
link.download = "skilldb-entries.json";
link.click();
console.log(`${entries.length} rows exported`);
```

The file lands in your downloads folder. Import it:

```bash
rigel skills import --from ~/Downloads/skilldb-entries.json
# imported 446 · skipped 0 (already present) · invalid 0     ← your numbers will differ
```

## What the import does

- Each row becomes a `local:` skill with its **original grade**, stars, installs, URL and
  `savedAt` — so "last seen" keeps its old date instead of jumping to today.
- Rows whose URL is already in the index are skipped, so running it twice imports nothing.
- A row with a blank name is counted `invalid` and not written.
- No body is attached. The prototype never stored bodies, and inventing one would make the shelf
  claim a static review it never performed. These rows show `body: not fetched` until the
  repository is indexed normally.

## Then delete the keys

Only after the import reports what you expect, clear them in the prototype tab:

```js
localStorage.removeItem("skilldb:entries:v1");
localStorage.removeItem("skilldb:catalogue:v1");
```

`rigel skills import` deliberately does not do this: the app has no access to that origin, and a
migration that silently deletes browser data on your behalf is not a migration.

## After the migration

`prototype/src/db.ts` is the only code that still names the `skilldb:` keys. Nothing in `crates/` or
`ui/` reads or writes them, which is what "then removal" means for the new app. The prototype stays
as the Phase 0 visual reference, frozen.

# Contributing

Rigel is a Rust workspace with a React renderer in `ui/` and a Tauri 2 shell. The plan, the
schemas and the reason for each decision live in [docs/handoff.md](docs/handoff.md); read the
"Non-negotiables" table first, because a change that violates one is a bug even if the tests pass.

## Set up

- Rust **1.85 or newer** (`rust-toolchain` is not pinned; the workspace's `rust-version` is).
- Node **20 or newer** for `ui/` and the prototype.
- Windows: WebView2 runtime (installed by default on Windows 11 and by the installer on 10).
- `git` on `PATH`, and `ripgrep` if you want the exit scripts to exercise a present tool.

```bash
cargo build --workspace
(cd ui && npm install)
```

On Windows, a deep checkout can break `link.exe` with `LNK1104`. Set
`CARGO_TARGET_DIR=C:/rigel-target` before building.

## The checks CI runs

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd ui && npx tsc --noEmit && npm run build)
```

Run them before opening a pull request. A failing test is fixed by fixing the cause: no
`#[ignore]`, no weakened assertion, no snapshot update without reading the diff, no `allow`
attribute added to silence a lint you introduced.

The exit scripts are separate because they need a built binary and, in some phases, a machine
with a package manager:

```bash
bash scripts/p3-exit-test.sh
bash scripts/p5-exit-test.sh
bash scripts/p6-exit-test.sh
bash scripts/p7-exit-test.sh
```

## Layout

| Path | What belongs there |
|---|---|
| `crates/rigel-core/src` | Storage, sessions, budget, patches, claims, attribution, tools, updates. No UI, no Tauri. |
| `crates/rigel-core/tests` | Behavioural tests over real SQLite files and real temporary projects. |
| `crates/rigel-adapters/src` | One module per harness protocol. Nothing in it decides permissions. |
| `crates/rigel-mcp/src` | The stdio tool host injected into agents. |
| `crates/rigel-cli/src` | One module per command group; every command has `--json`. |
| `crates/rigel-app/src` | Tauri commands. Thin: they call core and serialise. |
| `ui/src` | Renderer. It never parses human output from the CLI. |
| `migrations/` | Numbered `.sql`, one transaction each, with a header saying why. |

## Changing the database

A schema change is a new file in `migrations/{index,project}/` plus a `Migration` entry in
`migrate.rs`. Never edit an applied migration: `apply` refuses a renamed version on purpose. The
new file starts with a comment explaining what changes and why; the ones already there are the
model.

## Adding a tool to the registry

`crates/rigel-core/src/toolchain.rs` holds a `TOOLS` entry per tool with its binary, aliases, docs,
publisher, install package per manager and a rough install count. Add the package ids you have
actually checked; leave the others `None` rather than guessing. The unit test at the bottom of the
file fails if an entry has no package anywhere.

Trust weights live in `WEIGHTS` with a sentence each. Changing one changes what
`rigel trust explain` prints for every package, so change the pinned arithmetic test in the same
commit.

## Style

- Comments explain **why**, not what. If a line needs a "what" comment, rename something.
- No new dependency for something the standard library does in twenty lines. If a dependency is
  genuinely needed (a protocol, a crypto primitive), say why in the Cargo.toml comment and in the
  pull request.
- Error messages are sentences a user can act on, and they name the thing that failed.
- The UI never says a file is "locked". The claim ledger is advisory and the wording is a
  non-negotiable (N1). There is a string test for it in `scripts/p5-exit-test.sh`.

## Commits

One phase item or one coherent change per commit. The subject says what changed; the body says why
it had to. If an item is ticked in `docs/handoff.md`, the tick belongs in the same commit, with the
evidence — file names, test names, the command that was run. Tick only what you ran: an exit test
ticked from a machine that did not run it is worse than an unticked one.

## Security

Read [SECURITY.md](SECURITY.md) before touching credentials, approvals, path checks or the update
path. Those four are where a small mistake is a vulnerability rather than a bug.

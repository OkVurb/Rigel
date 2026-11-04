# Rigel b

Rigel b is Rigel's fork of [Obscura](https://github.com/h4ckf0r0day/obscura), the headless browser
used by every agent in the workspace. The name is the planet in Rigel's system: one engine, one
identity, shipped beside the app instead of installed separately.

## What this fork changes

- The shipped CLI and worker are `rigel-b` and `rigel-b-worker`.
- `rigel-b mcp` accepts `--capture <FILE>`. After every successful page action it writes the
  active tab's exact PNG and a small JSON metadata file before replying, so Rigel's inspector can
  show the frame the harness actually produced rather than re-rendering the URL.
- `rigel-b mcp --space <NAME>` gives each agent an isolated Space. Tabs, cookies and web storage
  are scoped to that process and name; another Space cannot list, switch or close its tabs.
  `browser_space` reports the boundary and its background-only focus policy.
- `browser_run` executes up to 50 navigate/click/fill/type/wait/evaluate/tab actions in one MCP
  call. It is the compact code-style surface, without a round trip per action.
- `browser_snapshot` includes text from open shadow roots and live iframe documents. Shadow
  controls are reachable through `browser_evaluate` or `browser_run`; element refs remain scoped
  to the top-level document.
- MCP input is synthetic and page-scoped. The fork creates no native browser window, never moves
  the OS pointer, never injects system keyboard input, and cannot focus or switch a user tab.
- The release binary is bundled as a Tauri sidecar and resolved next to the app executable.
- The MCP server keeps the `obscura` config name so existing agent configs and tool names keep
  working while the binary and product are Rigel b. Rigel passes a session/agent-derived
  `--space` when it launches the server.

## Upstream

Forked from <https://github.com/h4ckf0r0day/obscura>. The upstream `LICENSE` is retained
verbatim. Obscura's documentation under `docs/` describes the engine and remains applicable;
references to `obscura` binaries in upstream docs map to `rigel-b` here.

The upstream repository is Apache-2.0. The Rigel application bundles a copy of the license in
`resources/rigel-b/LICENSE`.

## Rebuild the sidecar

From the Rigel repository root:

```bash
cargo build --release --manifest-path vendor/rigel-b/Cargo.toml \
  --target-dir vendor/rigel-b/target -p rigel-b --features render
cp vendor/rigel-b/target/release/rigel-b.exe \
  crates/rigel-app/binaries/rigel-b-x86_64-pc-windows-msvc.exe
```

Tauri's `externalBin` entry points at `binaries/rigel-b`; the app resolves the copied
`rigel-b.exe` beside its own executable.

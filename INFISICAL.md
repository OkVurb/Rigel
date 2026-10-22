# Secrets

Secrets manager in use: none

Rigel reads its configuration from the process environment — `std::env::var` in
`crates/rigel-core`, `crates/rigel-adapters` and `crates/rigel-app`, plus `process.env` in the
`integrations/` bridge. Nothing in the app reads a `.env` file itself, so a `.env` only works
because the shell loaded it. `infisical run --env=dev` puts the same names in the environment
before the process starts, and no application code changes.

## Local dev commands

| What starts | Command | Where |
|---|---|---|
| The app (Tauri 2 desktop shell; builds `ui/dist` and starts the renderer) | `bash scripts/dev.sh` → `infisical run --env=dev -- cargo run -p rigel-app` | `scripts/dev.sh` |
| The renderer alone, in a browser | `cd ui` then `npm run dev` | `ui/package.json` |
| The CLI | `cargo run -q -p rigel-cli -- <command>` | workspace root |
| The prototype (Phase 0 browser app, historical) | `cd prototype` then `npm run dev` | `prototype/package.json` |

Production entry points stay outside `infisical run`: `npm run build` in `ui/` and
`prototype/`, `cargo build --workspace`, a release `cargo run -p rigel-app`, and
`.github/workflows/ci.yml`. They need a machine identity, not the dev CLI (see Finish setup).

## Variables the app reads

Paths and switches (not secrets):

- `RIGEL_INDEX_DIR` — where `index.db` and `blobs/` live; defaults to the per-user app-data directory (`%APPDATA%\Rigel`, `~/.local/share/rigel`).
- `RIGEL_PROVIDER` — the native agent's default provider: `omniroute` (default), `openrouter` or `gemini`.
- `RIGEL_OMNIROUTE_URL` — base URL of the local OmniRoute router; defaults to `http://127.0.0.1:20128/v1`.
- `RIGEL_GEMINI_BASE_URL` — Gemini API base; defaults to `https://generativelanguage.googleapis.com/v1beta/openai`.
- `RIGEL_GITHUB_TOKEN` — token for `rigel skills index`; raises GitHub's quota from 60 to 5,000 requests/hour.
- `GITHUB_TOKEN` — fallback for `RIGEL_GITHUB_TOKEN`.
- `RIGEL_HYPER_CONFIG` — where the Hyper bridge (`integrations/hyper-rigel`) reads the token file it was handed; the path, not the token.

Provider and harness keys (secrets; the environment is read first, then an OS keychain entry
written by `rigel keys set`, so `infisical run` overrides a stored key for one process):

- `OMNIROUTE_API_KEY` — the `omniroute` provider, the key the local router itself checks.
- `OPENROUTER_API_KEY` — the `openrouter` provider.
- `GEMINI_API_KEY` — the `gemini` provider (Google AI Studio), used as `Authorization: Bearer`.
- `OPENAI_API_KEY` — the `openai` provider, and the key Codex receives.
- `ANTHROPIC_API_KEY` — the `anthropic` provider, and the key Claude Code receives.
- `CURSOR_API_KEY` — Cursor's own key, injected into the ACP child process.
- `CLINE_API_KEY` — Cline's own key, injected into the ACP child process.

`.env.example` lists every name above with a placeholder. `.gitignore` ignores `.env` and
`.env.*`, with an exception for `.env.example`.

## Finish setup

Create the project first (one project for this service, with the Development, Staging and
Production environments every new project starts with; `--env=dev` is Development), then run
these in this folder. One command per line, no `&&`: PowerShell 5.1 does not have it.

1. Create the project at https://app.infisical.com — name it Rigel.
2. Install the CLI: macOS `brew install infisical/get-cli/infisical`; Windows `scoop bucket add org https://github.com/Infisical/scoop-infisical.git` then `scoop install infisical`; any platform `npm install -g @infisical/cli`.
3. `infisical login`
4. `infisical init` — links this folder to the project and writes `.infisical.json` (the project id only, no secrets; commit it).
5. Import the existing keys: open the project's **Secrets Overview** page, select **Development**, and drag your `.env` onto the page. Every key lands in the environment at once, values included.
6. `bash scripts/dev.sh` — the desktop shell starts through `infisical run --env=dev`.
7. Move `.env` **out** of this repository folder, to your home directory or a password manager. Do not rename it to `.env.backup` next to the code: git would see that file.
8. `bash scripts/dev.sh` again — the app now reads the same values from Infisical, with no `.env` left to mask a key that was never imported.

For CI and production, the dev CLI is the wrong tool: create a machine identity with Universal
Auth scoped to the environment, keep `INFISICAL_UNIVERSAL_AUTH_CLIENT_ID` and
`INFISICAL_UNIVERSAL_AUTH_CLIENT_SECRET` in the platform's secret store, and authenticate
non-interactively (`infisical login --method=universal-auth --client-id=… --client-secret=…`)
before the production command. `.github/workflows/ci.yml` reads no secrets today.

A secret that was ever committed stays in git history and must be rotated, not just deleted:
rotate it at the provider, then update it in Infisical. `infisical scan` finds leaked secrets in
the working tree and the history.

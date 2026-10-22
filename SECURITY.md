# Security

Rigel runs coding agents against a real working tree, holds provider keys, and installs
dependencies. This document is the threat model it is built against, the boundaries it enforces,
and the ones it deliberately does not.

## Reporting

Email security@rigel.invalid with a description, the version (`rigel --version`) and, if you have
one, a proof of concept. Please do not open a public issue first. We aim to acknowledge within
three working days and to ship a fix or a written mitigation within thirty.

There is no bug bounty. There is no telemetry, so we cannot see an incident unless you tell us.

## Supported versions

The latest minor release. `main` is development; a bug that only exists there is still worth
reporting.

## Boundaries

| Boundary | What Rigel enforces | Where |
|---|---|---|
| Credentials | One OS-keychain entry per provider and per harness. Never in config, never in a project file, redacted as `Secret` in logs and `Debug`. | `credentials.rs` |
| Agent environment | Provider keys, `RIGEL_GITHUB_TOKEN`, `GITHUB_TOKEN` and `INFISICAL_TOKEN` are removed from every child process Rigel spawns. | `agent/tools.rs` |
| Paths | Every write resolves under the project root; `..`, absolute paths and symlink escapes are refused before a byte is written. | `patches.rs` |
| Patches | Compare-and-swap on `base_hash`; a stale base writes nothing and returns `STALE_BASE`. Binary files are refused. | `patches.rs` |
| Commands | The exact text, working directory and timeout are shown before approval. The shell sees the same string the user approved. Output is capped, the process is killed at its timeout, and children are killed with it. | `agent/tools.rs`, `approval.rs` |
| Destructive commands | A deterministic classifier marks the operation and requires approval plus a snapshot of uncommitted work. Advisor agents are advisory and cannot allow anything. | `agent/destructive.rs` |
| Network | `fetch_url` requires per-origin approval with `once` / `session` / `always` scope, follows redirects only within an approved origin, reads text-like types, and caps the body at 256 KB. Every result is labelled `untrusted: true`. | `agent/egress.rs` |
| MCP tool host | Scoped to one `session` and one `agent`; a tool call cannot name another agent's files or budget. | `rigel-mcp` |
| Skills | A skill body is data. Loading one is visible in the transcript and grants no permission. | `skills/` |
| Memory | Committed files pass a secret and absolute-path validator before landing in the tree. | `memory.rs` |
| Updates | Off by default. A manifest is Ed25519-verified over `version\nurl\nsha256` before it is read, and the artifact's SHA-256 must match before it is offered. | `update.rs` |

## Network calls, exhaustively

- The local OmniRoute router on loopback (the native agent), which fans out to providers.
- The selected provider directly (OpenRouter, Gemini) when configured that way.
- `api.github.com` and `raw.githubusercontent.com` during `rigel skills index`, on demand.
- SkillsMP and SkillHub during `rigel skills sync`, on demand.
- The update manifest, only when `rigel update check` runs and only after the switch is on.

Nothing else. `rigel doctor` and `rigel tools list` spawn local processes and read local files;
they make no requests. The Phase 7 exit script runs them behind a dead proxy to keep that true.

## Secrets in a project

Rigel writes `<project>/.rigel/` and a `.gitignore` inside it on first use, so session logs cannot
be committed by accident. Memory files are the only Rigel-authored content that belongs in the
repository, and they are scanned for key-shaped strings before they are written.

If you find a key in a skill body or a fetched page, treat it as compromised: Rigel does not
redact third-party content, only its own copies of your credentials.

## Prompt injection

Repository files, skill bodies, fetched pages and agent output are **data**. No agent output is
executed as a Rigel command, and approvals are bound to a request Rigel itself created, not to a
line in a transcript. An agent that reads a malicious page can be persuaded to *ask* for something;
it cannot grant itself permission, because the gate is code and the answer comes from the user.

## What is not defended against

- **A malicious local process.** Rigel is not a sandbox. Any process running as the same user can
  read `.rigel/rigel.db` and the keychain entries it is allowed to read.
- **A compromised publisher key.** Signature verification proves the manifest came from whoever
  holds the key, not that they are honest.
- **A malicious package manager.** Rigel shows the command and records the exit; it does not
  reimplement winget's, Chocolatey's or npm's verification. `install_ledger.verification` states
  what the manager checks.
- **An OS-level attacker with debugger rights.** Nothing user-mode survives that.

## Unsigned builds

Release installers are Authenticode-signed when a certificate is configured; see
[docs/PLATFORMS.md](docs/PLATFORMS.md). A build without a signature is named as such in the release
notes. Rigel never disables a Windows security feature to make an install succeed, and it never
runs `sudo`, `doas` or `pkexec` — an install that needs elevation prints the command and stops.

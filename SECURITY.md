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
| Updates | On by default. One signed manifest check runs at launch (and on demand); Settings → Updates turns the check or auto-install off. The manifest is Ed25519-verified over `version\nurl\nsha256` (plus the optional MSI fields) before it is read, and the downloaded artifact's SHA-256 must match before anything runs. | `update.rs` |
| Mail | Opt-in and off until configured. The inbox address lives in `agentmail.json`; the AgentMail API key lives in the OS keychain (`rigel/agentmail`) and never reaches the renderer. Reading needs no confirmation. **Sending is a card first** — recipient, subject and body — and silence is a refusal; a read-only subagent is not offered `email_send` or `email_reply` at all. A reply is addressed to the message's own sender and carries its subject, so it cannot be redirected to a third party. Every message read or listed is returned labelled `untrusted: true`. | `agentmail.rs`, `agent/tools.rs`, `approval.rs` |
| Cloud | Opt-in and off until configured. The project id and public web API key live in `cloud.json`; the refresh token and the email-verification answer live in the OS keychain and never reach the renderer. Email/password sign-in, sign-up and verification go through Identity Toolkit; a password-reset answer is the same for known and unknown addresses, so the page is not an account oracle. Verification is shown, not enforced: `firestore.rules` checks the uid, not the `email_verified` claim. Firestore rules restrict every document to its owner. **App Check is optional and off until configured:** the attestation URL lives in `cloud.json` (not a secret), its tokens are bearer credentials held in memory only and never serialized, printed or sent to the renderer, and a configured-but-unreachable endpoint fails the request instead of sending it un-attested. The endpoint is the security boundary — an attestation service that mints tokens for anyone adds no protection. Shelf metadata — including deletion tombstones — is uploaded only on **Push shelf**; the local manifest in `cloud-state.json` holds ids and timestamps, never bodies; a session's prompt and assistant text are uploaded only on **Publish to phone** or `rigel cloud publish`, and tool calls, command output, file changes and the local project path are never part of that shape. | `cloud.rs`, `firestore.rules` |

## Network calls, exhaustively

- The local OmniRoute router on loopback (the native agent), which fans out to providers.
- The selected provider directly — the adapter table's own base URL (OpenRouter, Google AI Studio, Vertex, OpenAI, xAI, Groq, DeepSeek, Mistral, Together, Fireworks) when configured that way.
- `api.github.com` and `raw.githubusercontent.com` during `rigel skills index`, on demand.
- SkillsMP and SkillHub during `rigel skills sync`, on demand.
- The latest GitHub release manifest (`github.com/OkVurb/Rigel/releases/latest/download/latest.json`) and its installer assets. A check runs once at launch and on demand; the installer is downloaded only for an available, signed update. Turning the update switch off stops both.
- Firebase Authentication (`identitytoolkit.googleapis.com`, `securetoken.googleapis.com`) and Firestore (`firestore.googleapis.com`) **only after** the user configures a project and signs in, and only from `rigel cloud` or Settings → Cloud, and from the iPhone client's Rigel Cloud screen. A signed-out install makes no cloud request. The refresh token is in the OS keychain (or the phone's); the password is used for one request and never stored. When App Check is configured, every one of those requests also carries a token fetched from the user's own attestation endpoint, whose URL is the only thing stored.
- AgentMail (`api.agentmail.to`) **only after** the user saves an inbox and a key, and only when mail is actually used: `email_list`, `email_read`, `email_send`, or the Settings → Email page. Nothing is polled and no mailbox is watched. A message, or one conversation in full, is fetched only when it is asked for, and no body is written to the project database.

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
- **A stolen Firebase refresh token.** Signing out deletes the local copy but does not call
  Firebase's revoke endpoint. A machine that is lost should have its refresh tokens revoked in the
  Firebase console (Authentication → Users → Revoke refresh tokens), which invalidates the copy
  before it can be used.
- **Public sign-up on the user's project.** Once Email/Password is enabled, the project's public
  web API key can create accounts from `rigel cloud sign-up` — or any other client. Rigel does not
  enforce a registration policy the Firebase project does not; App Check (which Rigel can send
  tokens for), a blocking function, or keeping sign-up closed is the project owner's decision.
- **An OS-level attacker with debugger rights.** Nothing user-mode survives that.

## Unsigned builds

Release installers are Authenticode-signed when a certificate is configured, and so is the payload
inside them; see [docs/PLATFORMS.md](docs/PLATFORMS.md). The Authenticode certificate and the
Ed25519 key that signs the update manifest are different keypairs with different jobs: Windows
checks the first, `rigel update verify` checks the second, and neither authorises the other. A build
without a signature is named as such in the release notes. Rigel never disables a Windows security feature to make an install succeed, and it never
runs `sudo`, `doas` or `pkexec` — an install that needs elevation prints the command and stops.

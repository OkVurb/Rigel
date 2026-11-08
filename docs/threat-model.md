# Threat model (roadmap 79)

Scope: the Rigel desktop app and its native agent. Reviewed 2026-11-06 against the code in this
tree. The local-first data inventory is in [privacy.md](privacy.md); the vulnerability route is in
[SECURITY.md](../SECURITY.md).

## Assets

- The user's source tree and the contents of `.rigel/rigel.db` (prompts, tool output, patches).
- Provider, harness, Firebase, AgentMail and MCP credentials.
- The user's provider spend and the integrity of commands the agent runs.
- The user's cloud-published session documents.

## 1. Someone with the user's OS account, or a stolen unlocked machine

**Mitigation.** Provider keys, the Firebase refresh token, the App Check client token and the
AgentMail key are stored in the OS keychain, not in `settings.toml` or `cloud.json`. The local
database is readable only through the user's account. Signing out removes the local cloud
credentials, and account deletion is a typed confirmation with an export offered first.

**Non-goal: defending against an administrator or kernel-level attacker.** Rigel runs as an
ordinary desktop process and cannot protect memory or the keychain from the account that owns
them. The database is also not encrypted at rest; [privacy.md](privacy.md) says so plainly, and
full-disk encryption remains the user's control.

## 2. A malicious repository the user opens

**Mitigation.** File tools resolve through the patch pipeline's jail, which refuses `..` and
symlink escapes. Repository hooks are not run automatically, and a project MCP configuration is
shown and approved before its server is started. Skill scripts are surfaced as script approvals
with the path the user can inspect.

**Non-goal: treating repository text as trusted.** `AGENTS.md`, README files, issue text and tool
output can all contain instructions aimed at the model. Rigel does not claim to classify them
reliably. The boundary is that model text cannot approve a command, write outside the jail, or
spend without the same gates a user-initiated turn uses.

## 3. A compromised or malicious model provider

**Mitigation.** A provider response is data. Every command, write, network fetch, mail send and
skill script still passes its own approval or policy gate, and the provider has no way to answer
an approval. A cancelled stream is billed at most once and closes as cancelled.

**Non-goal: content-filtering provider output for prompt injection.** Instructions and the data
they refer to arrive on one channel, and a filter that removes "ignore previous instructions"
also removes legitimate code and prose. The reason for the non-goal is that the enforceable
boundary is the tool gate, not a classifier. Residual risk: a user who approves a command the
model composed can still run it.

## 4. A malicious MCP server or MCP tool

**Mitigation.** Adding a project server requires seeing its real `tools/list` before approval.
Each tool call is presented with its arguments and is approved according to the same scopes as a
native call. A server that fails its handshake produces an error rather than a silent empty tool
list.

**Non-goal: sandboxing a stdio MCP server.** The server is a child process running as the user,
so it can do anything the user can. There is no OS sandbox in this release; the reason is that a
portable Windows sandbox is not part of the current runtime. Users should only approve servers
whose code they trust.

## 5. A stolen cloud refresh token

**Mitigation.** Cloud is opt-in and runs against the user's own Firebase project. Firestore rules
scope documents to the authenticated user id, so a token for one account cannot read another
account's shelf. Sign-out clears the local credential; deleting a shelf row writes a tombstone;
account deletion is explicit and offers export first.

**Non-goal: revoking a token after it has left the machine.** Revocation is a Firebase project
operation, not something the desktop app can perform offline. The project owner can rotate or
disable the account in the Firebase console, and provider-side session revocation remains the
project's control.

## Residual risks

- The database is not encrypted, and backups are the user's responsibility.
- A user can approve a destructive command; approvals show exact text, cwd and timeout, but do
  not judge intent.
- Third-party providers, Firebase, AgentMail and MCP servers apply their own retention and
  security policies.

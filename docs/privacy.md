# Privacy

Rigel is a local-first desktop app. This note describes what 0.1.7 actually does; the threat model
is in [SECURITY.md](../SECURITY.md). A per-call receipt with purposes and retention is in
[cloud-privacy.md](cloud-privacy.md).

## What stays on this machine

- `.rigel/rigel.db` holds prompts, turns, tool calls, command output, patches, spend records,
  approvals and session metadata. It is ordinary SQLite, readable by your Windows account, and is
  not encrypted at rest.
- `settings.toml`, `cloud.json` and `agentmail.json` hold preferences and non-secret configuration.
- Provider keys, the Firebase refresh token, the App Check client token and the AgentMail key live
  in the OS keychain.
- Rigel sends no telemetry, analytics or crash reports in 0.1.7. Diagnostics are assembled locally
  and leave only when you copy or export them.

## What leaves the machine

- **Model providers.** A turn sends the prompt, the conversation context and tool results to the
  provider you selected. Its own terms and retention policy apply.
- **OmniRoute.** The local router runs on loopback and forwards to whichever provider it is
  configured to use.
- **Skills.** `rigel skills index` reads GitHub on demand; `rigel skills sync` reads SkillsMP and
  SkillHub on demand.
- **Updates.** With the update switch on, Rigel requests the latest GitHub release manifest once at
  launch and on demand. Auto-install downloads a verified installer only when an update is offered;
  the installer is started only after its SHA-256 matches the signed manifest.
- **Cloud.** Cloud is opt-in. After you configure a Firebase project, Rigel uses Identity Toolkit
  and Firestore for sign-in and sync. A session's prompts and assistant text are published only on
  **Publish to phone**, or after each turn if automatic publishing is on. Tool calls, command
  output, file changes and the project path are not part of that document.
- **App Check.** If configured, requests to Firebase carry a token from your endpoint. The endpoint
  URL is not secret; the per-install client token is in the OS keychain.
- **AgentMail.** Only after you save an inbox and key, and only when mail is used. Reading needs no
  confirmation; sending is a card and silence is a refusal. No mailbox is polled.
- **GitHub.** PR actions and forge reads happen only when you invoke them.
- **MCP servers.** A configured server is contacted only when its tool is used; the tool arguments
  and the result it returns go to that server, whether it is a local child process or a remote
  endpoint. The server operator's retention policy applies.

## Retention and deletion

Local sessions stay until you archive or delete them. Archiving never destroys rows. Cloud rows are
yours: deleting a shelf row leaves a tombstone, and account deletion is an explicit, typed-confirm
action with an export offered first. Firebase's own backups and retention are controlled by the
Firebase project you configured.

## Your controls

- Turn the update check or auto-install off in Settings → Updates.
- Keep Cloud off, sign out, or delete the local credentials.
- Remove provider and harness keys from Settings.
- Use **Export diagnostics** only when you choose to share the bundle.
- Back up `.rigel/` yourself before deleting a project; 0.1.7 does not yet keep rolling database
  backups.

## Contact

For a privacy question, open a GitHub Discussion. For a vulnerability, use the private route in
[SECURITY.md](../SECURITY.md).

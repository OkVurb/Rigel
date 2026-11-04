# Grilling — round 9 (Q216–Q335): the 0.1.7 release and the quality cut

Asked 4 Nov 2026, after 0.1.6 and the Advanced-settings repair. The round is deliberately larger
than the previous ones: 120 questions in six batches of twenty, covering the release contract, the
agent runtime, sessions, the window, cloud and integrations, and the release gate.

**Answered 4 Nov 2026.** The answers below are the decision record. Q330 overrides the first
answer's broad scope: **0.1.7 is a small release**, carrying the settings repair, release
hardening, and only the low-risk work that can be verified now. Substantial accepted work is
listed in [roadmap-0.2.md](roadmap-0.2.md), not forgotten and not claimed as shipped.

The `0.1.7` column says what this release does. `0.2` means the decision is accepted but deferred
under Q330. `kept` means the answer matches behavior already in 0.1.6 and needs no new code.

## A. Release contract (Q216–Q235)

| # | Decision | 0.1.7 |
|---|---|---|
| Q216 | Feature patch: include accepted work that can land safely, even with a migration. Q330 later narrows the release to a small cut. | Q330 overrides |
| Q217 | Publish a public GitHub beta marked unsigned, with SHA-256 hashes and a SmartScreen warning. | yes |
| Q218 | The Ed25519 manifest plus SHA-256 is sufficient for this release; no Authenticode certificate is required. | yes |
| Q219 | Tag `v0.1.7` at the release commit and consolidate all changes since `v0.1.4` in the notes. | yes |
| Q220 | Publish MSI and NSIS x64 installers, both branded and hash-published. | yes |
| Q221 | Check for updates and auto-install by default. | yes |
| Q222 | Auto-install after download; show the one-click offer only when auto-install is off. | yes |
| Q223 | Run the Windows Sandbox install/start/doctor/uninstall pass; record 1809 and SmartScreen as manual gaps. | yes |
| Q224 | Keep Windows 10 1809+ as the supported floor. | yes |
| Q225 | Write curated user-facing notes with the full commit list linked. | yes |
| Q226 | Keep the answers in this file, link them from `handoff.md`, and keep the release checklist in `docs/release-checklist.md` and `CHANGELOG.md`. | yes |
| Q227 | Require all current CI gates plus focused regression tests for every changed area. | yes |
| Q228 | Keep the v0.1.6 artifacts and document manual rollback; no automatic rollback. | yes |
| Q229 | Install per-user by default and preserve app data across upgrade and uninstall. | yes |
| Q230 | Send no telemetry; diagnostics stay local. | yes |
| Q231 | Release a patch when a meaningful fix lands; no calendar train. | yes |
| Q232 | Support Windows 10 1809+ and the latest release only. | yes |
| Q233 | Add `SECURITY.md` and enable GitHub private vulnerability reporting. | yes |
| Q234 | Generate an SBOM and run Cargo and npm audits before tagging. | yes |
| Q235 | The agent runs the version bump, tag, push, workflow, and receipt review. | yes |

## B. Agent runtime and trust (Q236–Q255)

| # | Decision | 0.1.7 |
|---|---|---|
| Q236 | Auto-install is the default; the one-click offer is for users who turn auto-install off. | yes |
| Q237 | Summarize older steps at 80% of the context window, keep pinned files and recent turns, and show what was compacted. | 0.2 |
| Q238 | Retry a transient provider failure twice, then offer a one-click fallback to the next healthy provider. | 0.2 |
| Q239 | Before destructive approvals, show resolved target paths and counts where provable, and "cannot determine" elsewhere. | 0.2 |
| Q240 | Add a per-session network kill switch and a project allowlist while keeping per-origin prompts. | 0.2 |
| Q241 | Redact known credential patterns from tool output, logs, and diagnostics, and never echo keys. | 0.2 |
| Q242 | Show each MCP server's tools and require a per-project approval before first use, remembered per project. | 0.2 |
| Q243 | Require approval before a skill's scripts run and mark untrusted tool output. | 0.2 |
| Q244 | Validate a newly saved provider key with a lightweight roster/models call. | 0.2 |
| Q245 | Give every subagent its own session cap. | 0.2 |
| Q246 | Keep the ten-minute approval refusal, with a countdown and a Retry action. | 0.2 |
| Q247 | Default `run_command` timeout is 200 seconds, configurable per command. | 0.2 |
| Q248 | Mark partial provider output as incomplete and bill actual usage. | 0.2 |
| Q249 | At the step limit, pause and offer Continue with a fresh step budget. | 0.2 |
| Q250 | Show tokens, cost, and duration per step in a collapsed details row. | 0.2 |
| Q251 | Require explicit approval before writing `MEMORY.md` or a memory topic. | 0.2 |
| Q252 | Build an on-demand file and symbol map with a token cap, refreshed by the watcher. | 0.2 |
| Q253 | Defer first-class local-model support to 0.2. | 0.2 |
| Q254 | Pause at the next step boundary and resume the same turn. | 0.2 |
| Q255 | No side-by-side agent race in 0.1.7; keep one writer per file. | 0.2 |

## C. Sessions and the workspace (Q256–Q275)

| # | Decision | 0.1.7 |
|---|---|---|
| Q256 | Fork from a chosen turn into a new session with the transcript prefix, parent link, and a new model choice. | 0.2 |
| Q257 | Add nullable per-project model, effort, and step defaults with an inherit state; the turn choice still wins. | 0.2 |
| Q258 | Import Claude Code and Codex history as full, owned, resumable sessions. | 0.2 |
| Q259 | Keep sessions until the user archives or deletes them; archive never destroys. | kept |
| Q260 | Export redacted Markdown plus JSON under app data. | 0.2 |
| Q261 | Show forked sessions as a parent/child tree with the branch point named. | 0.2 |
| Q262 | Let a cheap model suggest a session title after the first turn. | 0.2 |
| Q263 | After a crash, offer to reopen the last session and explain what was recovered. | 0.2 |
| Q264 | Keep the shared project tree and file claims; no per-session worktree by default. | kept |
| Q265 | Allow each agent to pick and edit its own model, defaulting to the session model. | 0.2 |
| Q266 | Add "Hand to…" with a prefilled prompt naming files and outcome; sending still needs a click. | 0.2 |
| Q267 | Show held and stale claim counts per agent in the roster, linking to Diff. | 0.2 |
| Q268 | Add a Run tests action with parsed pass/fail counts and a link to output. | 0.2 |
| Q269 | `AGENTS.md` always wins over `CLAUDE.md`. | 0.2 |
| Q270 | Pin skills per session, add a skill-pin surface, and include pins in session templates. | 0.2 |
| Q271 | Inline images for vision-capable models; the user chose no size cap, which needs a separate memory/abuse review. | 0.2 |
| Q272 | Auto-prune merged and expired subagent worktrees after a visible delay; keep unmerged ones. | 0.2 |
| Q273 | Keep three rolling daily database backups. | 0.2 |
| Q274 | Archive first; permanent deletion needs typed confirmation and removes local rows. | 0.2 |
| Q275 | Keep one window and add a taskbar badge for running and attention counts. | 0.2 |

## D. The window (Q276–Q295)

| # | Decision | 0.1.7 |
|---|---|---|
| Q276 | First launch offers a skippable three-step setup: project folder, provider key, theme. | 0.2 |
| Q277 | Keep dark as the default and add a Follow system theme option. | 0.2 |
| Q278 | Add fully rebindable keyboard shortcuts. | 0.2 |
| Q279 | Add an All / Tools / Diffs / Errors / Yours transcript filter, remembered per session. | 0.2 |
| Q280 | Render Markdown with syntax highlighting and per-block copy. | 0.2 |
| Q281 | Let typing during a turn steer that turn immediately. | 0.2 |
| Q282 | Add a full slash-command language with aliases. | 0.2 |
| Q283 | Add per-event notification routing, quiet hours, and click-to-open-turn. | 0.2 |
| Q284 | Tray panel: live metrics, attention items, pause all, open, quit. | 0.2 |
| Q285 | Add an automatic crash reporter. | 0.2 |
| Q286 | Empty surfaces show a contextual explanation and one next action. | 0.2 |
| Q287 | Model picker remembers favorites and recents and shows a relative cost hint. | 0.2 |
| Q288 | Inspector: overview, changed files, spend, pinned skills, and notes. | 0.2 |
| Q289 | Add focus mode, hiding rail, list, and inspector. | 0.2 |
| Q290 | Add an explicit high-contrast toggle in Appearance. | 0.2 |
| Q291 | Build full internationalization now. | 0.2 |
| Q292 | Drag-and-drop attaches in place by path, with preview and no copy. | 0.2 |
| Q293 | Add word-level diff highlighting and collapse unchanged lines. | 0.2 |
| Q294 | Settings search indexes every control and deep-links to it. | 0.2 |
| Q295 | Put release notes in a separate About dialog. | 0.2 |

## E. Cloud, data, and integrations (Q296–Q315)

| # | Decision | 0.1.7 |
|---|---|---|
| Q296 | Enable Cloud by default with a hosted project. This requires an operator-owned Firebase project and a new privacy review. | 0.2 |
| Q297 | Offer a complete export, then delete cloud rows and local credentials after typed confirmation. | 0.2 |
| Q298 | Detect sync conflicts by version, show both versions, and never silently overwrite. | 0.2 |
| Q299 | Publish prompts and assistant text by default; make tool calls, output, and diffs explicit opt-ins. | 0.2 |
| Q300 | Require App Check for every Cloud request. | 0.2 |
| Q301 | Run Rigel-sponsored cloud agent runs inside a fixed budget. | 0.2 |
| Q302 | Let the iPhone send prompts and approve commands remotely. | 0.2 |
| Q303 | Full two-way GitHub sync of PR state and checks. | 0.2 |
| Q304 | Build a generic forge interface now. | 0.2 |
| Q305 | Keep AgentMail optional: list, read, and approval-gated send only. | kept |
| Q306 | Add a Linear connector. | 0.2 |
| Q307 | Keep the curated MCP catalogue plus local project config, with a visible origin on every server. | kept |
| Q308 | Use SQLCipher for the whole database. | 0.2 |
| Q309 | No end-to-end encryption for cloud session text; document Firebase rules and App Check. | kept |
| Q310 | User-controlled retention; no automatic deletion; explicit deletes leave tombstones. | kept |
| Q311 | Honor the system proxy and allow explicit `HTTP(S)_PROXY` overrides. | 0.2 |
| Q312 | All local features work offline; Cloud actions fail with a clear message. | kept |
| Q313 | Add Google and GitHub OAuth. | 0.2 |
| Q314 | Use the OS keychain exclusively; no app-level vault. | kept |
| Q315 | Publish a full Terms of Service and privacy policy. | 0.2; beta terms in `docs/terms.md` |

## F. Quality and release gate (Q316–Q335)

| # | Decision | 0.1.7 |
|---|---|---|
| Q316 | No numeric coverage target; every changed behavior gets a regression test and critical paths stay covered. | yes |
| Q317 | Automate the full Tauri window with a native driver. | 0.2 |
| Q318 | No CI time budget; correctness only. | yes |
| Q319 | Add fuzz/property tests for parsers, path handling, and migration input. | 0.2 |
| Q320 | Back up the database, test forward migrations on a copy, and refuse unknown schema versions. | 0.2 |
| Q321 | Startup under 2 seconds and common interactions under 100 ms on this machine; record the measurements. | yes |
| Q322 | Upload logs automatically on crash. | 0.2 |
| Q323 | Lockfiles, Cargo/npm audits, and a written reason for every new runtime dependency. | yes |
| Q324 | Allow MIT/Apache/BSD, review copyleft, and publish the SBOM. | yes |
| Q325 | Keep README, PLATFORMS, what-works, round 9, CHANGELOG, and SECURITY.md current. | yes |
| Q326 | Use a hard release checklist covering CI, branding, install, update, checksums, notes, and rollback. | yes |
| Q327 | Strict SemVer; every release gets a `v` tag and tags are never skipped. | yes |
| Q328 | Use GitHub Issues with templates and Discussions for questions. | yes |
| Q329 | Publish the deferred round-9 work as a 0.2 roadmap. | yes |
| Q330 | Cut a smaller 0.1.7 and move substantial work to 0.2. | yes |
| Q331 | The agent releases automatically once every checklist gate passes and posts the receipt. | yes |
| Q332 | Freeze code after the release checklist is green; only release-blocking fixes afterward. | yes |
| Q333 | Publish release notes immediately after generation. | yes |
| Q334 | Build twice and compare artifact hashes where the toolchain permits. | yes |
| Q335 | Install the published MSI, verify the 0.1.6 update path, and check hashes before announcing. | yes |

## The 0.1.7 cut

**Shipping:** the Settings repair from this session, curated release notes, the release checklist,
`SECURITY.md`, privacy/beta terms, the SBOM generator, audit results, and the existing local-first
Cloud/MCP/AgentMail behavior.

**Not shipping in 0.1.7:** every row marked `0.2`. The reasons are on the roadmap. The most
important one is that a public unsigned beta should not also carry an untested hosted-cloud
migration, a new encryption layer, a native-driver test harness, or remote approvals.

Back to the [handoff](handoff.md) · [what works](what-works.md) · [0.2 roadmap](roadmap-0.2.md).

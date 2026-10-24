# What works

This is the long version of the README's summary: every surface in the window and the CLI, its
state, and the reason to believe it. Anything deliberately unfinished is listed as such.

| Surface | State |
|---|---|
| Shelf: search, rows, detail, evidence table, variants | **real**: SQLite FTS5 over `index.db`, offline, `skills.search`/`skills.load` over IPC |
| Shelf: static review | **real**: six capability rules with `file:line` evidence, grade from the same scan the prototype used |
| Shelf: source health | **real**: `ok` / `stale` (with last-sync date) / `broken`, read without a network call |
| Shelf: empty state | offers "Index repository" when no source has ever synced |
| GitHub source | **real**: parse `owner/repo` or any GitHub URL, one tree call, raw bodies, ETag cache |
| SkillsMP / SkillHub | **real** via `rigel skills sync`; metadata only, no bodies (they have none to give) |
| Duplicates | **real**: SimHash ≤ 3 bits, clusters computed after every ingest, variants listed under the row |
| Evidence | **real**: advertised percentages from the description, `unmeasured` until a measurement exists |
| CLI | `rigel skills search|get|index|status|sync|sweep|stats|import|gc`, all with `--json`, §7.2 exit codes |
| Native agent | **real**: OmniRoute (local router, autostarted; default `gpt-5.6-sol-medium`), OpenRouter and Gemini (`gemini-4-argon` by default) streaming with retries, a deterministic scripted model, ten tools with path-checked reads and writes, skills pinned per session |
| Commands | **real**: `run_command` shows the exact text, cwd and timeout before anything runs; cwd is jailed to the project, output is capped at 64 KB per stream, the process is killed at its timeout, and the exit code is recorded as a `command` item. `--approval ask\|allow\|deny`; `--json` defaults to deny |
| Destructive commands | **real**: a deterministic classifier maps a command to policy rows — `delete-outside-project`, `recursive-remove`, `force-push`, `git-reset`, `registry-write` — before approval; the prompt names the rows and the matched token, and the `command` item records them |
| Snapshots | **real**: before an approved destructive command, `git stash create` object (kept alive by `refs/rigel/snapshots/<id>`) plus raw copies of changed and untracked files; a non-git tree or a repository with no commit is copied whole; 2 000 files / 200 MB cap, refusing above it. `rigel snapshot capture\|list\|restore` |
| Deliberation | **real**: a destructive command first gets one round of three read-only advisors (advocate, opponent, referee) on the parent's model, billed to its own turn and shown before the approval prompt; the answers are advisory and cannot allow, deny or alter the gate. Scripted runs record that advice was not gathered instead of consuming their fixed steps |
| Egress | **real**: `fetch_url` asks per origin with `once` / `session` / `always` scopes (`always` persists in `<index dir>/approved-origins.json`), re-approves a redirect that crosses origins, reads only text-like content types, caps the body at 256 KB, and records the page as a `web_search` item with `untrusted: true` |
| Subagents | **real**: `spawn_agents` starts up to 4 read-only children in the background on an eight-model cheap allowlist (GPT-5/5.4 nano, Gemini Flash-Lite/3 Flash, DeepSeek V4.1 Flash, Qwen3 Coder, Haiku 5.5, GPT-5.4 mini); each child is its own agent row and bill, nested under the parent turn, and must be given a role, an explicit file list and a task. The parent returns at once, can `chat_send`/`chat_wait` with the children in the session room while they run, and collects them with `subagent_results` before finishing. The split is recorded on the session work board, every child sees it and may read only its own files, and each starts its answer with the files it worked on; `assign_agents` gives the same file-scoped work to agents already in the chat room. The allowlist is OpenRouter-shaped, so the tool is hidden on the Gemini provider rather than handing it ids Google rejects |
| The bill | **real**: micro-dollar accounting from provider usage, local price fallback, 80% warning, cap enforced within one turn, `reconcile` |
| Session surface | **real**: roster, role-gutter transcript, streaming text, collapsed tool lines, patch diffs, raw/debug toggle, inline 80% prompt, a live model and effort picker fed by the provider catalogue (provider stays in Settings), and a Swarm button that hands the typed task to every agent in the chat room |
| Patches | **real**: CAS check, write-ahead journal, fsync + rename, idempotent apply, revert once applied |
| Resume | **real**: tree hash vs checkpoint, running turns closed as `cancelled`, journal reconciled without applying, forced continue is an intervention |
| Memory | **real**: committed `MEMORY.md` + topics per agent, absolute-path/secret validator, index rebuild |
| MCP | **real**: stdio server scoped to `--session` and `--agent`, sharing the same tools |
| Codex harness | **real**: `rigel codex probe` and one turn through `codex app-server`, with `codex exec --json` as the tested fallback; Rigel writes the router `-c` overrides itself (verified against Codex 0.162.1) |
| Claude harness | **real**: `rigel claude probe` and one turn, recorded as a session turn and billed with the CLI's own `total_cost_usd`; the turn passes `--mcp-config` and `--permission-prompt-tool mcp__rigel__rigel_approve`, and the CLI answers each question on stdin with the same four scopes as the native agent. Protocol verified against a scripted 2.1.251 stream; a live subscription turn has not been run. The window sends Claude a turn as well: pick `claude` in the composer and `turn_send` runs the harness, with the approval card answering `rigel_approve` through the app's own `mcp` mode |
| Gemini CLI harness | **real protocol**: `rigel agent acp --harness gemini` drives one ACP turn through `gemini --acp` (the flag replaced `--experimental-acp` in 0.33), records session updates and both sides of every permission request, and takes `GEMINI_API_KEY` — the same keychain entry as the native `gemini` provider. Probed as its own row with the install hint `npm install -g @google/gemini-cli`; verified against the scripted ACP agent, because the CLI is not installed on this machine |
| Cursor/Cline harness | **real protocol**: `rigel agent acp --harness cursor|cline` drives one ACP turn, records session updates and both sides of every permission request, and refuses a subscription-only identity; each harness takes its own key (`CURSOR_API_KEY`, `CLINE_API_KEY`) and is probed separately. An ACP run asks on the CLI's stdin, and the window card is the native agent's surface. Verified against a scripted ACP agent — neither CLI is installed on this machine |
| Claims | **real**: `acquire` / `heartbeat` / `release`, a tombstone on release, staleness reported and never auto-released, 24 h expiry; `rigel claim list\|release`, MCP `rigel_claim` / `rigel_release` / `rigel_who` |
| Attribution | **real**: a 250 ms watcher that hashes changed files, names the writer or marks `ambiguous`, and records a conflict when a claimed path moves under someone else |
| Diff | **real**: conflict strip with both names and hashes, claims with stale rows, unverified counts, per-patch revert |
| Tools and trust | **real**: twelve curated tools probed per host, cached with path and SHA-256; requirements inferred from `allowed-tools` and body commands with their line; the five-factor arithmetic printed by `rigel trust explain`; approval-gated installs with exit code and duration in the ledger; unattended queues; apt shown and refused |
| Doctor | **real**: index and project integrity, schema versions, blob rows vs files, stale claims, budget reconcile, keychain, config age, one row per harness, tool hash pins, unfinished installs, update switch; exit 0/1/2 |
| Updates | **real but off**: an Ed25519 manifest check over `version / url / sha256`, an artifact SHA-256 check, a switch that defaults off, and no automatic request. The Ed25519 publisher key is not the Authenticode certificate that signs the installers |
| Local services | **real**: the window starts every service a turn can need as it opens — OmniRoute when the provider uses it, and the agentchattr room when its checkout sits beside the restored project. Both run off the UI thread; a service that cannot start is a status in its tab, never a failed launch |
| Prototype migration | `rigel skills import --from` + [the export snippet](migrating-prototype-index.md) |
| Discover / Targets / Dependencies (prototype) | still the Phase 0 browser app; Phase 6 ports them |

Back to the [README](../README.md).

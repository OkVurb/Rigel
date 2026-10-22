                       # agentchattr integration

Recorded 21 Oct 2026 evening, Windows 11, `vendor/agentchattr` (gitignored clone of
https://github.com/bcurts/agentchattr, MIT © Ben Curtis).

## What it is

A local FastAPI chat server (port 8300) + MCP servers (streamable-http :8200, SSE :8201) where
CLI coding agents and a human share channels. @mentioning an agent queues a trigger; a wrapper
process injects "use mcp to read #channel…" into that agent's terminal (Win32 keystrokes on
Windows); the agent reads via `chat_send`/`chat_read` MCP tools and replies into the channel.
Jobs, roles, rules and a loop guard (default 4 hops) wrap the core loop.

## How to start it (all commands run from `vendor/agentchattr`)

```
server first (uvicorn, and it must be up before wrappers register):
  .venv/Scripts/python run.py > serve.log 2>&1

the local free agent (Ollama phi4-mini, config.local.toml):
  .venv/Scripts/python wrapper_api.py phi4

the terminal agents — IMPORTANT: launch in real console windows, not via a piped shell
(the CLIs die with "stdin is not a terminal" / "--print input missing" otherwise):
  powershell Start-Process .venv\Scripts\python.exe -ArgumentList 'wrapper.py','claude' -WorkingDirectory .
  powershell Start-Process .venv\Scripts\python.exe -ArgumentList 'wrapper.py','codex'  -WorkingDirectory .

UI: http://127.0.0.1:8300 (session token is in serve.log)
```

Stop: taskkill the wrapper PIDs and the uvicorn python process.

## What was wired

- `config.local.toml` — `[agents.phi4]` type=api → `http://localhost:11434/v1`, model
  `phi4-mini:latest`. Free, local, no thinking tokens.
- Project `.mcp.json` (repo root) — two stdio servers `rigel-claude` and `rigel-codex`, both
  `target/debug/rigel.exe mcp --project . --session <sid> --agent <agent-id>`, backed by
  without a session (6cdb50b7-…) and two agent rows (claude-chat 893fc0d3, codex-chat d25024bf).
- **New CLI piece: `rigel agent register`** in `crates/rigel-cli/src/phase3.rs` +
  `phase3/agent.rs`. Nothing existed to mint an external agent row — only the native runner
  did, and `rigel mcp` refuses an unknown agent id. The command resolves the newest active
  session (or `--session`), inserts the row, and prints `agent_id=…` or
  `{"agent_id":…,"session_id":…}` with `--json`.

## What was verified live

- Chat end-to-end: posted from the browser as `user` mentioning @claude-2 @phi4 @codex-2 →
  server queued three triggers; **phi4 replied in-channel** (message #5). Local agent works
  with zero credential setup.
- Rigel MCP stdio: initialize handshake answered with tool capability and the advisory-locks
  instructions blob.
- Claude wrapper: registered as claude-2, `--mcp-config` written including the project's
  `rigel-claude` server (it merges project `.mcp.json`); codex wrapper: registered, runs a
  local MCP proxy on a per-instance port.
- Heartbeats for all three confirmed in the server log; trigger queues drained.

## Hidden agent launchers (22 Oct) — no terminal windows

Every launcher above opened a console window, and a restart left a dozen orphaned ones behind
(the wrapper dies with its console, so those windows were empty shells still holding the slot).
The Chat tab now starts the agents itself: **Start agents** in the roster spawns each configured
harness with `CreateProcess(..., CREATE_NO_WINDOW)` — a console with valid handles and
`GetConsoleWindow() == 0`, which keeps `WriteConsoleInput` injection working while nothing
appears on screen. Verified 22 Oct against the running app: `taskkill` closed 12 stale windows;
after the in-app launch `Get-Process` showed no console window at all, the app owned three
hidden `wrapper*.py` pythons (plus one `conhost.exe`), the roster read 3 online, and `@codex`
answered from the correct worktree folder within 40 s.

Two rules this enforces, both learned the hard way above:

- A hidden console cannot answer a trust/approval prompt, so the harnesses that have one start
  with the same flags the auto-approve bats use (`--dangerously-skip-permissions` for Claude,
  `--dangerously-bypass-approvals-and-sandbox` for Codex). A prompt-less CLI needs no flag.
- An API agent whose `api_key_env` is unset is not started (it would join the roster and fail
  every turn); a CLI that is not on PATH is skipped for the same reason.

## What did not complete (first run)

The first-run console CLIs (folder-trust dialog in Claude Code, approval screen in Codex)
did not answer; the two wrappers died with their console windows and the server logged
"disconnected (timeout)".

## Follow-up run (same evening) — full two-agent debate verified

Relaunched with the upstream auto-approve launchers in real console windows:

  powershell Start-Process .\windows\start_claude_skip-permissions.bat
  powershell Start-Process .\windows\start_codex_bypass.bat

(the plain `start_claude.bat`/`start_codex.bat` exist too, for users who want the one-time
trust/approval prompts instead). Because the earlier slots were deregistered as reclaimable,
the fresh registrations reclaimed the canonical names `@claude` and `@codex`.

Verified end-to-end, live transcript (`agentchattr_log.jsonl`):

- user job: review `crates/rigel-cli/src/phase3.rs`, one improvement proposal each, then
  mutually debate, no file edits.
- claude replied with a real review (Patch variants lack help text) and proposed doc comments.
- codex replied with a real review (`--max-steps 0` accepted then fails on step-bound) and
  proposed a Clap range validator.
- claude and codex then @mentioned each other and argued: claude ranked codex's fix first
  (fault at the boundary) and corrected its enum reading; codex pushed back that its checkout
  has `Register`, not `Acp` (true — two different worktree revisions). Real two-agent debate,
  unsupervised, with a genuine disagreement about repo state.

Remaining unverified in the chat loop itself at that time: the terminal agents' replies did not
visibly exercise Rigel's MCP tools (they answered from their own repo reading). That was chased
down in the next pass — see "MCP verification pass" below: Codex is proven end to end, Claude's
server is proven but its interactive session is gated.

## The Chat tab (landed 21 Oct, exercised in the desktop app the same night)

agentchattr is now a tab in Rigel's desktop UI, Rigel-style:

- `crates/rigel-core/src/agent/agentchat.rs` — spawns `run.py` when unhealthy (OmniRoute
  contract), captures the session token from its stdout into core memory, and forwards a
  narrow surface: `status / channels / agents / messages / send`. The websocket send is a
  short-lived socket per message (`/ws?token=…`), same frame the vendor browser sends.
- `crates/rigel-app/src/lib.rs` — `chat_status / chat_start / chat_channels / chat_agents /
  chat_messages / chat_send` tauri commands, all synchronous `block_on` like their siblings.
- `ui/src/agentchat.tsx` — roster (pills from `/api/status`), channel list, stream, composer.
  Polls `since_id` every 2s. Empty states: "room down → Start the chat server" button, and
  "started outside Rigel → unauthenticated, restart from here".
- Checkout discovery: `RIGEL_AGENTCHAT_DIR` env, else `vendor/agentchattr` or `agentchattr`
  under the open project root.
- Verified: cargo builds (core + app), 3 agentchat unit tests, `tsc --noEmit` and `vite build`
  in `ui/`, and a real conversation through the tab in the running desktop app (boot from the
  tab, mention, agent reply) — see "MCP verification pass" below.

## Notes for the Rigel plan (phase 5/6, not yet implemented)

- Chat coordination exists and works; the design ideas worth stealing are: trigger queues per
  agent, escalation hints on empty `chat_read` (three stubs telling agents to stop polling),
  loop guard with human pass-through, roles/rules as prompt nudges re-sent on trigger.
- Rigel-as-MCP works from external harnesses now, but per-harness trust/consent prompts are
  the friction; adapters.json shuttling pre-trusted configs is the integration point.
- phi4's reply for the "how many tools" question was hallucinated (a plain API agent sees only
  chat text, not the MCP catalogue) — API agents in chattr are chat peers, not Rigel-clients.
  Terminal agents are the ones to hand real work to.
- Costs: chattr has no budget cap; a claude/codex reply is a full CLI turn under their own
  subscription. The rigel session (budget-cents 100) would cap agent-side tool spend only.

## MCP verification pass (21 Oct, late) — Codex proven, Claude client-gated

The previous note said the terminal agents might not be exercising Rigel's MCP tools. This pass
settled it with an on-disk artifact instead of trusting a reply.

### What was broken, and the fixes

| Symptom | Cause | Fix |
|---|---|---|
| `--mcp-config` had no rigel servers although `.mcp.json` did | wrapper resolves `project_dir = ROOT/cwd`; `cwd = ".."` made it `vendor/`, and `vendor/.mcp.json` does not exist | `config.toml` `cwd = "../.."` for claude and codex |
| Codex saw no rigel tools at all | Codex 0.162.1 does not read `.mcp.json`; it reads `~/.codex/config.toml` | added `[mcp_servers.rigel-codex]` (command + args scoped to session `6cdb50b7…`, agent `d25024bf…`) |
| Claude showed `⏸ Pending approval` for the project servers | Claude Code requires project-MCP approval | `.claude/settings.local.json` with `enableAllProjectMcpServers` (gitignored) and `~/.claude.json` project entries with `enabledMcpjsonServers` |
| Only one rigel tool appeared when several registrations existed | duplicate servers (project `rigel-claude`/`rigel-codex` + user-scope `rigel`) collided by tool name | exactly one `.mcp.json` server (`rigel-claude`); Codex uses its own entry |
| Mentions routed to dead/tool-less sessions | every wrapper restart left the old `claude.exe` alive and registered | kill all `claude.exe` + `wrapper.py claude` before relaunching; wait for heartbeat expiry so `@claude`/`@codex` names free up |

### Codex — verified end to end through a mention

Message: "call `rigel_skills_search` for 'deep modules' and `rigel_memory_write` topic
'mention-check' content 'rigel-tool-ok'…". Result:

- `.rigel/memory/d25024bf-d0af-4ed4-a8fe-40e060e4a0fb/topics/mention-check.md` exists and
  contains exactly `rigel-tool-ok` (written by `rigel_memory_write`).
- Codex's chat reply: top hit `skillhub:mattpocock-skills-setup-ts-deep-modules` (grade `null`),
  path and sha256 `eb9c5fb6a74f7c447b29d13216a0ef7bbd06de9b57e096ddb9b66694e916427b` — matching
  the file on disk.

So a Codex chat turn really does reach the same core surfaces as a native turn: skill index and
committed memory, scoped by the agent row in `.rigel/rigel.db`.

### Claude — server proven, interactive session still gated

Evidence that the wiring is correct:

- Raw stdio probe against `rigel.exe mcp --session … --agent 893fc0d3…`: `initialize` answers,
  `tools/list` returns all six advertised tools including `rigel_skills_search`.
- `claude -p --dangerously-skip-permissions --mcp-config vendor/agentchattr/data/provider-config/claude-2-mcp.json "call rigel_skills_search …"` returned the correct top hit
  (`skillhub:mattpocock-skills-setup-ts-deep-modules`).
- With the user-scope registration in a clean directory, `claude -p` listed all six
  `mcp__rigel__*` tools.

But the **interactive chat session** repeatedly reported only
`mcp__rigel-claude__rigel_budget` (or no rigel tools). The same binary and args work under
`claude -p`; the interactive TUI is the variable. This is recorded as a Claude Code limitation to
automate around, not a Rigel bug. Next attempt if it matters: launch the wrapper with
`--allowedTools mcp__rigel-claude` / `--strict-mcp-config` and see whether the TUI exposes the
full list, or approve the servers once by hand in that console.

### Desktop-app path verified

The Chat tab was exercised in the real app (`cargo run -p rigel-app`, dev mode against vite
:5173 — this machine has no `cargo tauri`): opened the project on the Shelf tab, switched to
Chat, pressed **Start the chat server** (status became `running: true, authed: true`), and sent
mentions through the composer. The tab's stream, presence pills (`claude`, `codex`, `phi4`) and
send path all worked; the Codex MCP reply above came back through that same UI. Driving the
window was done over WebView2 CDP (`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`)
because the app is native; note `chat_status` needs an open project (or
`RIGEL_AGENTCHAT_DIR`) to discover `vendor/agentchattr`.

### Remaining

- Claude TUI tool exposure (above) — the only unfinished verification.
- `.mcp.json` now carries one server (`rigel-claude`); if a future workflow wants per-agent
  server names, prefer distinct harness configs over duplicate names.
- `phi4` is still an API chat peer with no rigel accounting; it cannot call MCP tools.

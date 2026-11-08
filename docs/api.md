# Rigel's API for an external orchestrator

`rigel serve` puts Rigel's coordinator and one project workspace behind a loopback HTTP API.
It is the third client of the same service layer the window and the CLI use: a turn started over
HTTP is a normal session turn, billed to that session, recorded in the same transcript, and
stopped by the same budget cap.

The API is **local by default**: it binds `127.0.0.1`. For a tailnet peer, name the machine's
Tailscale address; `0.0.0.0` is refused because it would also listen on Wi-Fi and Ethernet. A
hosted orchestrator cannot reach `127.0.0.1`; see [Reaching it over Tailscale](#reaching-it-over-tailscale)
and [Reaching it from a hosted orchestrator](#reaching-it-from-a-hosted-orchestrator).

## Start it

```bash
rigel serve --project C:\path\to\your\workspace
```

```text
rigel api on http://127.0.0.1:20140
  project: C:\path\to\your\workspace
  config:  C:\Users\you\AppData\Roaming\Rigel\api.json
  token:   <64 hex characters>
  openapi: http://127.0.0.1:20140/v1/openapi.json
  loopback only; every request needs `Authorization: Bearer <token>`
```

The token is minted once and reused across restarts. `--rotate` replaces it, `--print-token`
prints it without starting the server, and `--port` / `--host` change the listener. The same
facts are written to `api.json` (`RIGEL_API_CONFIG` overrides the path) so a client can read the
token from disk instead of being handed it.

| Flag | Meaning |
|---|---|
| `--project <PATH>` | The one project the API may touch. Every file path is relative to it. |
| `--port <N>` | Default `20140`. `0` asks the OS for a free port. |
| `--host <ADDR>` | Loopback (the default), or one interface address such as a Tailscale IP. `0.0.0.0` and hostnames are refused. |
| `--tailscale` | Bind the machine's Tailscale IPv4 address, found with `tailscale ip -4`. Conflicts with `--host`. |
| `--public-url <URL>` | The base URL a tailnet peer or tunnel reaches this port on; changes what is advertised, not what is bound. |
| `--rotate` | Replace the stored token before serving. |
| `--print-token` | Print the token and exit. |
| `--json` | One machine-readable line on start instead of the human banner. |

## The four connector questions

**Base URL.** `http://127.0.0.1:20140` on the machine running Rigel. Over Tailscale, use
`http://<tailscale-ip>:20140` and pass the same URL to `--public-url` so the printed banner and
the OpenAPI `servers` entry agree. A tunnel works the same way with its public HTTPS URL.

**Spec.** `GET /v1/openapi.json` — OpenAPI 3.1, bearer security scheme, every operation with an
`operationId`. Generate the connector from that URL; nothing else needs to be read.

**Auth.** A bearer token, in the header:

```
Authorization: Bearer <token>
```

There is no OAuth, no query-string key, and no per-endpoint variation. A request carrying an
`Origin` header is refused with `403` so a web page cannot drive the API even if it learns the
token.

**First endpoints.** `GET /v1/health`, `GET /v1/providers`, `GET /v1/models`, then
`POST /v1/sessions` and `POST /v1/sessions/{id}/turns`. For file work, `GET /v1/workspace` and
the four `/v1/workspace/*` routes below.

## Endpoints

### Identity and models

| Method | Path | What it returns |
|---|---|---|
| GET | `/v1/health` | `{ok, name, version, project, pid, uptime_secs, sessions}` |
| GET | `/v1/providers` | One row per provider: `id`, `name`, `default_model`, `credential` (`keychain`/`local`/`none`), `ready` |
| GET | `/v1/models` | The merged roster. Query: `provider`, `current`, `all`, `refresh`. Cached 60 s |

### Sessions and turns

| Method | Path | What it does |
|---|---|---|
| GET | `/v1/sessions` | Sessions, newest first |
| POST | `/v1/sessions` | Start one: `{title?, budget_cents?}` |
| GET | `/v1/sessions/{id}` | Session, its agents, `spend_micros` and `budget_cents` |
| GET | `/v1/sessions/{id}/turns` | Transcript, oldest first. Query: `limit` (default 50), `items=0` |
| POST | `/v1/sessions/{id}/turns` | Run one coordinator turn |
| GET | `/v1/turns/{id}` | Turn row: `status`, `usage`, `stop_reason` |
| GET | `/v1/turns/{id}/items` | That turn's items: messages, tool calls, patches, usage |
| POST | `/v1/turns/{id}/cancel` | Signal cancellation |

`POST /v1/sessions/{id}/turns` body:

```json
{
  "prompt": "Add retry with backoff to the deploy poller",
  "agent": "coordinator",
  "provider": "omniroute",
  "model": "gpt-5.1-codex",
  "effort": "medium",
  "max_steps": 40,
  "approve": false,
  "dry_run": false
}
```

* `provider` defaults to `RIGEL_PROVIDER`, then `omniroute`; the id must exist in
  `/v1/providers`.
* `model` defaults to that provider's own default. A provider that publishes none requires one.
* `dry_run: true` prices the first step and creates nothing.
* A turn request stays open until the turn finishes. Fan out by sending concurrent requests:
  each connection runs in its own task, and each turn is recorded and billed independently.
* `approve` defaults to **false**. With it off, every `run_command`, `fetch_url` and writable
  plan card is answered with deny, so a turn can read, write and edit files but cannot run a
  shell command. `approve: true` allows once **for that one request** it appears on; no scope is
  stored.

A writable child agent is refused over this API (`worktree_dir` is unset), so a coordinator turn
can spawn read-only subagents only. That keeps a remote caller from acquiring a private checkout
it could never merge.

### Workspace

These are the hands-on-the-project routes. Every path is project-relative; the patch pipeline
resolves it, so `..`, absolute paths and symlinks out of the project are refused.

| Method | Path | What it does |
|---|---|---|
| GET | `/v1/workspace` | Project root, the session and agent a file call inherits, and the byte limits |
| GET | `/v1/workspace/files?path=.` | One directory: `name`, `path`, `kind`, `size`. Skips `.git`, `.rigel`, `node_modules`, `target`, `dist`, `.next` |
| GET | `/v1/workspace/read?path=src/lib.rs` | UTF-8 content, `sha256`, line count. 512 KiB cap |
| POST | `/v1/workspace/write` | Create a file, or overwrite one by naming its current `base_hash` |
| POST | `/v1/workspace/edit` | Replace one exact occurrence, guarded by `base_hash` |
| POST | `/v1/workspace/command` | Run one command through the approval gate |

Every write is a **journalled patch**, not a bare file write: it gets a `patch_id`, a diff, a
`result_hash`, and an author, so the window's Diff view names who changed the file. A stale
`base_hash` is a `409` and writes nothing.

```json
// POST /v1/workspace/write  (create)
{ "path": "notes/plan.md", "content": "# Plan\n" }

// POST /v1/workspace/write  (overwrite; base_hash came from a read)
{ "path": "notes/plan.md", "content": "# Plan v2\n", "base_hash": "<the 64 hex characters read returned>" }

// POST /v1/workspace/edit
{
  "path": "src/deploy.ts",
  "old_string": "for (let i = 0; i < 3; i++) {",
  "new_string": "for (let i = 0; i < 5; i++) {",
  "base_hash": "<the 64 hex characters read returned>"
}
```

`POST /v1/workspace/command` takes `{command, cwd?, timeout_secs?, approve?}`. The command runs
in the project root unless `cwd` names a subdirectory. `approve` defaults to false; a denied
command returns `403` with the gate's reason and nothing runs.

## Curl, start to finish

```bash
TOKEN=$(rigel serve --project . --print-token)
BASE=http://127.0.0.1:20140
AUTH="Authorization: Bearer $TOKEN"

curl -s -H "$AUTH" $BASE/v1/health
curl -s -H "$AUTH" $BASE/v1/providers
curl -s -H "$AUTH" "$BASE/v1/models?provider=openrouter"

SESSION=$(curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{"title":"muse","budget_cents":500}' $BASE/v1/sessions | jq -r .session.id)

curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{"prompt":"Summarise what this project does","dry_run":true}' \
  $BASE/v1/sessions/$SESSION/turns

curl -s -H "$AUTH" "$BASE/v1/workspace/read?path=README.md"
```

## Reaching it over Tailscale

A tailnet peer can dial the machine's Tailscale address directly; no tunnel is involved.

```bash
rigel serve --project . --tailscale
```

`--tailscale` runs `tailscale ip -4` and binds the first `100.64.0.0/10` address it prints, then
advertises `http://<that address>:<port>`; there is no raw address to update when the node is
re-addressed. If the CLI is not on `PATH`, Rigel also tries the standard install locations, and
`RIGEL_TAILSCALE_BIN` points it at another binary. The equivalent manual form is:

```bash
rigel serve --project . --host 100.70.84.102 \
  --public-url http://100.70.84.102:20140
```

The listener is on that address only, so `127.0.0.1` stops answering; use the Tailscale address
locally too. `0.0.0.0` is refused on purpose, and so is a non-`localhost` hostname: MagicDNS resolving `rigel`
is the client's job, not the API's. If the peer cannot resolve the machine name, give it the
`100.x` address. There is no TLS here — Tailscale already encrypts and authenticates transport
with WireGuard, and the bearer token remains the authorization check. The `Origin` refusal is
still in force, so a browser on the tailnet cannot drive the API either.

## Reaching it from a hosted orchestrator

A hosted Muse cannot dial `127.0.0.1` on your computer. Put a tunnel in front of the port and
start Rigel with the public URL, so the contract it serves matches the URL a client uses:

```bash
cloudflared tunnel --url http://127.0.0.1:20140
rigel serve --project . --public-url https://your-tunnel.trycloudflare.com
```

The tunnel, not the API, is the thing exposed to the internet. Keep the token out of the tunnel
URL, and rotate it (`--rotate`) if it is ever pasted somewhere it should not be. A tunnel also
removes the loopback guarantee: the bearer token becomes the only thing between the internet and
your project, which is why it is 244 bits and never sent in a query string. Prefer Tailscale to a
public tunnel when the orchestrator can join the tailnet: it keeps the API off the public
internet while still reaching it from another machine.

## Choosing the workspace

The window is the workspace picker. It writes `%APPDATA%\Rigel\current-workspace.json` (or the
`RIGEL_INDEX_DIR` equivalent) whenever it opens a project, and `rigel serve` reads that marker on
each authenticated request. When it names a different directory, the server swaps both the
project root and its session database in one step, so the next request works in the project the
window has open. A missing marker leaves the `--project` choice alone, and a bad one is ignored
rather than taking the API down.

The API can also choose explicitly:

```bash
curl -s -H "$AUTH" $BASE/v1/workspaces
curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{"path":"C:/Users/me/Rigel/notes"}' $BASE/v1/workspaces/open
```

Sessions belong to one workspace database. After a switch, an id from the previous workspace is a
404; start a session in the new one. `/v1/workspaces` lists the current workspace plus the folders
under `~/Rigel`, which is where **New workspace** creates them. Without `--full-access`, this
endpoint refuses a folder outside `~/Rigel` unless it is already active, so it cannot turn `C:\`
into a project and bypass the workspace jail; the window can still open any folder it likes, and a
later request follows that marker.

## Full computer access

By default every file route is confined to the active project. Start the server with
`--full-access` to add `/v1/fs/*`, which accepts an absolute path or one relative to the active
workspace:

```bash
rigel serve --project . --full-access

curl -s -H "$AUTH" "$BASE/v1/fs/read?path=C:/Users/me/notes.txt"
curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{"path":"C:/Users/me/notes.txt","content":"hello","create_dirs":true}' \
  $BASE/v1/fs/write
```

`/v1/fs/list` lists any directory, `/v1/fs/read` reads a UTF-8 file, and `/v1/fs/write` and
`/v1/fs/edit` use the same sha256 compare-and-swap contract as the workspace routes. A write
inside the project is still a journaled patch with an attributed author; a write outside it is an
atomic replace and says `"journaled": false`, because the journal belongs to one project. Commands
may also start in any directory in this mode. Nothing else changes: the bearer token, the Origin
refusal and the approval gate still apply, and `approve` is still denied by default.

## Concord

Concord is the shared work-state layer the harnesses use. The API exposes its MCP tools directly,
so Muse can join the same room without reading `.mcp.json` or launching Node itself:

```bash
curl -s -H "$AUTH" $BASE/v1/concord/tools
curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{"tool":"inspect_work","arguments":{}}' $BASE/v1/concord/call
curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{"tool":"start_work","arguments":{"task_id":"muse-1","title":"API work","kind":"task"}}' \
  $BASE/v1/concord/call
```

Muse speaks as `muse`. `agent_id` is filled in for the five known tools, and `start_work` gets the
active workspace as `cwd`. The tool list is live: `start_work`, `inspect_work`, `update_work`,
`transfer_work` and `finish_work`, with their real schemas. Concord runs through `npx`, so Node is
required; a call has a two-minute deadline and a timeout is a 502. The workspace's `.concord`
database is created by Concord itself on first use.

Muse is a **pull client**, not a live harness: `inspect_work` reports it as `promptable false`.
Another agent cannot wake an idle HTTP client, and Concord offers no webhook. Rigel therefore runs
a background poller for the inbox: it holds one Concord stdio session open, calls `inspect_work`
as `muse` every four seconds, and keeps new messages in a small cache with a monotonic `seq`.

```bash
curl -s -H "$AUTH" "$BASE/v1/concord/inbox?since=0"
curl -s -H "$AUTH" "$BASE/v1/concord/inbox?since=42&limit=20"
```

The response is `{agent, messages: [{seq, message}], retained, oldest, revision, checked_at,
updated_at, error}`. Repeated messages are de-duplicated by `id` (or by their whole body when
Concord sends no id), and the newest 200 are retained. The cache is written to
`concord-inbox.json` in the data directory, because `inspect_work` marks a message read when the
poller collects it; a restart therefore does not drop mail Muse has not read yet. `since` is the
last `seq` Muse handled; the limit applies after that filter, so replaying from zero still
returns the newest rows. If
Concord or Node is unavailable the poller retries every ten seconds and reports the failure in
`error` without discarding what it already cached. `/v1/concord/call` with `inspect_work` is
still there for a full workspace view, a specific thread, or an outbox that the poller does not
cache.

`/v1/app` and `/v1/events` carry `concord.pending` and `concord.revision`. The events fingerprint
changes when a new message lands, so Muse's existing long poll wakes it and it reads
`/v1/concord/inbox` from its last `seq`. No manual `inspect_work` loop.

Replying is one route, not a hand-written `update_work` call. The wrapper takes the message id
and body, fills in `operation: "reply"`, `reply_to_message_id`, the cached sender as
`to_agent_id`, and a stable `idempotency_key`, then calls Concord as `muse`:

```bash
curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{"message_id":"8601651d-4306-4597-b23f-6234254ee704","content":"On it — I’ll split this across the room."}' \
  $BASE/v1/concord/reply
```

`to_agent_id` and `idempotency_key` are optional overrides. The default key is
`muse-reply-<message_id>`, so retrying the same reply cannot double-send. The response echoes the
message id and recipient with Concord’s result and text.

The window reads the same cache at **Settings → Concord**. Each message shows its content and the
`sent_at` / `delivered_at` / `read_at` / `replied_at` receipt, and a reply box posts to
`/v1/concord/reply`; the window never runs Concord or builds a tool call itself.

## Waking Muse

Muse is the boss of **Swarm** mode, and the window presents that boss as **Rigel**. A Swarm prompt
is not broadcast by the window unless the user explicitly chooses “No single boss”; it is written
to a small inbox and Muse claims it. Ordinary Chat and Session prompts still go straight to one
agent. Two files make this work, both under the data directory (`%APPDATA%\Rigel` or
`RIGEL_INDEX_DIR`):

* `app-state.json` — the window's heartbeat. It says `{"open": true, "project": "...",
  "updated_at": <unix ms>}` and is rewritten every ten seconds while the window is alive. Older
  than 45 seconds means the window crashed or quit, so the API reports `open: false` instead of
  leaving a stale `true` behind.
* `swarm/inbox/<id>.json` — one queued prompt, with `id`, `project`, `prompt`, `targets` (the room
  agents the window's preflight found), `lead` and `created_at`. `lead` is `{"kind":"rigel"}` for
  Rigel, the Muse coordinator, or `{"kind":"model","provider":"…","model":"…"}` when the user
  asked Rigel to think with a particular model. A `broadcast` swarm has no boss and is sent by the
  window directly, so it never appears in this inbox.

The consumer must read `lead` after a claim, before it starts the boss turn. `{"kind":"rigel"}`
means its own default coordinator model; `{"kind":"model",...}` means that exact provider and
model, not a suggestion to substitute. If the consumer cannot run that pair, it must stop and say
so rather than silently using another model. The job's `prompt` and `targets` are unchanged.

Muse does not need a public callback. It is a pull client: hold `GET /v1/events` open and the
request returns as soon as the window opens or closes, the project changes, a Swarm prompt
arrives, or Concord delivers an inbox message.

```bash
curl -s -H "$AUTH" $BASE/v1/app
curl -s -H "$AUTH" "$BASE/v1/events?wait=25&since=..."
curl -s -H "$AUTH" $BASE/v1/swarm/jobs
curl -s -X POST -H "$AUTH" $BASE/v1/swarm/jobs/next
curl -s -X POST -H "$AUTH" $BASE/v1/swarm/jobs/<id>/complete
curl -s -X POST -H "$AUTH" $BASE/v1/swarm/jobs/<id>/release
```

`wait` defaults to 25 seconds and is capped at 60. `since` is the previous response's
`event.fingerprint`: omit it for an immediate snapshot, then pass it back so the next call blocks.
A timeout is still a 200, with `changed: false` and the current snapshot, so a reconnect does not
need a separate case. `/v1/app` has the same body without waiting: `open`, `project`,
`heartbeat_age_ms`, `swarm.pending`, `swarm.oldest` and the fingerprint.

`POST /v1/swarm/jobs/next` claims the oldest prompt by renaming it into `swarm/claimed/`. A second
claim cannot take the same file, and a Muse crash leaves the job claimed rather than silently
re-running it. After the work is reported, `POST /v1/swarm/jobs/{id}/complete` moves the file to
`swarm/completed/` as an audit record. If the job cannot be run, `POST
/v1/swarm/jobs/{id}/release` moves it back to `swarm/inbox/`. Both transitions are idempotent in
their desired state; trying to complete a queued job or release a completed job returns `409
invalid_job_state`. An unknown id returns `404`.

For the consumer-side operating contract (exact `lead` rules, preflight before claiming, room
assignments, Concord replies, and crash recovery), see
[muse-big-boss.md](muse-big-boss.md).

## Muse in the room

The room is the same agent chat the window's Swarm preflight starts. Muse joins as `muse`, reads
the roster, and posts one assignment per agent:

```bash
curl -s -X POST -H "$AUTH" $BASE/v1/room/start
curl -s -H "$AUTH" $BASE/v1/room/agents
curl -s -H "$AUTH" "$BASE/v1/room/messages?channel=general&since_id=0&limit=50"
curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{"channel":"general","assignments":[{"agent":"forge","task":"Add the retry test","files":["tests/retry.rs"],"spawn_subagents":true}]}' \
  $BASE/v1/room/assign
```

Each assignment is sent as `muse` and tells that agent it is a squad lead: split the piece, spawn
its own subagents for independent parts, give each helper a disjoint file set, stay inside those
files, and report changed files, tests and blockers. Up to twelve assignments per call.

Rigel does not add or start Meta's Muse Code CLI; **Muse is the remote muse.ai boss** and reaches
the room through this API. The room contains only the worker harnesses configured by the
operator.

The room is now self-healing. `POST /v1/room/start` and every other room route first call the
health-first adoption path: if the room server is already up, the API reads its landing-page
session token and adopts it; only an absent room is started. A restart of `rigel serve` alone
therefore no longer produces `502 room_unavailable` from `GET /v1/room/agents`.

The API also adopts the installed sidecar beside `rigel.exe` before it resolves any room path. The
room and its wrapper processes must read the same app-data copy because an assignment is delivered
through `*_queue.jsonl`: a wrapper launched from a different checkout watches a different queue
even though `/api/status` still reports the agent online. This is why `rigel serve` and the desktop
app both use the installed sidecar, and why a room that shows `available: true` but never replies
should be checked for a queue-directory mismatch before it is assigned more work.

A remote boss can create and staff its own room without opening the Chat tab:

```bash
curl -s -H "$AUTH" $BASE/v1/room/harnesses
curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{}' $BASE/v1/room/create
curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{"names":["forge","atlas"]}' $BASE/v1/room/agents/start
```

`GET /v1/room/harnesses` reports the configured harnesses with `name`, `label`, `api` and
`ready`, plus the probe-backed `status` (`usable`, `missing`, `missing_key`,
`unauthenticated`, `probe_timeout`, `provider_blocked` or `probe_failed`), its captured `evidence`, the exact `remediation`
command and the accepted `credential`. `ready` is true only for `usable`: installation alone no
longer counts, because Kimi, Kilo, CodeBuddy and Copilot can all be installed while refusing a
prompt until they are signed in. The probe runs a bounded headless prompt with the harness's
own `strip_env` rules and is cached for a minute, so the roster can poll it. `POST
/v1/room/create` ensures the room and starts every configured ready harness;
`POST /v1/room/agents/start` does the same, optionally limited to the `names` array. Both return
`created`, `started`, `problems`, `project` and the current `agents` roster. If a name is not
configured, it appears in `problems` rather than silently doing nothing. Messages are read from
the room's own store, so `since_id` is how a client resumes without replaying the channel.

`POST /v1/room/send` is the ordinary-message path. It posts `{channel?, text}` and then, only
when every configured ready harness is online under a clean base name, writes the same text to
the Swarm inbox as a job for Muse. The response says which happened:
`sent`, `kicked`, `job_id`, the `targets` it queued, and any `missing` or `duplicates` that
blocked the kickoff. A message is always posted; a job is not. A duplicate such as `claude` plus
`claude-2` blocks it because assigning to either slot would leave the other unaccounted for.

```bash
curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{"channel":"general","text":"@claude @codex - plan the retry work"}' \
  $BASE/v1/room/send
```

### Stuck-agent watchdog

A harness can report `busy: true` forever after its provider stream dies, and the room will keep
routing work to it. `rigel serve` and the desktop window each run a watchdog over the agents they
started. An agent counts as stuck when it stays `available` and `busy` for 15 minutes without a
new room message; a message resets the clock, so a long turn that is genuinely talking is left
alone. A stuck agent whose pid this process knows is killed and relaunched after the room frees
its name (about 50 seconds: the 15-second crash timeout plus the 30-second name reservation), then
takes its next assignment normally.

```bash
curl -s -H "$AUTH" $BASE/v1/room/watchdog
curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{"stuck_after_secs":60}' $BASE/v1/room/watchdog
```

`GET` reports `enabled`, `stuck_after_secs`, `last_check_ms`, the current `stuck` and `owned`
names, and the last 20 `restarts`. `POST` runs one inspection immediately; `stuck_after_secs`
overrides the threshold for that check. Only agents the answering process launched can be
restarted — the room status carries no pid, so an agent started by the other Rigel process is
listed under `skipped` with that reason instead of being killed by a guess. Set
`RIGEL_ROOM_STUCK_SECS` to change the default (minimum 30, default 900) and
`RIGEL_ROOM_WATCHDOG=0` to turn it off.

## Probing the roster

`POST /v1/models/probe` sends a real, tiny completion to a batch of the merged roster so a remote
agent can distinguish models that work from models that fail before it chooses a coordinator or
room worker. It is deliberately batched: `limit` is capped at 100 and defaults to 25, and the
response's `next_offset` is the cursor for the next call (`null` at the end).

```bash
curl -s -X POST -H "$AUTH" -H 'content-type: application/json' \
  -d '{"offset":0,"limit":25,"concurrency":4,"timeout_secs":30}' \
  $BASE/v1/models/probe
curl -s -H "$AUTH" $BASE/v1/models/probe
```

The default request is `Reply with the single word OK.` with `max_tokens: 8` and `effort: low`.
`provider`, `models`, `prompt`, `effort`, `max_tokens`, `include_non_chat` and `timeout_secs` are
optional. Each result carries `provider`, `model`, `state` (`ok`, `error` or `skipped`), `reply`,
`error`, `latency_ms`, `tools` and `modality`. `ok` means the reply actually contained the word
`OK`, not merely that the call returned bytes: a provider's upgrade notice or error text is an
`error`. The relay row (`rigel`) is always skipped because
it is not an inference model; non-chat rows are skipped unless `include_non_chat` is true. A
provider that holds no usable credential yields one error per row with the provider's message.

Every batch is merged by `(provider, model)` into `model-probe.json` in the API data directory
(`%APPDATA%\Rigel` by default), and `GET /v1/models/probe` returns that persisted report. Re-running
a model overwrites its old row, so the report is a current readiness snapshot, not a history. The
report never contains a credential. A full pass on 2026-11-07 covered all 941 rows and finished
with 231 `ok`, 709 `error` and 1 `skipped`; the per-provider table is in
[muse-big-boss.md](muse-big-boss.md#model-readiness-probe).

## Keeping the API up at sign-in

`scripts/install-rigel-autostart.ps1` copies the built binary to
`%LOCALAPPDATA%\Rigel\bin\rigel.exe`, writes a hidden VBS + CMD wrapper, and places
`Rigel API.vbs` in the Startup folder. The CMD loop restarts `rigel serve` if it exits, and retries
`--tailscale` every ten seconds, so a login where Tailscale comes up late still converges. Output
is appended to `%LOCALAPPDATA%\Rigel\logs\api.log`. It is a per-user Startup entry, not a
scheduled task, so it never needs elevation and never prompts at login.

```powershell
powershell -ExecutionPolicy Bypass -File scripts/install-rigel-autostart.ps1
```

Stop the running API before rerunning the script: the copy fails if `rigel.exe` is locked. The
service uses the same `api.json` token as a manual `rigel serve`, so an existing client keeps its
token across a reboot; `--rotate` still invalidates it deliberately.

## Limits worth knowing

* Turns are not streamed over HTTP: the response arrives when the turn ends. Run turns
  concurrently for fan-out, and poll `GET /v1/sessions/{id}/turns` while they are in flight.
* A command is a real shell command. `approve: true` means the caller, not you, answered the
  gate. With `--full-access`, a command can also start outside the project.
* Without `--full-access` the API never reads or writes outside the active project. With it,
  `/v1/fs/*` does, and only writes inside the project keep a patch-journal row. The API never
  reads keychain values, only whether a credential exists.
* The API is one process. Stop it and the port closes; a restart keeps the token but gets a new
  pid and, if `--port 0` was used, a new port.

# Multi-agent channels — the phase 5/6 draft

Written 21 Oct 2026, after a full day running [agentchattr](https://github.com/bcurts/agentchattr)
alongside Rigel (`vendor/agentchattr`, MIT). Everything below that says "verified" was observed
live, not designed. The vendor run log is `vendor/agentchattr/data/agentchattr_log.jsonl`; the
integration record is [qa/agentchattr-integration.md](qa/agentchattr-integration.md).

## What agentchattr proves works (verified live today)

- **A chat room where CLI agents are peers.** Claude Code and Codex, launched in console
  windows by a thin wrapper, answered a review job in `#general`, proposed one patch idea each,
  then @mentioned each other and argued — a real, unsupervised debate including one genuine
  disagreement about repo state (each booth on its own checkout revision).
- **The trigger loop is small.** `@agent` → per-agent queue file → wrapper injects
  "use mcp to read #channel…" into the console → the agent does a full CLI turn with the chat
  MCP servers attached. No sockets, no daemons beyond the server itself.
- **Escalation and loop control need tool-side nudges.** `chat_read` returns three explicit
  "stop polling" stubs; a hop router caps chains (default 4) and only ever resumes on a human
  `/continue`. The loop guard prevented zero runaway chains all day.
- **Identity is a registry, not an account.** Instances get `base`, `slot`, a per-epoch
  registration token, heartbeat + crash-timeout (60s), and reclaimable regeneration — slot 1
  renames rather than lets a second instance send as the base name.
- **The friction is the trust dialogs.** First-run Claude Code (folder-trust) and Codex
  (approval screen) stall injected keystrokes; the vendor ships auto-approve launchers and
  those work.

## The Rigel-native design

Principles already in the repo: the core owns state and secrets, the window never parses human
readables, every IPC answer is the Rust struct verbatim. Applied to channels:

1. **Identity = `agents` rows.** `rigel agent register` now exists (added 21 Oct, see QA doc).
   A channel participant is an agent row in the rigel session DB — the same rows the budget
   meter and patch journal already account. Harness comes from `adapters.json`; a row without
   a ledger cap is refused a turn, which is the one thing rat chat has nothing like.
2. **The Chat tab fronts the room, the core fronts the token.** Landed 21 Oct: `agentchat.rs`
   spawns/health-probes the vendor chat server (the OmniRoute contract), captures the session
   token from its stdout, and exposes `chat_status / start / channels / agents / messages /
   send` as tauri commands; `ui/src/agentchat.tsx` renders roster + stream + composer in the
   shell design language. The renderer never sees the token.
3. **A turn in a channel is a rigel turn.** The trigger that arrives on @mention becomes a
   `turn_send` with the channel message as prompt; spend lands on the agent row and stops at
   the session cap. Patches proposed in the debate stay *proposed* in the journal until a
   human applies — the two-agent debate today argued proposals, and that is exactly the
   journal's `proposed` gate.
4. **Loop guard, proven shape.** Keep: default routing = explicit mention only, a max-hops
   cap, and human-only resume. The vendor numbers (4 hops, 60s crash timeout) are sane
   defaults, not folklore — they were fine for a full day.
5. **Roles as nudges, not enforcement.** Roles/rules are prompt fragments re-sent on every
   trigger (verified: they change behavior through suggestion only). Rigel should keep the
   same honesty in its memory surface instead of pretending roles are permissions.
6. **API agents are text peers, harness agents are tool peers.** phi4 hallucinated its MCP
   tool list — an API chat peer sees only text. Terminal agents (harness rows) are the ones
   that can call the index and patch pipeline. Keep the distinction visible in the roster.

## What to build next (phase 6, in order)

1. Make the Chat tab read from the rigel DB instead of the vendor store only: channel events
   land as `Turn`/`Item` rows keyed by the mentioned agent's row.
2. Trigger queue inside the core: the vendor queue-file injection becomes `trigger(agent_row,
   prompt)` → `turn_send`; CLI injections keep the vendor wrapper until adapters gain
   headless one-shot support (codex/claude both have print modes already used by rigel-core).
3. Attribution: every channel message that authorized a patch cites `patch_id`, so the debate
   log and the diff tab are linked.
4. Kill the trust-dialog class of problems: adapters pre-trust per-project before registering
   harness rows (the `.mcp.json` + auto-approve launcher flow, automated).

## Open questions

- Session cap granularity: per-turn vs per-agent-per-day for chat turns (chattempts are small
  but frequent — today's two-agent debate consumed ~6 full CLI turns).
- Should channel history live in the rigel DB with vendor-compatible replay, or stay vendor
  storage with the core as proxy? Draft says: mirror to rigel DB, keep vendor as the live wire.
- phi4-class API agents have no rigel accounting at all today. A $0 "local model" row type
  would make the roster tell the whole truth about spend.

# Golden fixtures (P4.7)

One JSONL stream per harness per scenario. The fixture binaries replay the file verbatim, so
`tests/replay.rs` runs the production decode loop against a stream that stands still.

| Directory | Harness | Replayed by |
|---|---|---|
| `claude/` | Claude Code, `-p --output-format stream-json` | `fake-claude` (`FAKE_CLAUDE_FIXTURE`) |
| `codex/` | Codex app-server JSON-RPC notifications | `fake-codex` (`FAKE_CODEX_FIXTURE`) |
| `acp/` | ACP v1 (`agent acp` / `cline --acp`) | `fake-acp` (`FAKE_ACP_FIXTURE`) |

Scenarios, all five per harness: `start`, `tool_call`, `approval`, `error`, `interrupt`.

## Where these streams come from

They are **authored to the documented shapes**, not captured from a live binary on this machine:

- Claude: the 21 Oct 2026 `2.1.251` capture (`system/init`, `stream_event` deltas, `assistant`,
  `result`) plus the documented `tool_use` / `error_during_execution` forms. No live turn was run
  because there is no Anthropic key on this machine (D21 rules out the subscription).
- Codex: the app-server shapes listed in the vendor docs and exercised by the router probe on
  21 Oct (`item/agentMessage/delta`, `item/completed`, `thread/tokenUsage/updated`,
  `turn/completed`), plus `turn/failed` and `item/commandExecution/requestApproval`.
- ACP: the published v1 `session/update` variants and `session/request_permission`.

Re-recording them from installed binaries is P4.12's job, where a real turn per harness is already
required. The point of these files is to fail when a decode changes, and to give the next
normalisation step an exact wire shape to work from.

## What is mapped today, and what is not

`tests/replay.rs` asserts each scenario's outcome against the current decoder, including the gaps.
A gap is pinned rather than papered over, so closing it changes a test on purpose.

| Scenario | Claude | Codex | ACP |
|---|---|---|---|
| `start` | `Started` + `Delta` + `Message` + usage | `Started` + `Delta` + `Message` + usage | `MessageDelta` + `Usage` |
| `tool_call` | text, then `Item(file_change, read)` from the `tool_use` block and `Item(file_change)` for its `tool_result` | `Item(command)` with `exit_code`, then `agentMessage` → `Message` | `ToolCall` (+ `item_kind`, `operation`) + `ToolCallUpdate` |
| `approval` | CLI declines under `dontAsk`; the refusal surfaces as a failed result (P4.6 deviation) | `item/commandExecution/requestApproval` is **ignored**, no reply is sent (P4.10) | request routed to the `Approver`, answer returned on the wire |
| `error` | failed `result` → error with its text | `turn/failed` → error | JSON-RPC error response → error |
| `interrupt` | stream ends without `result` → "ended without a result line" | stdout closes before `turn/completed` → "closed its stdout" | prompt answers `stopReason: cancelled` |

Since P4.8 every decodable event lands in one of the ten kinds (§5.2), and anything unmappable is
kept as an `error` item with `unmapped: true` and the wire event under `raw`. The mapping table is
`crates/rigel-adapters/src/normalise.rs`; the scenarios above are what proves it against real
stream shapes.

Known defect recorded here rather than fixed in P4.7: Codex `turn/completed` with status
`interrupted` is classified as a completed turn. The interrupt fixture therefore uses the
stream-close signature (which is how a killed child actually looks), and the status mapping is
P4.12's to settle.

`$prompt` in an ACP fixture is replaced by the `session/prompt` request id, and `$permission` by the
fixed id `901` the fake then waits on; the prompt response a fixture carries is emitted instead of
the fake's own `end_turn`.

# Local model — setup and head-to-head race

Recorded 21 Oct 2026, Windows, NVIDIA RTX 3060 6 GB / 32 GB RAM / i7-11800H.
Purpose: pick the D69 local teammate and verify it end-to-end through the router.

## Setup verified

- Ollama 0.40.3 already installed; service answering on `localhost:11434`.
- Router connection added through the OmniRoute dashboard (`ollama-local`, name
  "Local teammate (Qwen3.5-4B on this PC)", 11 models imported). The CLI
  `omniroute providers add` failed with `fetch failed`; the dashboard flow works.
- End-to-end check through the router's playground: "Say exactly: router OK"
  answered correctly on `qwen35-4b-q4-original:latest`.

## Tool-calling test (file-based)

Both candidates were asked to inspect a file using a tool. `qwen35-4b-q4-original`
used the tool and pointed at the right file. `phi4-mini` ignored the tool and
answered in plain text — unusable for the review/deliberation roles.

## Race

Same prompt ("name three kitchen chores, one per line"), direct to Ollama:

| Model |_tokens/s_ | Answer |
|---|---|---|
| `qwen35-4b-q4-original` (default) | 10.9 | 73 lines — its thinking is written into the reply |
| `phi4-mini` | 11.9 | 3 lines, but failed the tool test above |

Same prompt on the qwen model with `think: false`: **3 clean lines, 27 tokens,
11.0 tok/s warm (~3 s round trip)**. The `think: false` switch works and is
required: without it qwen spends ~65 extra tokens narrating reasoning into the
response for a 13-token job.

## Result

**Winner: `qwen35-4b-q4-original`** — it works as a teammate only because the
runtime must pass `think: false` for cheap roles. `phi4-mini` stays installed as
a fallback for plain, non-tool jobs. Roster label stays `local / slow` (D69).

## Limits

- Two prompts, one warm pass — indicative, not a benchmark suite.
- First-call latency includes model reload when the GPU swaps between models.

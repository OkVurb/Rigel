# Research notes — round 6 (22 Oct 2026)

Ten threads, chosen because they bear on decisions already made rather than adding new features. Each
section is the evidence, then what it means for Rigel. Sources are inline.

---

## 1. Context windows are working sets

- A study of 2,303 agent context files found median **485 words for Claude Code**, **335.5 for Codex**
  (~630 and ~440 tokens). Anthropic recommends keeping `CLAUDE.md` under 200 lines; Cursor says rules
  under 500. [study](https://arxiv.org/html/2511.12884v1) · [Claude memory](https://code.claude.com/docs/en/memory)
- Claude Code compacts near a model-dependent limit; `/compact` takes a focus prompt, re-injects
  instructions and memory, and re-reads up to five recently modified files. Codex auto-compacts at a
  configured limit and replaces history with a smaller input. [Claude context](https://code.claude.com/docs/en/context-window)
  · [Codex loop](https://openai.com/index/unrolling-the-codex-agent-loop/)
- Retrieval beats stuffing: Aider ships a symbol-oriented repo map at about a **1,000-token** budget;
  Claude recommends grep/glob/LSP and subagents. Cursor's docs describe local grep and say it does not
  store code embeddings. [Aider repo map](https://aider.chat/docs/repomap.html)

**For Rigel:** the 1,000-token skills budget (P4.19) is in line with the field. What is missing is
visibility: show which instruction sources and skills loaded, and their token cost, per turn; keep
compaction inspectable and the full transcript local.

---

## 2. What an honest bill must separate

| Provider | Coding model | Input / output / cached input ($/MTok) |
|---|---|---|
| OpenAI | GPT‑5.3‑Codex | 1.75 / 14 / 0.175 |
| OpenAI | GPT‑5.4 | 2.50 / 15 / 0.25 (2× input above 272K) |
| Anthropic | Opus 5.5 | 4 / 20 / 0.20 (cache write 1.25×–2×) |
| Anthropic | Sonnet 5.5 | 2 / 10 / 0.10 |
| Gemini | 4 Argon | 2 / 10 / 0.10 cached (4,096-token cache minimum) |
| Gemini | 3.8 Flash | 0.75 / 3.75 / 0.075 |
| Ollama local | any | $0 tokens, hardware cost not counted |

Reasoning tokens bill as output on all three. Pre-turn estimates are ranges: cache assumptions,
reasoning, tool results, retries and fallbacks all move the number. Claude Code's `/usage` is a
post-hoc estimate from reported totals. [OpenAI pricing](https://developers.openai.com/api/docs/pricing)
· [Anthropic pricing](https://platform.claude.com/docs/en/about-claude/pricing) · [Gemini pricing](https://ai.google.dev/gemini-api/docs/pricing)

**For Rigel:** D92's spend view should break out uncached input, cache reads/writes, output/reasoning,
tools, retries and route — and label the pre-turn number as a range. "Local = $0" needs a footnote that
it is electricity, not free.

---

## 3. Isolation on Windows — D107's real fork

| Option | Isolates? | 1809 | Needs | Verdict |
|---|---|---|---|---|
| Windows Sandbox | separate VM, own kernel | **no** (1903+) | Pro/Enterprise, Hyper-V, reboot | best boundary when present; optional only |
| AppContainer | low-integrity process + capabilities | **yes** | API only, no admin | realistic fallback; ACL work |
| Restricted token | privileges/SIDs | yes | API only | weak alone |
| Job Objects | lifetime/CPU/memory | yes | API only | not a sandbox |
| Docker Desktop | Linux VM via WSL2 | **no** (22H2/11) | WSL2, virtualization, licence | poor assumed dependency |
| WSL2 | Linux VM, but `/mnt/c` and interop | WSL1 only on 1809 | admin/reboot | not isolation |
| App Guard | browser/Office only | deprecated | licensing | no |

Microsoft: Sandbox needs 1903+, not Home, 4 GB RAM, virtualization; networking and clipboard default on
and must be disabled in the `.wsb`. AppContainer works on 1809 and 11 with `CreateAppContainerProfile`
and `STARTUPINFOEX`. [Sandbox](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/)
· [AppContainer](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer)
· [Job objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)

**For Rigel:** D107 asked "Docker Desktop vs WSL2" — both are the wrong default. The Windows-native
answer is **Sandbox when available, AppContainer otherwise, with an explicit refusal rather than a
silent unsandboxed fallback**. Docker adds a WSL2 dependency the installer cannot promise and excludes
1809 outright.

---

## 4. Secrets

Credential Manager scopes: Session, Local, Enterprise; **Local** is per-user per-machine, Enterprise may
roam. DPAPI default binds to user + machine; `CRYPTPROTECT_LOCAL_MACHINE` lets any user on the box
decrypt. The Rust `keyring` crate is a front-end to those stores; its Windows backend defaults to
**Enterprise**, so the scope must be set explicitly. Tauri Stronghold is a file vault, not the OS store.
[CREDENTIAL](https://learn.microsoft.com/en-us/windows/win32/api/wincred/ns-wincred-credentiala) ·
[keyring](https://docs.rs/keyring/latest/keyring/v1/index.html) · [Trail of Bits on MCP creds](https://blog.trailofbits.com/2025/04/30/insecure-credential-storage-plagues-mcp/)

**For Rigel:** per-agent provider keys already go to the keychain (D14) — confirm the persistence is
**Local**, not Enterprise. MCP credentials should be injected at process launch from the keychain, never
written to `.mcp.json` or a project config, and never copied into the diagnostics bundle.

---

## 5. Git worktrees at scale

- Worktrees split `HEAD` and index but share objects, refs and config, so concurrent commits and ref
  updates still contend. Reports on Windows: stale `index.lock`, and a run where lock failures plus
  cleanup lost uncommitted work. [git worktree](https://git-scm.com/docs/git-worktree) ·
  [issue #55724](https://github.com/anthropics/claude-code/issues/55724)
- `worktree remove` refuses dirty trees unless forced; `prune` only drops metadata for missing
  directories. `--no-optional-locks` avoids lock-taking status polls. `core.longpaths` is off by
  default on Windows; case-insensitive paths make case-only collisions hazardous.
- GitHub's merge queue tests the merge group, but its April 2026 incident shows queue bugs can produce
  wrong squash results. [merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)

**For Rigel:** D80's writable worktrees should follow five rules — unique branch and short path per
agent; serialise ref/config/merge mutations inside Rigel; poll with `--no-optional-locks`; never
`remove --force`, quarantine dirty work instead; prune only verified orphans after `--dry-run`.

---

## 6. Evaluation, and why D118's harness matters

Terminal-Bench 2.0 is **89 tasks**; SWE-EVO is **48 tasks** averaging 21 files; OSWorld is **369**;
τ-bench is 165; SWE-Bench Pro is 1,865 from 41 repos. All are public and all can be contaminated; none
measures human rescue or review load. [Terminal-Bench](https://github.com/harbor-framework/terminal-bench)
· [SWE-EVO](https://arxiv.org/html/2512.18470v1) · [OSWorld](https://arxiv.org/html/2404.07972) ·
[SWE-Bench Pro](https://arxiv.org/html/2509.16941v1)

For an internal suite: **60–100 tasks**, 2–3 runs each, acceptance checks written before the run, and
report **interventions per completed task**, cost per *successful* task, review findings, rework and
p50/p90 wall time. At 60 tasks a 50% success rate carries roughly **±13 points** of 95% uncertainty; 100
tasks narrows it to about ±10. Freeze model, prompts and graders; keep a hidden holdout.

**For Rigel:** `rigel bench` should start at 40–60 tasks from real sessions rather than a leaderboard
clone, and publish intervals, not a headline.

---

## 7. ACP: what it standardises, and what it does not

ACP v1 is stable, v2 is a draft; JSON-RPC over stdio, with remote transports still in progress. It
covers initialisation/capabilities, sessions and prompts, streamed updates, cancellation, plans/diffs,
tool-call status and optional permission requests — **not** the agent's model, tools, sandbox, provider
account or durable session storage. Claude Code and Codex reach ACP through adapters; Gemini CLI,
Cline (`--acp`) and Cursor (`agent acp`) speak it directly. Zed and JetBrains run the registry.
[spec](https://agentclientprotocol.com/protocol/v1/overview) · [registry](https://agentclientprotocol.com/rfds/acp-agent-registry)
· [Zed report](https://zed.dev/blog/acp-progress-report)

**For Rigel:** hosting five harnesses over ACP is exactly the supported use. Publishing Rigel *as* an
ACP agent is possible but the public registry expects binaries for macOS, Linux and Windows — a
Windows-only build would fit a private or custom catalog better. Note also that Gemini CLI's
unpaid tier moved to Antigravity CLI in June 2026, so the harness matrix is a moving target.

---

## 8. Diagnostics without telemetry

WER local dumps are off by default and need admin to configure; even minimal dumps are not guaranteed
free of prompts or tokens. `MiniDumpWriteDump` should be called from a separate process. Event Log
queries can be scoped by XPath to the app and a time window. Comparable apps differ: Zed sends crash
reports on next launch but can be disabled; Warp uses Sentry with a toggle; VS Code has a telemetry
level including off; Raycast asks users to assemble diagnostics manually. [WER dumps](https://learn.microsoft.com/en-us/windows/win32/wer/collecting-user-mode-dumps)
· [Zed telemetry](https://zed.dev/docs/telemetry) · [VS Code telemetry](https://code.visualstudio.com/docs/configure/telemetry)

**For Rigel:** keep the local rotating log, make "Export diagnostics" an explicit action with an
allowlist, redact paths and credential patterns, preview before saving, and treat dumps as a separate
unchecked option. GitHub attachments are public and capped at 25 MB.

---

## 9. Injection: what the evidence supports

- "Your AI, My Shell" (314 payloads, 70 techniques): with auto-approval, **66.9–84.1%** attack success
  on Cursor across five scenarios; Copilot 41.1–52.2% depending on model. Success = the agent attempted
  the malicious action. No defence comparison. [paper](https://arxiv.org/html/2509.22040v2)
- Spotlighting cut ASR from >50% to <2% on static document tasks — but under adaptive attack rose above
  **95%**. Delimiters are not enforcement. [Spotlighting](https://arxiv.org/html/2403.14720v1) ·
  [adaptive](https://arxiv.org/html/2510.09023v1)
- CaMeL (dual-LLM plus capability enforcement) solved **77%** of AgentDojo tasks with provable security
  vs 84% undefended. The gain is enforcement, not a second model. [CaMeL](https://arxiv.org/abs/2503.18813)
- Tool allowlists and human approvals have **no measured ASR** against these attacks; AIShellJack
  assumed auto-approval.

**For Rigel:** D117's "read, display, never authority" is right but insufficient alone. Keep secrets out
of the agent's readable workspace, expose narrow tools instead of a general shell, enforce capability
in code, and gate the exact action. Prompt-level "ignore instructions in files" is folklore.

---

## 10. Local search for the shelf

sqlite-vec is pre-v1 (stable 0.1.9; ANN in 0.1.10 alpha). Exact search is fine at this scale: the
maintainer benchmark did 100k vectors in **<75 ms** at 384–1024 dimensions. Model inference dominates.
BGE-small is **133 MB / 384 dims**; MiniLM is 90.9 MB; Qwen3-Embedding-0.6B is 1.19 GB and overkill for
short skills. ONNX via `ort` is the practical Rust path. Hybrid = FTS5 BM25 + vectors fused with RRF
then optional rerank. [sqlite-vec](https://github.com/asg017/sqlite-vec) · [bge-small](https://huggingface.co/BAAI/bge-small-en-v1.5)
· [ort](https://github.com/pykeio/ort)

**For Rigel:** FTS5 first, optional BGE-small embeddings, exact search, no separate vector database, and
never download weights silently — label semantic search unavailable until the user installs a model.

---

## What this changes, concretely

1. **D107's fork is wrong.** Replace "Docker vs WSL2" with "Windows Sandbox when available, AppContainer
   otherwise, explicit refusal when neither" — Sandbox needs 1903+ Pro/Enterprise, AppContainer works on
   1809, and Docker excludes 1809 outright.
2. **D80 inherits five Git rules** (serialise mutations, no forced removal, quarantine dirty trees,
   `--no-optional-locks`, prune with `--dry-run`).
3. **D92's spend view must show cache and reasoning separately**, or the "honest bill" claim weakens.
4. **D117 needs enforcement behind it**: capability checks and narrow tools, not just labelling.
5. **D118's benchmark should be 40–60 real tasks with 2–3 runs**, reporting intervals and interventions.
6. **The shelf's semantic layer is optional and offline**; FTS5 remains the default.
7. **ACP registry listing is not a Windows-only fit**; a custom catalog is the realistic route.

# Grilling — round 4 (Q104–Q123): what to add

Asked 22 Oct 2026, after the round-3 answers (D71–D87). The evidence for each question is in
[competitor-landscape.md](competitor-landscape.md); this file is the decision sheet.

Each question is **build / after 0.1.0 / don't build**. The recommendation weighs three things: does it
strengthen something Rigel already does better than the field (budgets, claims, attributed diff), does a
live competitor already make it table stakes, and does it fit before a private beta.

Answer out loud, by number. Answering "all of them" is the one answer that is not allowed — this is a
ranking exercise, and the build queue is already long with D74–D83.

---

### Q104 — Inline comments on diff hunks

Conductor, Vibe Kanban and Cursor all let a reviewer comment on a hunk and send it back. Rigel's Diff
tab shows per-hunk attribution but only has per-patch revert.

**Build:** a comment box per hunk, stored against the patch, delivered to the authoring agent as a turn.
Small — the hunk rows already carry the agent and the patch id.

➡️ **Build for 0.1.0.** It is the cheapest review feature and it completes the attributed diff: seeing
who wrote a hunk is only half of what a reviewer needs.

**Answer:**

---

### Q105 — Per-turn checkpoint and restore

Windsurf has named checkpoints, Zed has Restore Checkpoint, Kilo snapshots per user message. Rigel has
`git stash create` snapshots only before a destructive command (P4.17) and per-patch revert.

**Build:** extend `snapshot.rs` to take a checkpoint at the end of every turn, list them in the
transcript, and restore any of them. The storage and restore path already exist; the trigger and UI are
new.

➡️ **Build for 0.1.0.** "Undo that turn" is the safety promise users assume, and the expensive half of
the machinery is already written and tested.

**Answer:**

---

### Q106 — Desktop notifications

crystl.dev and Superset notify on approvals, questions and completion; both treat it as a reason to
leave the app in the background.

**Build:** a Tauri notification on three events — approval waiting, turn finished, turn failed — with a
per-event toggle and no sound by default.

➡️ **Build for 0.1.0.** It is hours of work and it is what makes a long unattended turn usable instead
of a tab you keep checking.

**Answer:**

---

### Q107 — Rules interop (`AGENTS.md`, `CLAUDE.md`)

Every serious agent reads a rules file. Rigel has its own per-agent memory (D2/D43), and today ignores
the files a repo already has.

**Build:** read `AGENTS.md`, `CLAUDE.md` and `.cursor/rules/*` into the session preamble, expose Rigel's
memory as a generated `AGENTS.md` block, and show in Settings which sources were used.

➡️ **Build for 0.1.0.** A repo that already documents its conventions should not have to repeat them for
Rigel, and this is the cheapest adoption feature on the list.

**Answer:**

---

### Q108 — Cross-session spend dashboard and per-task caps

Claude Code has `/usage` and `--max-budget-usd`; Agent Deck has a cost dashboard; Rigel enforces a cap
per session (D10) and shows spend per agent.

**Build:** a spend view across sessions and projects, plus an optional per-turn or per-task cap below the
session cap.

➡️ **Build for 0.1.0.** Rigel's budgeting is its differentiator, and today it cannot answer "what has
this project cost me this month?" — the question every user asks in week two.

**Answer:**

---

### Q109 — skills.sh import with its audit badge

skills.sh now shows Gen/Socket/Snyk audit results and hides malicious skills from search. Rigel indexes
GitHub and local sources only.

**Build:** an importer that pulls a skills.sh entry by URL, records the audit verdict and its date as
trust evidence, and keeps Rigel's own arithmetic as the visible score.

➡️ **Build for 0.1.0.** It gives the shelf reach without giving up the trust model, and the audit
verdict is exactly the kind of dated evidence P6.3 already knows how to display.

**Answer:**

---

### Q110 — Agent role presets

Vibe Kanban has agent profiles; crystl.dev ships custom agents and a prompt library. Rigel makes you
write a prompt and pick a model every time.

**Build:** named presets — implementer, reviewer, tester, researcher — each a saved system prompt,
model/effort pair and tool allowance, applied from the roster.

➡️ **Build for 0.1.0.** The multi-agent story is hard to start from a blank prompt; presets are what turn
"spawn an agent" into "give this to the reviewer".

**Answer:**

---

### Q111 — Session export as a readable bundle

Nobody does this well. Rigel has the transcript, the patches and the bill in SQLite.

**Build:** one `.md` per session with the prompt, turns, tool calls, per-agent spend and linked patches,
plus the JSON alongside it. The diagnostics export (D82) already assembles a bundle; this is the
human-readable sibling.

➡️ **Build for 0.1.0.** It costs almost nothing, and it is how a user shows a colleague what happened
without handing over a database.

**Answer:**

---

### Q112 — Shelf search as an MCP tool

Rigel's MCP server exposes claims, budget and approval tools, but not the skill index itself. The
ecosystem has no local inventory an agent can query; agents guess at what is installed.

**Build:** `rigel_skills_search` and `rigel_skills_show` over the local index, returning id, name,
source, trust score, pins and the install path.

➡️ **Build for 0.1.0.** Tiny, and it is the one feature no marketplace can copy, because the index is
local and the answer includes what is actually installed on this machine.

**Answer:**

---

### Q113 — PR creation from the Diff tab via `gh`

Vibe Kanban and Conductor both end in a PR. Rigel ends at "merge into the working tree".

**Build:** detect `gh`, offer "open a pull request" on a patch set, compose the title and body from the
turns, never push without an explicit click.

➡️ **Build for 0.1.0.** The team story dies at a merged local tree; a PR is the handoff most users need,
and `gh` keeps Rigel out of the credentials business.

**Answer:**

---

### Q114 — Plan mode with a reviewable plan artifact

Cline has Plan/Act, Gemini CLI has a read-only Plan Mode, Cursor has one too. Rigel's native agent goes
straight to tool calls.

**Build:** a read-only first phase that writes a plan and proposed claims to the transcript, waits for
approval, then executes under the same budget.

➡️ **Build for 0.1.0.** It is also a safety feature: the destructive classifier and the approval card
work better when the user has already seen the shape of the work.

**Answer:**

---

### Q115 — Task board: tasks bound to agent and worktree

Vibe Kanban's whole product is this; Kilo's Agent Manager is a lighter version. Rigel has Swarm, which
broadcasts one prompt, and a roster.

**Build:** a task list where each task owns a worktree, an agent and a status, with the diff and the
review attached. This is the big one — it changes the Session tab's information architecture.

➡️ **After 0.1.0.** Do it once D80 (writable subagents) exists; a board over read-only agents would be a
pretty lie.

**Answer:**

---

### Q116 — MCP catalog with trust cards

Kilo's marketplace and Roo's list MCP servers; Glama builds and runs servers in microVMs to score them.
Rigel can add MCP servers but does not help you choose one.

**Build:** a local catalogue of MCP entries with transport, required credentials, last-seen schema and a
Rigel trust card. Starting read-only from the official registry is much cheaper than scanning.

➡️ **After 0.1.0.** The scan/verify half is a product of its own; a read-only catalogue can come sooner
if the shelf's trust UI generalises well.

**Answer:**

---

### Q117 — Skill lockfile export/import

The Agent Skills ecosystem has an open RFC for `skills.json` plus a content-hash lockfile, and no
product ships it. Rigel already pins skills per session (D9/D59) and records pins.

**Build:** write `rigel.skills.lock` with source ref, commit, content hash and trust evidence; import it
on another machine before indexing.

➡️ **After 0.1.0.** Valuable, but the standard is unsettled and a Rigel-only lockfile solves a smaller
problem than it looks. Revisit when the RFC lands.

**Answer:**

---

### Q118 — Mobile read-only view over the room

crystl.dev has an iPhone app; Superset's mobile needs Pro; Conductor's mobile is $50/month. D87 already
puts a LAN-bound, authenticated room in 0.1.0.

**Build:** a small responsive page served by the room's server, read-only, same token, no new app.

➡️ **After 0.1.0.** D87 is the prerequisite, and a read-only page is the honest first step — remote
*control* is a different security decision.

**Answer:**

---

### Q119 — Remote approvals from a phone

crystl.dev does this and treats it as the reason for its mobile app. It follows naturally from Q118.

**Build:** allow/deny an approval card from the read-only page, with the same four scopes.

➡️ **After 0.1.0.** Only after the read-only view has been used for a while; approval is the first
remote action that can destroy work.

**Answer:**

---

### Q120 — Issue import (GitHub, Linear)

Vibe Kanban starts from issues. Rigel starts from a prompt.

**Build:** pull open issues into the task board with a label filter, one task per issue.

➡️ **After 0.1.0.** It depends on the board (Q115) existing, and it is an integration per tracker.

**Answer:**

---

### Q121 — Lifecycle hooks

Claude Code and Kiro both ship hooks around tool calls and turns. Rigel has approvals and tools but no
scripting surface.

**Build:** pre/post-turn shell hooks from the project config, with the command shown before it first
runs.

➡️ **After 0.1.0.** Powerful and dangerous; the MCP tools cover most of the same ground without handing
users a second, less-governed scripting layer before the approval model is battle-tested.

**Answer:**

---

### Q122 — Merge queue with ordered landings

Vibe Kanban and Conductor both serialise merges. Rigel merges each patch through CAS and reports
conflicts, but has no queue or ordering.

**Build:** a queue that lands one patch set at a time, re-running the clean-merge check before each and
pausing on the first conflict.

➡️ **After 0.1.0.** Real value only when several writable agents merge at once, which is D80.

**Answer:**

---

### Q123 — Container sandbox execution mode

Container Use and Cursor both isolate execution in containers or sandboxes. Rigel relies on approvals,
cwd jails and snapshots.

**Build:** an optional per-agent container for `run_command`, with the project mounted and the network
off by default.

➡️ **Don't build for v1.** It is a second execution model, it contradicts the local-first simplicity
that makes the app installable, and the approval path already fails closed. Revisit if a user asks for
untrusted-repo work.

**Answer:**

---

## Answers (given 22 Oct 2026)

| Q | Feature | Answer | Decision |
|---|---|---|---|
| 104 | Hunk comments | build 0.1.0 | D88 |
| 105 | Per-turn checkpoints | build 0.1.0 | D89 |
| 106 | Notifications | build 0.1.0 | D90 |
| 107 | Rules interop | build 0.1.0 | D91 |
| 108 | Spend view + per-task cap | build 0.1.0 | D92 |
| 109 | skills.sh import | build 0.1.0 | D93 |
| 110 | Role presets | build 0.1.0 | D94 |
| 111 | Session export | build 0.1.0 | D95 |
| 112 | Shelf over MCP | build 0.1.0 | D96 |
| 113 | PR via `gh` | build 0.1.0 | D97 |
| 114 | Plan mode | build 0.1.0 | D98 |
| 115 | Task board | after 0.1.0 | D99 |
| 116 | MCP catalogue | after 0.1.0 | D100 |
| 117 | Skills lockfile | after 0.1.0 | D101 |
| 118 | Mobile read-only room | after 0.1.0 | D102 |
| 119 | Remote approvals | after 0.1.0 | D103 |
| 120 | Issue import | after 0.1.0 | D104 |
| 121 | Lifecycle hooks | after 0.1.0 | D105 |
| 122 | Merge queue | after 0.1.0 | D106 |
| 123 | Container sandbox | **build 0.1.0 — against the recommendation** | D107 |

Q123 is the one override. It conflicts with D56 ("no container isolation in v1"), and a container
backend needs Docker Desktop or WSL2, which the installer does not ship. The only shape that keeps the
PLATFORMS promise is an optional backend that detects a runtime and falls back to the approval path when
there is none — recorded in D107, and the first item to design before code.

# Grilling — round 8 (Q196–Q215): the window's quality, and six things competitors do not have

Asked 30 Oct 2026, after round 7's answers were recorded as D127–D146. The window has six rail
surfaces — Shelf, Session, Chat, Diff, Terminal, Settings — in **one** Tauri window, a session list
that is sorted by recency and nothing else, and a composer that can attach files and wake the room.
Round 7 asked what the surfaces should grow; this round asks what it is like to *use* them for a
day, and then what could be built that no competitor has. The state each question builds on is in
[what-works.md](what-works.md); the settled decisions are D1–D146 in [handoff.md](handoff.md).

**Answered 30 Oct 2026 — decisions D147–D166 in [handoff.md](handoff.md) §3.1.** Nineteen answers
follow the recommendations below; Q215 overrode one, so shared control is recorded with the identity
work and the two-approval rule that must precede it.

Every question would land code. Each carries the honest cost, the options and one recommendation;
"not yet" and "never" are real answers. The first four sections are ordinary quality-of-life work.
**Section E is deliberately hard** — features that need a new subsystem, not a new panel — and each
one names the narrow first version that would make it honest. Answer with a letter, "no", or your
own words in the blank.

---

## A. Finding things

### Q196 — Is there one search box for the whole workspace?

Three searches exist and none of them knows about the others: the shelf searches skills with FTS5
(`skills.search`), the model picker searches its own roster, and sessions, turns, patches and notes
are SQLite rows with no search surface at all. Finding "that merge note from Tuesday" means opening
sessions one at a time and scrolling.

- **A.** A `Ctrl+K` palette with sections — actions (open a view, start a session), skills,
  sessions, turns and patches — backed by one `search_all` core call, capped per section, where
  choosing a turn opens its session scrolled to that row. Actions make it a command palette; the
  rows make it a search. *(recommended)*
- **B.** Four separate searches, one per surface. Cheaper per surface, four places to remember.
- **C.** No; the shelf's search is the search.

➡️ **A.** The core already owns every row; the palette is a renderer over one command, and it is the
only version that answers "where did I see that" without knowing which surface it was on. The cost
is a turn-body index and a result-ranking rule that does not put a skill above the session you were
in yesterday.

**Answer:**

---

### Q197 — Can the session list be managed, or only scrolled?

`session_list` returns sessions newest first, but the window renders no list at all: `SessionView`
opens the newest non-ended session and there is no way to switch or find an older one. There is no
rename, pin, archive or filter either. After a week there are thirty sessions named by their
opening prompt and only the newest is reachable.

- **A.** A session list with rename, pin, archive and a text/status filter; archived sessions are
  hidden by default, still searchable and still resumable. Migration adds `pinned_at` and
  `archived_at` (`title` already exists). *(recommended)*
- **B.** Pin only. It fixes the daily case and leaves the long tail.
- **C.** Leave the list as it is; the palette (Q196) is the way back.

➡️ **A.** The list is the missing piece — the rows exist, nothing draws them. Two nullable columns
and one list component; the automatic title stays the fallback, so nothing has to be named. This is
the cheapest large improvement to the window in this round.

**Answer:**

---

### Q198 — Can a long transcript be navigated instead of scrolled?

A turn can run sixty steps. Tool lines collapse, and D136's filter is approved but not built, so the
transcript is one long column with no map: three diffs that matter sit between forty read results
and there is no way to jump between the six user prompts above them.

- **A.** A turn outline: a narrow rail of marks — one per user prompt, one per diff, one per error,
  one per approval — with a sticky "now" indicator, click to jump, and the marks derived from rows
  already in memory. *(recommended)*
- **B.** Search inside the transcript only. Useful, but it needs a query before it helps.
- **C.** No; collapse is enough.

➡️ **A.** It is a render-time index over the same rows, no schema and no new state. It also gives
D136's filter somewhere to show its counts.

**Answer:**

---

### Q199 — Can a turn or a hunk be bookmarked?

The transcript and the Diff view are both read-only ledgers with no place to leave a mark. A merge
note you will need after lunch, or the one hunk you want to review with someone, is found by
scrolling and remembering.

- **A.** Star any transcript item or patch hunk; a Bookmarks section per session lists them with
  jump links and survives a restart. Stored in the project database beside the rows they name.
  *(recommended)*
- **B.** Copy the text into a scratch note instead. No new state, no jump back.
- **C.** No.

➡️ **A.** Two tables or one polymorphic row, and one list; the value is the jump, not the star. It
also gives the later "hand to" and review actions a natural selection mechanism.

**Answer:**

---

## B. Composing

### Q200 — Does the composer remember what you typed before?

There is no prompt history: `↑` does nothing, and every past prompt is already a `turn` row. A
prompt you refined three times yesterday is retyped today. The composer also has no saved text.

- **A.** `↑` / `↓` cycles this session's prompts (with a modifier for all sessions), boundary-aware
  at the start and end of the text, plus a small saved-snippets menu with `{placeholders}`.
  *(recommended)*
- **B.** History only. Half the work, most of the value.
- **C.** Snippets only.

➡️ **A**, with history first: history is a read over rows that exist, and snippets are a settings
list. Neither ever sends without the send button.

**Answer:**

---

### Q201 — Can a session start from a template?

Starting a session means retyping the same shape — role presets (D94), a model, an effort, a step
budget, the same three pinned skills — every time. There is no saved setup and no first-prompt
skeleton.

- **A.** Save a session's setup (model, effort, steps, agent roster, pinned skills, prompt skeleton)
  as a named template, offered at create; templates are project-local rows. *(recommended, after
  the beta)*
- **B.** Role presets are enough; templates are a second settings surface for the same idea.
- **C.** Templates as committed files in the repository, so a team shares them.

➡️ **A**, after the beta. C is tempting for teams, but it puts machine-specific model ids in a
committed file, which N7 forbids; a project-local row is the honest version until a team feature
exists.

**Answer:**

---

### Q202 — Before a turn runs, does the window say what it will cost?

Cost is visible after the fact: usage rows, a running total, an 80% warning, a cap. Before send,
the composer says nothing — not the likely range, not the files the prompt will probably touch, not
whether the chosen model can even see the attachment.

- **A.** A preflight line under the composer: an estimated cost range from the last N comparable
  turns (same model and effort), a token estimate from the prompt and attached files, and "no
  history yet" when the sample is too small. A warning, never a block; the range is shown as a
  range. *(recommended)*
- **B.** A confirmation dialog above a threshold. One number, and a click that trains people to
  dismiss it.
- **C.** No estimate; the cap is the protection.

➡️ **A.** The data already exists; the honesty rule is the feature. A preflight that guesses a
single number is worse than none, because it will be wrong in the direction that spends money.

**Answer:**

---

## C. Understanding what happened

### Q203 — Is there one list of everything that needs attention?

Approvals are modal, failures are inline in one transcript, budget stops are banners, stale claims
and blocked merges live in the Diff view. Nothing aggregates them, so a session that stopped for
approval an hour ago is invisible from the Shelf.

- **A.** An Attention view (and a count on the rail) listing pending approvals, failed turns, budget
  stops, stale claims, unmerged reviews and blocked merges, grouped by session, each row a jump
  link, cleared by resolving it. Reuses the coordination poll. *(recommended)*
- **B.** Only the notification path (D139) — it tells you when something happens, not what is still
  waiting.
- **C.** No.

➡️ **A.** It is a query plus a list, and it is the difference between a window you watch and one you
can leave. It must show *unresolved* things only, or it becomes a second transcript.

**Answer:**

---

### Q204 — After an hour away, what changed?

The session list is sorted by recency; nothing says which turns finished while the window was
hidden in the tray, what merged, or what is waiting. Notifications only fire while the process runs
and only for three events.

- **A.** A "While you were away" card on return: turns finished, patches merged, approvals waiting,
  spend since the last time the window was focused, dismissible, with links. *(recommended)*
- **B.** A daily digest file written to app data.
- **C.** No.

➡️ **A.** One timestamp per window focus and one query; it complements D139 rather than duplicating
it. B is a report nobody opens.

**Answer:**

---

### Q205 — Can a changed line be traced to the turn that produced it?

The Diff view names the author agent, the patch and the result hash. It does not connect a line to
the tool call, the model or the prompt that produced it, so "why is this line like this" ends at the
patch boundary.

- **A.** Click a line to see its hunk, the patch, the turn, the tool call and the model that wrote
  it, read-only, with "unknown" for lines that predate the ledger. *(recommended)*
- **B.** Per-file history only: which turns touched the file, no line mapping.
- **C.** No.

➡️ **A**, with the honest limit stated in the UI: provenance is exact at the hunk boundary and
approximate inside it. That is still far more than any competitor shows, and the ledger already has
the hashes.

**Answer:**

---

### Q206 — Can the bill be explored, not just totalled?

There is a spend view (D92) and per-agent usage rows. There is no way to ask "what did this project
cost on Tuesdays", "which model is actually cheapest for this work", or "what would this session
have cost on the other door".

- **A.** A ledger explorer: group by day, project, agent and model; a counterfactual column pricing
  the recorded tokens at another model's rates, labelled as arithmetic on recorded tokens, not a
  prediction. *(recommended)*
- **B.** Totals only.
- **C.** Export CSV and use a spreadsheet.

➡️ **A.** It is a query and a table over rows that already exist, and the counterfactual is the one
number that would change how people pick models. Never call it a saving.

**Answer:**

---

## D. The window itself

### Q207 — Is the window usable without a mouse, and with a screen reader?

The rail buttons have labels and the shelf rows are buttons, but the transcript's structure is
mostly visual, focus order follows the DOM rather than the task, and there is no high-contrast
theme or reduced-motion path. N8 sets a visual bar; this is the other half of the same promise.

- **A.** An accessibility pass: transcript as a labelled list with roles and turn boundaries,
  `:focus-visible` everywhere, skip links, reduced motion honoured, a high-contrast palette, and a
  keyboard-only walkthrough that is part of the exit test. *(recommended)*
- **B.** Fix only what a screen reader reports on this machine. Cheaper, and it misses keyboard-only
  users.
- **C.** No.

➡️ **A.** A coding agent is a keyboard product; if the window cannot be driven from one, the
instrument claim is cosmetic. The cost is a pass over the shell and one new palette.

**Answer:**

---

### Q208 — Can the window be comfortable on a 13-inch laptop and a 32-inch monitor?

The default window is 1280×800 and the shell is four columns at a 960 px minimum. D125 tuned the
squeeze, but there is no UI scale, no compact density and no persisted pane widths; only the
terminal has a font setting.

- **A.** A UI scale (90/100/110/125%), a compact row density, draggable pane widths persisted per
  project, and a "fit" rule that collapses the inspector before it crushes the transcript.
  *(recommended)*
- **B.** A compact toggle only.
- **C.** No; resize the window.

➡️ **A.** Tokens already drive type and spacing, so scale is one root value; the splitters are the
larger half. This is the least glamorous and most felt item in the round.

**Answer:**

---

### Q209 — Is one window enough, and does the taskbar know what is running?

Closing hides the window to the tray and agents keep working, which is right. The taskbar shows the
same icon whether nothing is running or three agents are, and there is no per-project window, so two
projects means switching in-app (D24).

- **A.** A taskbar overlay badge (and thumbnail button on Windows) with the running-agent count and
  a "needs attention" state, keeping one window. *(recommended)*
- **B.** Multiple windows, one per project. It reopens D24 and multiplies which window owns the tray,
  the services and the terminal.
- **C.** No.

➡️ **A.** The tray already knows the process tree; this is one indicator and one state query. B is a
different product decision and should not be smuggled in as a QoL fix.

**Answer:**

---

## E. The hard ones — what competitors do not have

These are not panels. Each needs a new subsystem or a new guarantee, and each names the narrow
first version that would make it honest rather than a demo.

### Q210 — Can a recorded turn be replayed, step by step?

Every tool call and result is a transcript item, but replaying a turn means re-running the tools,
which is unsafe and often impossible. The tests have a deterministic scripted model; the window has
nothing. Debugging a bad turn is still reading.

- **A.** Full deterministic replay: record a complete input tape (messages, tool results, model
  choices) and short-circuit tools on replay, so the turn can be stepped through without side
  effects. Hard: every tool needs a recorded result, and a replay that diverges must stop rather
  than guess.
- **B.** A read-only step-through viewer over the recorded items: next/previous, inputs and outputs
  side by side, diffs inline, no re-execution. *(recommended first)*
- **C.** No.

➡️ **B** now, A only if a real debugging need appears. B is a viewer over rows that exist and is
useful immediately; A is a new contract between the loop and every tool, and a half-correct replay
is a debugging trap.

**Answer:**

---

### Q211 — The same task, two models, side by side?

A fork (D129) is approved but not built; worktrees, claims and diffs exist. Nothing runs one prompt
twice and compares the results, so model choice is still faith and a bill.

- **A.** A comparison run: fork twice from the same transcript prefix, run the same prompt on two
  models in two worktrees, show the two diffs, two bills and two test results side by side, merge
  one. *(recommended, after D129)*
- **B.** Sequential: run one, review it, then the other. Half the spend, no side-by-side.
- **C.** No.

➡️ **A** after D129. It is the feature that makes a multi-model product mean something, and it is
also the most honest benchmark a user can run on their own work. It doubles spend and needs a
two-worktree UI, so the cap and the merge gate must both hold.

**Answer:**

---

### Q212 — Before approving a destructive command, can you see its blast radius?

The classifier (D32) names policy rows and the approval shows the exact command, cwd and timeout.
It does not list the paths the command will touch, so "recursive remove" is a category, not a file
count.

- **A.** A read-only preview: parse the command for targets (`rm`, `mv`, `git clean`, `git reset`),
  resolve them against the cwd, list the existing paths and a count, and say "cannot determine" for
  everything else. Never claims completeness, never executes anything. *(recommended)*
- **B.** A dry run only where the tool supports one (`git clean -n`, `rm -i` is interactive and not
  a preview). Nothing for the rest.
- **C.** No.

➡️ **A.** The classifier already parses the command; extending that parse to targets is bounded, and
the honesty rule ("this is what we can prove, not everything it may do") is what keeps it from being
a false assurance. It also makes the snapshot feel earned.

**Answer:**

---

### Q213 — Can a good session become a skill?

The shelf indexes `SKILL.md` bodies and audits them; the best workflow you ran last week exists only
as a transcript. There is no authoring path, so the durable artifact is always someone else's skill.

- **A.** "Distill to skill": the agent drafts a `SKILL.md` from the session's successful steps, plus
  a fixture or test and a suggested trigger list; it lands in a local draft source, is shown for
  review, and is never auto-pinned. *(recommended, after the beta)*
- **B.** Draft the `SKILL.md` only, no fixture. Faster, and it produces prose nobody can verify.
- **C.** No; skills come from repositories.

➡️ **A**, after the beta. The fixture is the whole idea: a distilled skill that has not run is a
blog post. It needs a sandbox to execute the fixture and the same trust rules as any imported body
(D117).

**Answer:**

---

### Q214 — Which files keep breaking?

The patch ledger records every apply, revert, conflict and result hash. Nothing rolls that up, so
the file that has been reverted three times this month looks the same as one that has never moved.

- **A.** A read-only heatmap in Diff (and a column in the Shelf): per file, patches applied,
  reverted, conflicted and merges blocked over a window; click to filter the patch list. Small
  samples labelled as small. *(recommended)*
- **B.** A text report in `doctor`.
- **C.** No.

➡️ **A.** Arithmetic over rows that exist, and the first feature that turns the ledger into
foresight. It must not imply a file is risky because it is merely busy.

**Answer:**

---

### Q215 — Can two people share one live session?

The room is a LAN chat with per-agent auth (D87); a session is single-user, approvals are local, and
the terminal is yours. Two people working one task means screen-sharing, which is the thing this
product was built to replace.

- **A.** Shared control: two humans see the same session, presence and cursors; either can approve,
  pause or send. Needs per-person identity, not per-agent tokens, and a rule for two approvals at
  once.
- **B.** A read-only observer link first, after D87 has been used on a second machine: the
  transcript and the running state, no controls.
- **C.** No; the room is the shared surface, and a session stays one person's. *(recommended)*

➡️ **C** for 0.1.0. A shared session is the most dangerous feature in this round — it makes approval
remote, and approval is the first remote action that can destroy work — and the room already carries
the multi-machine story. If it is ever built, B is the only honest first step, and it still needs a
real identity model.

**Answer:**

---

## If you only answer five

**Q196** (one search box), **Q202** (cost preflight), **Q203** (attention inbox), **Q210** (step
through a turn) and **Q213** (distill a session into a tested skill). The first three change what an
ordinary afternoon feels like; the last two are the only items here a competitor could not copy in a
sprint.

Back to the [handoff](handoff.md) · [what works](what-works.md).

# Grilling — round 7 (Q176–Q195): twenty features the new surfaces invite

Asked 27 Oct 2026, after the light theme and the Appearance page landed (D126) and the picker's
chat-only filter became a saved setting (D124). Round 6 (Q136–Q175) asked what the agent spine
needed; this round asks what the window can now do that it still cannot. The state each question
builds on is in [what-works.md](what-works.md); the settled decisions are D1–D126 in
[handoff.md](handoff.md).

**Answered 30 Oct 2026 — decisions D127–D146 in [handoff.md](handoff.md) §3.1.** Two answers
redirected a question rather than picking an option: Q177 is covered by attaching a project file
**in place** (D146), and Q195 grows the curated provider table rather than keeping it closed (D145).

Every question is a feature that would land code. Each carries the honest cost, the options and one
recommendation; "not yet" and "never" are real answers, and the recommendations that cross the
signed-beta gate say so. Answer with a letter, "no", or your own words in the blank.

---

## A. The composer — what goes into a turn

### Q176 — Can an attached image reach a model that can see?

`attachment_import` copies a file into `.rigel/attachments` and the next turn's prompt lists the
paths. That is the whole contract: `ChatMessage.content` is `Option<String>`, and `read_file`
answers `BINARY` for anything that is not UTF-8. A screenshot attached today is therefore a path
no model can look at, and the picker cannot tell a vision model from a text-only one.

- **A.** Inline the bytes for models that declare image input — `image_url` on the OpenRouter and
  OpenAI-compatible rows, an `image` block with base64 on Anthropic, `inline_data` on Gemini — with
  MIME sniffing rather than the extension, a 5 MB cap, a downscale above 2,000 px, and a
  "text-only model" note in the composer when the picked row cannot take one. Images the user did
  not attach are never read. *(recommended, after the beta gate)*
- **B.** Keep paths and refuse images explicitly, with the reason in the composer.
- **C.** OCR every image to text, losing layout and adding a dependency.

➡️ **A.** It is the input a coding agent meets most often that the window cannot carry, and the
adapters already have a per-provider encoder. The cost is four wire encoders and a capability field
in the catalogue — one of the largest changes here, which is why it is not a pre-beta item.

**Answer:**

---

### Q177 — Does `@` autocomplete project files in the composer?

The window has no file tree and no path completion; the agent has `list_files`, the person has a
text box. Typing four nested paths from memory is the most common way a good prompt gets shorter
than it should be.

- **A.** A `project_files` command that walks the project with the watcher's ignore rules, capped
  at 20,000 entries, plus an `@` popover that inserts the relative path. It never inserts contents,
  so the turn still reads the file through the tools and the hash stays honest. *(recommended)*
- **B.** A file-tree pane instead. It competes with the four-column shell and is a bigger surface.
- **C.** No; paths are part of the prompt.

➡️ **A.** One read-only command and one popover; it makes the existing filesystem tools reachable
without memorising the tree. The cap is the whole safety story.

**Answer:** No popover. The add-file picker already carries a path, but it copies the file into
`.rigel/attachments`; the answer is to make a project file attach **in place** instead (D146). That
covers the need this menu was for without a second way to name a path.

---

### Q178 — Does an unsent draft belong to its session?

`SessionView` keeps one `text` state. `selectSession` clears the transcript, agents and patches but
not the composer, so a draft typed in one session follows you into another; a restart drops it.
Neither behaviour is a decision — it is where the state happens to live.

- **A.** One draft per project + session in `localStorage`, restored when the session is selected
  and after a restart, cleared on send. The key names the project root and session id, never the
  prompt. *(recommended)*
- **B.** Clear the composer on every session switch. It fixes the leak and loses the draft.
- **C.** One global draft. It is what exists now, minus the restart.

➡️ **A.** Drafts are the cheapest unfinished thought in the window, and a coding prompt is often
written in three sittings. The storage is the same trust boundary as the transcript: local, and
readable only by the app.

**Answer:**

---

## B. Sessions and projects

### Q179 — Can a session branch from a turn?

`session_resume` reconciles a crashed turn; it does not fork. A session is a straight line, so
"try the same task with the other model" means a new session and a copied prompt, without the
transcript that made the context.

- **A.** Fork from a chosen turn into a new session: copy the transcript prefix, agent roster and
  pinned skills, give the fork a `parent_session_id`, and let it pick a different model. Patches
  stay with the original; the fork starts from the tree as it is now, not a snapshot.
  *(recommended, after the beta gate)*
- **B.** Rerun in place, replacing the later turns. It edits history and makes the bill harder to
  explain.
- **C.** No; export and start again.

➡️ **A.** It is the natural counterpart to resume, and the transcript is already rows keyed by
turn — but the copy rule and patch ownership need a schema change, so it waits.

**Answer:**

---

### Q180 — Do model, effort and step budget have per-project defaults?

`Settings` is one `settings.toml` for the machine; caps and the safety knobs are project rows. A
person with a cheap side project and an expensive one changes the picker by hand each time, or pays
the expensive default by accident.

- **A.** Three nullable project columns (`model`, `effort`, `max_steps`) in the project database,
  each with an "inherit" state; the composer's choice still wins for one turn. Precedence is
  turn > project > app, written once in the core. *(recommended)*
- **B.** A settings file in the repository. It is machine-specific state in a file that gets
  committed, which N7 forbids.
- **C.** Keep one global default.

➡️ **A.** The project database already holds the cap and the safety rows; three more columns make
the defaults match the rest of the per-project state. The precedence rule is the feature, not the
fields.

**Answer:**

---

### Q181 — Can Rigel import a session from Claude Code or Codex?

`session_export` writes Markdown plus JSON under app data, redacted. There is no inverse: history
made in another harness cannot be searched beside Rigel's own, even though the room already runs
those harnesses.

- **A.** Read-only import: parse Claude Code's JSONL and Codex's session files into turns with an
  `origin` tag, searchable in the project, never resumable and never replayed as tool calls.
  *(recommended, after the beta gate)*
- **B.** Full resume, taking ownership of another tool's transcript. The formats move and the
  semantics do not line up; a wrong resume is worse than no import.
- **C.** No.

➡️ **A.** The value is one searchable history, not pretending Rigel wrote it. Two parsers, both
marked best-effort, and a visible origin on every imported turn.

**Answer:**

---

## C. Agents working alongside each other

### Q182 — Two writable children, one task: can you pick the better diff?

Write mode already gives a child a plan you approve, its own worktree, a claim per assigned file, a
clean-context reviewer and a merge gate. Nothing compares two attempts: a second child on the same
files is a claim conflict, not a race with a winner.

- **A.** A declared race: two children, the same file list, both claims recorded as a race rather
  than a conflict, their pending diffs side by side, one merged and the other's worktree pruned.
  The bill is two attempts and the cap still applies. *(recommended, after the beta gate)*
- **B.** Sequential: run one, review it, run the second only if the first is rejected. Half the
  spend, none of the comparison.
- **C.** No; one writer at a time is the product.

➡️ **A** is the feature that makes "several agents on the same project" mean something beyond a
shared room — but it doubles spend by design, so it should not arrive before a measured fan-out has
shown the cap holds.

**Answer:**

---

### Q183 — Can an agent pause instead of cancel?

Cancel is the only brake: `cancelTurn` ends the turn as `cancelled`, and continuing means a fresh
turn with the same prompt. A tool call halfway through a large read has no stopping point between
"let it finish" and "throw it away".

- **A.** Pause at the next step boundary: status `paused`, the message list, skills and remaining
  budget kept, no room held; Resume continues the same turn. *(recommended)*
- **B.** Keep cancel, and add a "Continue" prompt that starts a new turn from the transcript.
- **C.** No.

➡️ **A.** The loop already rebuilds its message list per step, so the boundary exists; what is
missing is a status and a resume path. It is also the honest version of the stop button people
press when they want to read before spending more.

**Answer:**

---

### Q184 — Hand a finished turn to another agent?

The room can assign file-scoped work (`assign_agents`) and the transcript records which agent did
what, but a finished turn is a dead end: there is no "now let the reviewer look at it" action that
carries the summary and the changed files.

- **A.** A "Hand to…" action on a finished turn: pick an agent, get a prefilled prompt naming the
  files and the outcome, edit it, send it as that agent's turn. Nothing starts without the send.
  *(recommended)*
- **B.** Automatic handoff on finish. It spends without a click and surprises the room.
- **C.** No; type the next prompt.

➡️ **A.** The team and the file list already exist; this is one button and one prefilled prompt.
The review-before-send rule is the reason it is safe.

**Answer:**

---

### Q185 — Can each agent have its own model?

The composer's model is the session default and `spawnAgent` copies it once. The roster shows
`agent.model`; nothing edits it. `spawn_agents` has an eight-model cheap allowlist, so a child
started by the coordinator is already not necessarily on the parent's model — the rule exists, the
surface does not.

- **A.** A per-agent picker at spawn and in the roster, defaulting to the session model, with the
  subagent allowlist as the default for coordinator-spawned children and a recorded note when a
  model changes mid-session. *(recommended)*
- **B.** At spawn only, never edited; the audit stays one model per agent.
- **C.** One model for the whole session.

➡️ **A** is what "multiple providers for multiple agents" means in practice (D26); the transcript
row, not a hidden setting, is what makes it auditable.

**Answer:**

---

## D. Reading the window

### Q186 — Can the transcript show only the parts you are reading?

Tool lines collapse, and there is a raw/debug toggle, but a twenty-step turn is still one long
scroll in which the three diffs that matter sit between forty read results. The filter exists in
the reader's head, not in the window.

- **A.** A segmented filter — All / Tools / Diffs / Errors / Yours — that hides non-matching rows
  while keeping every turn header and the running state; remembered per session. *(recommended)*
- **B.** Search inside the transcript instead. Useful, but a different question.
- **C.** No.

➡️ **A.** It is a render-time predicate over rows already in memory, and it makes the collapsed
tool line worth having.

**Answer:**

---

### Q187 — Is there a keyboard layer beyond Enter?

`/` focuses the shelf search, Enter sends, Shift+Enter adds a line, Escape closes a popover. The
views, the composer, the session list and the terminal have no keys of their own; a person who
lives in the window still reaches for the mouse to change view.

- **A.** A fixed, documented set: `Ctrl+1…6` for the views, `Ctrl+Enter` to send from anywhere in
  the composer, `Ctrl+K` for the palette if it ships (Q147), `?` for a cheat sheet. Not
  rebindable. *(recommended)*
- **B.** A rebindable shortcut system with its own settings page. The config surface costs more
  than the conflicts it resolves.
- **C.** No.

➡️ **A.** Six keys and one overlay; the brief's instrument should be playable from the keyboard,
and a fixed map is the version that stays true.

**Answer:**

---

### Q188 — Can the rails get out of the way?

The shell is four columns — rail, list, detail, inspector — at a 960 px minimum. On a laptop the
inspector is often explanation for a page already read, and the rail is navigation already known;
there is no way to give that width to the transcript.

- **A.** Focus mode: hide the rail and the inspector for the current view, keep the list, toggle
  with `Ctrl+.`, remembered per view. *(recommended)*
- **B.** Auto-collapse under a width. It fights the resize handle and hides controls without being
  asked.
- **C.** No.

➡️ **A.** Two CSS columns and one piece of state; it is the difference between a 960 px window
feeling cramped and feeling deliberate.

**Answer:**

---

### Q189 — Does clicking a notification open the turn that raised it?

`notify.ts` creates a silent `Notification` with a title and a body. Clicking it focuses nothing;
the window may still be hidden in the tray, and the session that finished is wherever it was. An
approval notification is worse: it says something is waiting without taking you to the card.

- **A.** Click focuses the window and opens the session and turn that raised it; an approval
  notification opens its card. The payload carries ids, not paths or prompts. *(recommended)*
- **B.** Click only focuses the window.
- **C.** No.

➡️ **A.** The notification already knows which session raised it; carrying that id one step further
is the whole feature, and it is what makes hiding to the tray safe.

**Answer:**

---

## E. The shelf and skills

### Q190 — Can the shelf pin a skill to the session?

The index is searchable and the shelf can open a body, but there is no action that puts a skill in
front of the next turn. `skill_load` pins on first use, inside a turn; `pin_skill` writes the row;
the window calls neither. "Browse skills…" only changes view.

- **A.** Shelf detail gets "Pin to session": writes the current body hash into `session_skills`,
  shows a chip in the composer with a remove action, and the turn's preamble names the pinned
  skill so the model knows to load it. `unpin` and a list are new core calls. *(recommended)*
- **B.** Leave pinning to the agent; the shelf stays a library.
- **C.** Pin everything opened in the shelf. Too much context, silently.

➡️ **A.** It joins the two halves of the product the brief keeps saying are one, and the pin
machinery already exists. A pin is not permission: the body is still untrusted instruction.

**Answer:**

---

### Q191 — When the index moves under a pinned skill, is there a diff?

`skill_load` compares the pinned hash with the current one, reads the pinned blob when it still
exists, and answers `pin_stale: true`. That flag is in the tool result, not in the window; a person
cannot see what changed or choose to keep the old body.

- **A.** In the session's pinned list, a "changed" badge, a diff between the pinned body and the
  current one, and Keep pin / Update. Updating re-pins with the reason recorded on the session.
  *(recommended, after Q190)*
- **B.** Show the hash and date only, with no diff.
- **C.** Re-pin silently when the index moves.

➡️ **A.** The blob store already keeps both bodies; the honest version of a frozen pin is one that
shows its age. C is the one answer that breaks §8.7.

**Answer:**

---

### Q192 — Can a source be paused or forgotten?

Settings lists each source, its last sync and its error, and offers Refresh and Index a repository.
There is no per-source off switch: a hub that starts returning junk (or costs a token to walk)
stays in the launch refresh until the whole switch is turned off.

- **A.** Two actions per source: Pause (stop refreshing; keep rows and history) and Forget
  (two-step; delete rows and blobs, except a body a session pinned, which is kept and marked
  `missing`). *(recommended)*
- **B.** Forget only. It cannot answer "stop touching this for now".
- **C.** No; the global switch is enough.

➡️ **A.** Pause is the missing middle, and the pinned-body exception is the part that must be
written down before the delete lands.

**Answer:**

---

## F. Models and doors

### Q193 — Does the picker remember favourites and recent picks?

The merged roster is **1,222 rows** on this machine, and the picker has chips, search and a
chat-only filter. It does not remember that you use three of them; finding one again means
searching the same way every session.

- **A.** A star on each row, a small Pinned section above the list, the last five picks below it,
  saved in `settings.toml`; the current model stays visible whatever the filter. *(recommended)*
- **B.** Recents only. It is the cheaper half and drifts as the roster changes.
- **C.** No.

➡️ **A.** It is a settings list and a sort, not a catalogue change; a 1,222-row picker without
recents is a search box pretending to be a menu.

**Answer:**

---

### Q194 — Does the picker say what a model can do, and what it costs?

`ModelOption` carries `free`, `chat` and the vendor's `modality` string, but the row renders only
the id, the name, a free tag and the door. Context length, price and image input are invisible —
which matters more once Q176 can send pixels.

- **A.** Add context length, input/output price per million and modality tags where the vendor
  publishes them; "unknown" where it does not; no derived price. The vision flag is what Q176
  reads before it inlines an image. *(recommended)*
- **B.** Price only.
- **C.** No; the vendor's page is one click away.

➡️ **A** is a catalogue parse plus three fields in the row; the "unknown" rule keeps it honest
across vendors that publish nothing.

**Answer:**

---

### Q195 — Is there a custom OpenAI-compatible endpoint?

The provider table is closed: a dozen rows with compile-time bases, and the router is the local
door. A person running LM Studio, vLLM or a company gateway has no row, and the only way in is to
put that service behind OmniRoute.

- **A.** A "custom" provider: https base, key in the keychain, model ids typed or fetched, loopback
  http allowed, redirects off the base host. *(recommended only if a named endpoint appears)*
- **B.** No. OmniRoute is the byo-endpoint door, and every extra base URL is a key sent to a host
  the app did not choose. *(recommended)*
- **C.** After 1.0, with a probe and a warning.

➡️ **B.** The product's promise is a closed, audited table; a custom door moves the trust boundary
from "these twelve vendors" to "whatever URL is in settings" and adds a compatibility surface the
tests cannot enumerate. If a real endpoint is named, C is the shape to build.

**Answer:**

---

## If you only answer five

**Q176** (images), **Q190** (pin a skill to the session), **Q183** (pause), **Q178** (drafts) and
**Q193** (favourites). The first two change what the window can do; the last three change how it
feels to use it for an afternoon.

Back to the [handoff](handoff.md) · [what works](what-works.md).

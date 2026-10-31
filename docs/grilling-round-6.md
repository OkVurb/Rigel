# Grilling — round 6 (Q136–Q175): what to add next

Asked 27 Oct 2026, against `main` with the features of the last two sessions in it — the adapter
table (Vertex, Anthropic, seven OpenAI-compatible vendors), the merged model roster, the shelf's
launch refresh, the eleven Settings pages and the rebuilt Terminal surface. The state each
question builds on is in [what-works.md](what-works.md); the decisions already settled are D1–D135
in [handoff.md](handoff.md) §3 and §15.

Every question is a **feature that would land code**, and each is scored the round-4 way: what it
buys, what it costs, and whether it belongs before the signed beta (which is still gated on the
certificate and an end-to-end update test), after it, or never. "Don't build" is a real answer for
at least ten of these; a wizard is faster at adding surfaces than at removing them.

---

## A. The turn loop — the part that decides whether a task finishes

### Q136 — Is there a chat-only filter in the model picker?

Yesterday's live check answers this: the merged roster is **1 220 rows**, and a large share of the
router's 627 are image and audio checkpoints (`aihorde/…`, `google/lyria-…`) that cannot answer a
chat request. The picker now has provider chips and search, so nothing is unreachable — but the
first screen of a chat picker should not be a Stable Diffusion checkpoint.

- **A.** Default to chat-likely rows with a "show everything" toggle, using a small documented
  denylist (`aihorde/`, `:image`, `-image`, `lyria`, `tts`, `whisper`, `embedding`, …) plus the
  provider's own `/models` metadata where it publishes any. *(recommended)*
- **B.** Keep every row and lead with the current provider only.
- **C.** Split the picker in two: a `chat` tab and an `everything` tab.

➡️ **A.** It is a filter, not a removal — the toggle keeps every id reachable — and it is the
difference between opening the picker and reading a list of things that will refuse a turn.

**Answer:**

---

### Q137 — When a provider roster answers 4xx for one key, what does the user see?

Today: the provider is reported in the picker's fine print (`groq: Groq answered 401`) and the
other rows still render. There is no per-provider "test this key" affordance, so a wrong key looks
like a slow roster.

- **A.** A "Test key" button on each Providers page row: one `GET /models` (or a 1-token request
  where no roster exists), and the answer recorded with its status code. *(recommended)*
- **B.** Show the roster error only, as today.
- **C.** Block a turn's send button while any provider key fails a check.

➡️ **A.** It is the one click that turns "nothing happens" into "401, that key is wrong", and it
reuses the catalogue read that already exists.

**Answer:**

---

### Q138 — Mid-turn steering: can a person add a note while a turn is running?

Today the composer sends a new turn, and a running turn can be cancelled — there is no way to say
"also, the config file moved" to the turn in flight, which is the most common thing a person wants
to type while watching one.

- **A.** A note goes into the running turn at its next step boundary, shown in the transcript as a
  user note on that turn. *(recommended)*
- **B.** Sending while running cancels and restarts the turn with both prompts.
- **C.** Keep it: reading only until the turn ends.

➡️ **A.** The loop already rebuilds its message list per step, so the note is one insertion rather
than a new protocol — and B throws away paid-for work.

**Answer:**

---

### Q139 — At the step bound, does the loop stop or ask?

`max_steps` (10 by default, now a Settings field) ends a turn with `stopped after N steps`. The
common case is a task that was two steps from done.

- **A.** Stop as today, but with a **Continue** button on the stopped turn that starts a new turn
  with the same task and a fresh bound. *(recommended)*
- **B.** Pause mid-turn and wait for a click, as the plan gate does.
- **C.** Raise the default to 20 and stop silently.

➡️ **A.** The transcript stays honest (a turn ended), and continuing is a decision the user makes
with the whole transcript in front of them.

**Answer:**

---

### Q140 — Long sessions: does Rigel compact context?

A turn rebuilds context from the transcript, capped by `fixed_context_chars`. A long session
therefore re-sends a lot, and the newest instructions compete with the oldest tool output.

- **A.** Compact: when the context exceeds the cap, replace the oldest tool results with the model's
  own one-line summaries of them, marked as summaries in the preamble. *(recommended, after 0.1.0)*
- **B.** Keep the hard cap; long sessions just see less.
- **C.** Raise the cap and let the provider truncate.

➡️ **A**, but not this week: it is the largest quality change on this list and it needs an eval
harness to say whether it helped.

**Answer:**

---

### Q141 — Should a turn record the exact provider traffic for replay?

Diagnostics currently carry the database check, the newest turn and the log — not the request
bodies, so a provider-shaped bug (a field a vendor rejects) cannot be reproduced offline.

- **A.** An opt-in recording: full request/response bodies for one turn, written beside the
  diagnostics bundle, with keys and absolute paths redacted. *(recommended, after 0.1.0)*
- **B.** Never record bodies; rely on the unit tests' scripted servers.
- **C.** Record always, into a ring buffer.

➡️ **A.** The Anthropic 401 is exactly this bug class: the request is verified up to the provider's
error page, and nothing after it can be checked without the body the vendor saw.

**Answer:**

---

## B. Agents working alongside each other

### Q142 — Team presets: are they extensible?

`ROLE_PRESETS` ships four roles (planner, implementer, verifier, researcher). A project that wants
"migration author" and "fixture keeper" has no way to say so.

- **A.** Let a project define presets in a `.rigel/roles.toml`, merged with the built-ins and shown
  in the roster's `+ agent` form. *(recommended)*
- **B.** Keep the four.
- **C.** Load presets from a hub skill, so roles are shareable.

➡️ **A.** It is a table read plus a form field, and it makes the multi-agent spine (D50) usable for
work that is not a web app.

**Answer:**

---

### Q143 — How long do merged subagent worktrees live?

A merged worktree currently stays on disk under the app data folder, which is what makes the Diff
view's "pending diff" and the merge verdict reviewable after the fact — and also what makes the
folder grow forever.

- **A.** Keep merged worktrees for 14 days or 5 merges per agent, then remove them and record the
  removal in the turn. *(recommended)*
- **B.** Remove on merge; the verdict row is the record.
- **C.** Keep everything until the user cleans it by hand.

➡️ **A.** The verdict row is the durable record either way; the worktree is there for the "wait,
what did it change" question, which is a same-week question.

**Answer:**

---

### Q144 — Does a third agent ever resolve a merge conflict?

The conflict strip shows base/ours/theirs and merges line by line, with the human choosing. The
temptation is a resolver agent that proposes the merged file.

- **A.** Don't build it. The conflict UI exists because automatic reconciliation is where silent
  data loss lives (D79). *(recommended)*
- **B.** A resolver agent that proposes a merge into the editable buffer, never writing directly.
- **C.** Auto-resolve whitespace-only conflicts only.

➡️ **A** for the resolver; **C** is acceptable if it is restricted to whitespace and recorded.

**Answer:**

---

### Q145 — Is there a per-child spend cap?

The session cap pools children (D42/D82): a fan-out can overshoot by one step per running child,
which is documented and bounded. A $20/day project cap now sits above it (D116).

- **A.** Add a per-child cap in addition to the pooled one, default `max_steps × step estimate`,
  shown on the child's row. *(recommended, after 0.1.0)*
- **B.** Keep pooling; the project cap is the guard.
- **C.** Refuse fan-out above two children near the cap.

➡️ **A**, once a real fan-out has been run and the overshoot has been measured. Designing it from
the estimate alone risks a cap that blocks the useful case.

**Answer:**

---

### Q146 — Do agent-to-agent messages become transcript turns?

The room is a chat server; the transcript shows turns. A parent asking a child for its result
currently writes a note, and the room's messages are not all in the transcript.

- **A.** Only messages that change work — an assignment, a result, a hand-off — become transcript
  items, marked as room messages; chatter stays in the room. *(recommended)*
- **B.** Every room message lands in the transcript.
- **C.** The transcript shows nothing from the room.

➡️ **A.** B makes the transcript unreadable and re-sends chatter as context on every step; C loses
the reason a child did what it did.

**Answer:**

---

## C. The window

### Q147 — Is there a command palette?

Every surface is reachable by mouse and a few shortcuts exist (`/` focuses search). Power use means
switching surface, project, model and session without reaching for the mouse.

- **A.** `Ctrl/Cmd+K` palette: surfaces, projects, sessions, model switch, "index repository",
  "refresh sources", "export diagnostics", each with the same label the surface uses.
  *(recommended)*
- **B.** More single-letter shortcuts per surface.
- **C.** Nothing; the rail is enough.

➡️ **A.** It is the cheapest way to make eleven settings pages and five surfaces feel like one
application rather than a set of tabs.

**Answer:**

---

### Q148 — Search across sessions and projects?

Session history is in project databases; the rail lists sessions, and each has a transcript. "Which
session did I change the winget manifest in" has no answer today.

- **A.** FTS5 over turn text in the project database, one search box on the session list, results
  as "session · turn · snippet". *(recommended, after 0.1.0)*
- **B.** Search only within the open session.
- **C.** Nothing.

➡️ **A.** The index already exists as a pattern (the shelf), and history that cannot be searched is
history nobody reuses.

**Answer:**

---

### Q149 — A light theme?

Settings carries `theme` and the CSS is dark-only, which is why the field is deliberately absent
from the Settings pages: a control nothing reads teaches the user not to trust the screen.

- **A.** Build a real light theme (every token, both terminal themes), then surface the picker with
  a `system` option. *(recommended, after 0.1.0)*
- **B.** Dark only, and delete the setting.
- **C.** A "high contrast" variant of dark first.

➡️ **A**, but only with the token pass done properly; a half-inverted theme is worse than none. If
it does not get the pass, **B** is the honest fallback.

**Answer:**

---

### Q150 — One window per project?

The window is single-project, with the tray keeping agents alive when it hides. Comparing two
projects means quitting or re-picking the project root.

- **A.** A second window per project (`Ctrl+Shift+N`), sharing the tray and one Quit that reaps
  every child. *(recommended, after 0.1.0)*
- **B.** Tabs inside the one window.
- **C.** Keep one window; switching roots is fast enough.

➡️ **A.** Tabs fight the three-pane grid that the whole interface is built on; a second window
costs one state map and no new layout.

**Answer:**

---

### Q151 — Tray quick actions?

The tray currently reopens the window or quits. With agents running in the background, the useful
verbs are "pause everything", "open the window", "quit when idle".

- **A.** Add Pause/Resume all and "Quit when idle" to the tray menu, each writing a transcript note
  when it acts. *(recommended)*
- **B.** Keep reopen/quit.
- **C.** Add a tray summary line (agents running, spend today).

➡️ **A** now (it is the difference between a tray icon and a control), **C** when spend is wired
into the tray's data.

**Answer:**

---

### Q152 — What does a first run show?

Today: an empty shelf, an empty session, and Settings to add a key. A person who has just installed
a signed build has no project, no key and no idea which of eleven pages matters.

- **A.** A four-step first-run card: open a project, add one provider key, index one source, send
  one turn — each step linking to the surface that does it, disappearing as it is done.
  *(recommended)*
- **B.** A tour with tooltips.
- **C.** Nothing; the README is the onboarding.

➡️ **A.** The steps are the actual dependency order of the product, and the card is a checklist, not
a modal.

**Answer:**

---

### Q153 — Localisation?

The interface is English, the CLI is English, the docs are English.

- **A.** Don't build it before 1.0. A half-translated interface (CLI messages included) is worse than
  an English one, and the agent's own prompt language is a separate, harder problem. *(recommended)*
- **B.** Translate the window only.
- **C.** Full localisation, starting with the largest non-English developer population.

➡️ **A.** Say no once, in writing, and revisit with a translator who will own the whole surface.

**Answer:**

---

## D. The shelf and skills

### Q154 — Does Rigel run a skill's scripts?

A skill is instructions plus, often, a script the model is told to run. Rigel reads the skill,
pins it, and then the model may ask to run its command through the same approval gate as any other
command.

- **A.** Keep that: a skill's scripts are commands, and commands go through the gate with their
  cwd and timeout shown. Add a per-skill "trusted scripts" grant that covers exactly that skill's
  documented commands. *(recommended)*
- **B.** Never run anything a skill names.
- **C.** Run a skill's scripts without a prompt once the skill is pinned.

➡️ **A.** C is the sandbox escape the whole security model exists to prevent; a narrow grant is
both safer and less annoying than re-approving the same three commands.

**Answer:**

---

### Q155 — A local folder as a skill source?

Sources today are GitHub, SkillsMP and SkillHub. A person with 40 skills in `~/skills` or on a
mounted drive has no way to index them.

- **A.** `source = local path`, read with the same walk, ETag-free and marked `local` in the shelf,
  refreshed when the folder's mtime changes. *(recommended)*
- **B.** Git with an SSH key for private repos.
- **C.** Neither; export to a zip and import.

➡️ **A.** It is the private-source answer that needs no credential at all, and D62 (reference, don't
copy) already covers deleting one.

**Answer:**

---

### Q156 — Does the shelf learn which skills get used?

Every turn pins skills; nothing records which pin actually ended up in a tool call or an edit that
stuck.

- **A.** Record locally: per skill, how many turns pinned it and how many of those produced a write
  with that skill's guidance in context, shown as a column. *(recommended)*
- **B.** Nothing; a skill's grade is the only signal.
- **C.** Ship those numbers to the marketplace.

➡️ **A** (with the numbers labelled as correlation, not a grade) and never **C** — the local-only
index is D3.

**Answer:**

---

### Q157 — Can a user publish a skill set?

The shelf consumes marketplaces; it cannot produce one. "Publish my five skills as a set" is a
repository template away.

- **A.** Don't build it before 1.0. A marketplace needs moderation, takedowns and a trust story
  that does not exist yet. *(recommended)*
- **B.** A template repository plus `rigel skills export --set` writing a ready-to-push folder.
- **C.** A hosted publisher with accounts.

➡️ **B** is a decent compromise if people ask; **C** is a different company.

**Answer:**

---

### Q158 — Is a hub ingest scanned before it is stored?

A GitHub ingest runs the six-rule scan and stores evidence. The hub sources store metadata only
(no bodies to scan), so a hub row can be pinned and read with no scan at all.

- **A.** Refuse to pin a hub row with no body. Offer "fetch this skill's body and scan it" as one
  click, which pulls the source file and runs the same scan and evidence rows. *(recommended)*
- **B.** Keep the current split and label the row `unscanned`.
- **C.** Trust the hub's own verdicts.

➡️ **A.** "Unscanned" is honest but useless in a review UI; one click to make it scanned is the
difference between the trust column working and being decorative.

**Answer:**

---

### Q159 — Standing pins: "always use these skills"?

Pins are per session (D59). A project with a house style skill wants it in every session.

- **A.** Project-level defaults in `.rigel/skills.toml`: a list of ids pinned into every new session
  of that project, shown as `from project` in the roster and overridable per session.
  *(recommended)*
- **B.** Keep session-only pins.
- **C.** Pin from the shelf's context menu into the global defaults.

➡️ **A.** D59's guarantee (frozen per session) is untouched; this only changes what a session starts
with, which is exactly what a project file should decide.

**Answer:**

---

## E. Money, safety, policy

### Q160 — The per-task cap (D92)?

D10 caps a session; D116 caps the project per day. A single task can still run to the session cap.

- **A.** A per-turn cap: an estimated ceiling shown before the first step, enforced as a stop with
  the transcript intact. *(recommended)*
- **B.** Keep session + project caps.
- **C.** A cap the model proposes and the user confirms in the plan gate.

➡️ **A.** The pre-turn estimate already exists; turning it into an enforced bound is one comparison
in the loop.

**Answer:**

---

### Q161 — At a cap, does the turn stop mid-step or finish it?

Today a crossed cap stops the turn at the step boundary; a tool call already in flight completes.

- **A.** Keep it, and say so in the transcript: "stopped at the cap — the running command finished,
  nothing new starts". *(recommended)*
- **B.** Kill in-flight commands immediately.
- **C.** Pause and ask.

➡️ **A.** Killing an in-flight write is how a half-applied patch happens; the boundary is the only
safe cut point.

**Answer:**

---

### Q162 — Per-project model allowlist?

A project can today be sent any model the machine holds a key for; a work project might need to
stay on a company-approved model.

- **A.** `.rigel/models.toml`: allowed ids (with prefixes), enforced before a turn starts and shown
  as a lock on the picker. *(recommended, after 0.1.0)*
- **B.** A global allowlist in Settings.
- **C.** Nothing.

➡️ **A.** The check belongs where the model is chosen, and a project file is what makes it
travel with the repository.

**Answer:**

---

### Q163 — One key to stop everything?

Cancelling is per turn and per agent, and "stop all agents" is a note in the plan. In an emergency
the fastest control should be one chord.

- **A.** `Esc Esc` (and a rail button) cancels every running turn in the session, with a transcript
  note per agent and the shells left alone. *(recommended)*
- **B.** Keep per-agent cancel.
- **C.** Kills the terminal shells too.

➡️ **A.** The terminal is the human's own shell; killing it in an agent panic is a surprise, not a
safety feature.

**Answer:**

---

### Q164 — How far does redaction reach?

`StreamRedactor` covers the keychain values and the usual token shapes in streamed text and
diagnostics. It does not know about `.env` files read as tool output or about secrets pasted into a
prompt.

- **A.** Extend the patterns (AWS/JWT/PEM/Slack/GitHub token shapes, `KEY=`-style assignments),
  applied to tool output before it enters the transcript, and keep the local-only guarantee.
  *(recommended)*
- **B.** Keep the current set and document the gap.
- **C.** Redact only in exports, not in the live transcript.

➡️ **A.** The transcript is what gets pasted into issues; that is the leak people actually cause.

**Answer:**

---

## F. Providers and harnesses

### Q165 — A local model on this PC (D69)?

The user has a machine that can host a small model and explicitly accepted the slowness. A local
runner is OpenAI-compatible on `127.0.0.1`, which makes it one provider row rather than an adapter.

- **A.** A `local` row whose base URL is a setting (default `http://127.0.0.1:11434/v1`), listed
  with the others, with a probe that says whether anything answers. *(recommended)*
- **B.** A dedicated adapter per runner (llama.cpp, Ollama, LM Studio).
- **C.** Wait until the routing story is settled.

➡️ **A.** One row, no new protocol, and it is the only provider whose cost is electricity — which
makes it the natural first stop for the read-only helper work.

**Answer:**

---

### Q166 — Which vendor is next after the current table?

Rows exist for OmniRoute, OpenRouter, AI Studio, Vertex, Anthropic, OpenAI, xAI, Groq, DeepSeek,
Mistral, Together and Fireworks. The notable absences are the enterprise clouds.

- **A.** AWS Bedrock (its OpenAI-compatible endpoint and SigV4 are a real adapter, not a row).
  *(recommended, when someone asks)*
- **B.** Azure OpenAI (a row plus a deployment name and an `api-version` query).
- **C.** Neither until 1.0; the long tail is already covered by the gateway row.

➡️ **B** is the cheaper of the two and the more common in companies; **C** is defensible until a
user names one.

**Answer:**

---

### Q167 — Vertex: one project or several?

Vertex's project and location come from environment or a stored setting, so switching projects
means editing configuration.

- **A.** A project/location pair per saved credential, switchable in the Providers page, with the
  active pair shown on the row. *(recommended, after 0.1.0)*
- **B.** Environment only.
- **C.** A project per session in the session record.

➡️ **A.** It is a Settings field group plus a probe, and it is what makes Vertex usable for someone
with a work and a personal project.

**Answer:**

---

### Q168 — Does Rigel install a missing harness?

The Tools page installs missing command-line tools through an approval gate. A missing harness
(Gemini CLI, Cursor, Cline) shows an install hint and no button.

- **A.** Offer the same approval-gated install for harnesses, using the documented package command
  (`npm install -g @google/gemini-cli`), and re-probe the row when it finishes. *(recommended)*
- **B.** Keep the hint only.
- **C.** Bundle the harness binaries in the installer.

➡️ **A.** It is the same machinery as the tools ledger, and C is a licensing and update problem the
project does not want.

**Answer:**

---

### Q169 — Which account verifies the Claude adapter live?

The Anthropic row is the one adapter whose live turn has never succeeded: this machine's stored
`rigel/anthropic` entry is refused with `401 invalid x-api-key`, twice, under both accepted
credential forms. Everything after the credential is verified against a scripted server.

- **A.** A working Anthropic API key is added to this machine's keychain, and one real
  `agent run --provider anthropic --prompt "say pong"` closes the item. *(recommended)*
- **B.** Leave it: the unit tests cover the shape.
- **C.** Keep the row visible but label it "unverified live" in Settings.

➡️ **A.** It is a five-minute test that removes the only "verified-by-script" adapter in the table.

**Answer:**

---

## G. Shipping it

### Q170 — Self-update: what happens when a new build will not start?

The updater downloads a signed installer, verifies its SHA-256 and publisher key, and runs it. It
has never been observed moving 0.1.3 to 0.1.4 — that test is owed — and there is no path back if
the new build crashes on launch.

- **A.** Keep the previous installer beside the new one, and offer "roll back" from the tray when
  the last launch failed. *(recommended)*
- **B.** Download only; never auto-install.
- **C.** Ship as-is and rely on users downloading again.

➡️ **A**, and **run the owed end-to-end test first**: an updater that has never moved a version is
a hypothesis, and the rollback's value depends on what that test finds.

**Answer:**

---

### Q171 — Release channels?

Today: one tag, one release, one updater path. A beta user who wants the next fix early has to wait
for the next release.

- **A.** A `beta` channel the updater can opt into (prerelease flag on the GitHub release), off by
  default, with the channel named in the update card. *(recommended)*
- **B.** Stable only.
- **C.** Nightly builds from CI with a separate manifest.

➡️ **A** when there are beta users to serve; **B** until then — C is a second update system to
maintain for an audience of one.

**Answer:**

---

### Q172 — A documentation site?

The README and `docs/` are good and GitHub renders them; the same content would read better with
search, versioned pages and a sidebar.

- **A.** GitHub Pages from `docs/` — one workflow, same Markdown, no new source of truth. *(recommended, after 0.1.0)*
- **B.** Keep everything in the repository.
- **C.** A static-site generator with its own theme and structure.

➡️ **A.** Two publishing targets for one set of files is fine; a second copy of the docs is not.

**Answer:**

---

### Q173 — Is any upload of diagnostics acceptable?

The diagnostics bundle is local, redacted and copied to the clipboard; sharing it is a deliberate
paste (D-safety, round 3). A "send to the maintainer" button would need a server.

- **A.** Don't build it. Keep the bundle and the paste. *(recommended)*
- **B.** A button that uploads to a GitHub issue draft in the user's own browser (no server of ours).
- **C.** A small hosted collector with an explicit consent dialog.

➡️ **B** is acceptable and cheap; **C** contradicts the zero-infrastructure promise.

**Answer:**

---

### Q174 — What is the pricing and licence story at 1.0?

The workspace claims MIT OR Apache-2.0 and there is no paid tier. The question is not about money
today but about what the release notes are allowed to promise.

- **A.** Free and open source, no paid tier, no account, forever. *(recommended)*
- **B.** Free with an optional paid support tier later.
- **C.** Source-available with a commercial licence.

➡️ **A.** Every feature in this document assumes local data and no account; a paid tier would need
the one thing the product deliberately does not have.

**Answer:**

---

### Q175 — What is the single blocking task before the signed beta?

The certificate is ordered (D86) and the release files exist; the last mile is the one nobody has
walked.

- **A.** The end-to-end update test: install 0.1.3, publish 0.1.4, let the installed build find,
  verify and install it, then confirm the new version runs and the old one can be restored.
  *(recommended)*
- **B.** The room's LAN bind with per-agent auth (D87).
- **C.** Ten hand-onboarded users (D84's plan, which needs the build first).

➡️ **A.** It is the only piece of the product whose failure mode is "every existing install is
stranded", and it is the one thing on this list that cannot be fixed after release.

**Answer:**

---

## If you only answer five

**Q136** (the picker's noise), **Q165** (the local model), **Q170** (rollback), **Q169** (the live
Claude key) and **Q175** (the update test). The first two change what the product feels like
tomorrow; the last three are the difference between a beta that can be repaired and one that
cannot.

Back to the [handoff](handoff.md) · [what works](what-works.md).

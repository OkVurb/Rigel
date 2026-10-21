# The grilling — 60 answers

Recorded 20 Oct 2026, on this machine, in four rounds. Answers are verbatim, typos kept, because
the wording is the evidence. You asked for up to 50; this ran to 60 because rounds three and four
kept turning up collisions worth pressing on. Read the answers for the record, the last three
sections for the decisions.

**Naming note.** The questions were asked while the product was called Orion; it was renamed **Rigel**
immediately afterwards (answer 60). The quoted answers below still say Orion, because quotes are
quotes. Everything in my own prose says Rigel.

The thing to notice first: **round one described a safety scanner. Round two described a
multi-agent workspace.** By round four the safety scanner is gone from the answer set entirely.
That is not a contradiction in your head — it is the strategy moving, and everything downstream
should be read as "what does the workspace need".

---

## Round 1 — positioning (1–10)

**1. In one sentence, what does this do that `npx skills` plus a browser tab do not?**

> Tells me what is actually safe

**2. Who is the person who opens it twice a week without being reminded?**

> Me, on this machine

**3. Which competitor scares you most?**

> Claude Code / Codex shipping a built-in browser

**4. Pick one: is it a marketplace, a package manager, a safety scanner, or a search engine?**

> a agent handler so i can chat with agents and upload files and stuff see sessions hold memory and
> all of the other stuff too, so its like a super power for ai coders and stuff, i want to introduce
> something eventually that will let multiple agents like codex agetsn and claude and others to work
> together in the same file and stuff and work seamlessly and communicate with one another on changes

**5. What would make you kill this project?**

> Nothing — it is a personal tool regardless

**6. GitHub, SkillsMP and SkillHub are all wired in. If one had to go, which do you keep?**

> Keep all three, accept brittleness

**7. Is the core job "find more skills" or "know what I already have"?**

> Both, in that order

**8. When SkillsMP or SkillHub breaks, what happens to its listings?**

> Go stale, with the last-seen date shown

**9. What freshness does a listing need to be trustworthy?**

> A week

**10. Should discovery work offline?**

> Yes, from the local index, clearly marked

---

## Round 2 — the pivot (11–30)

**11. Which sentence is this now?**

> One place where several agents work on my code together

**12. With chat, uploads, memory and multi-agent sessions, what role do skills play?**

> The capability layer the workspace calls into

**13. Two agents on one repo: shared working tree, or isolated copies?**

> Same working tree, live

**14. Both edit the same file in the same minute. What happens?**

> Agents must claim a file before editing

**15. Do agents talk to each other, and through what?**

> Directly, agent to agent

**16. Are you in the loop on every inter-agent message, or only on conflicts?**

> Only conflicts and destructive actions need me

**17. Define a session. What starts it and what ends it?**

> A project-level thread that never really ends

**18. Where does memory live — per project, per agent, or global?**

> Per agent, portable across projects

**19. An uploaded file: project content, or just context?**

> Attached as context only

**20. Which harnesses must work on day one?**

> Claude, Codex, Cursor and Cline

**21. Can an agent spawn another agent?**

> Yes, with a depth and budget cap

**22. What stops one agent reading another's secrets?**

> Each agent gets its own credentials and paths

**23. Where do terminal sessions live?**

> One terminal per agent, labelled

**24. Do you need two agents' diffs side by side?**

> One merged diff with agent attribution

**25. An agent starts looping and spending. What is the kill switch?**

> Budget caps that stop them automatically

**26. Smallest slice of the vision that ships this week?**

> Skills plus one agent chat pane

**27. What do you cut to make the multi-agent workspace real?**

> cut nothing

**28. Build the orchestration, or adopt something existing?**

> Adopt an existing orchestrator and wrap it

**29. Which of these is the lie you are most likely to tell yourself?**

> "Agents will coordinate cleanly"
>
> Agents coordinate cleany and people want another ide

**30. What 60-second demo makes someone say "I need this"?**

> Two agents fix one bug and hand it back resolved
>
> All the above

---

## Round 3 — architecture and safety (31–50)

**31. Who enforces a file claim?**

> Orion brokers claims; agents ask it

**32. An agent claims a file, then hangs. What happens to the claim?**

> Orion asks me to release it

**33. How do two agents share one tree without reading a half-written file?**

> Writes go through Orion, atomically

**34. What is the actual message transport between agents?**

> Harness-native subagents and prompts

**35. Which orchestrator do you adopt first?**

> Codex sessions plus worktrees

**36. Same model for every agent, or model per task?**

> Per task — cheap model to explore, strong to fix

**37. How does the app know if an agent is thinking, stuck, or waiting?**

> Harness event stream

**38. An agent invokes a skill mid-session. Who loads it?**

> Orion runs it as a tool
>
> whatever is the best honestly

**39. Can a skill update mid-session and change behaviour silently?**

> No — sessions pin skill versions

**40. An agent installs a dependency mid-task. What must happen before it works?**

> Restart the agent's shell session

**41. Can an agent install without your approval?**

> scan the dependency and if deemed a safe enough wit henough users and people using it yk then its
> okay to download

**42. Which dependency failure blocks a task?**

> None — warn and continue

**43. When an agent hits a missing tool, who decides?**

> The agent installs within policy

**44. Do you need to type into an agent's terminal yourself, mid-task?**

> Yes, occasionally, and I want it

**45. Laptop sleeps or network drops. What survives?**

> Everything — agents resume where they stopped

**46. What must be persisted for a session to be replayable?**

> Messages, tool calls and diffs

**47. Where does the database live if memory is portable?**

> Per project, committed or synced with the repo

**48. Where do agent credentials live?**

> OS keychain, per agent

**49. Security boundary between your agent and a skill that agent installed?**

> Skills are untrusted; capabilities are gated

**50. One number that says the workspace is working after a month?**

> Tasks completed without me intervening

---

## Round 4 — forcing the collisions (51–60)

**51. You want brokered claims, but harness-native transport. What gives?**

> Agents talk natively; claims are advisory

**52. "Same working tree, live" versus "Codex plus worktrees". Which is it?**

> Depends on whether the agent was spawned

**53. One Codex session spawns three subagents. Can the app see them?**

> Enforced — no agent runs unregistered

**54. "Safe enough, with enough users" — what concretely decides that?**

> Popularity thresholds you can tune

**55. You leave it running an hour. What never happens without you?**

> No network installs, no schema migrations

**56. The session DB travels with the repo. How do machine paths and secrets stay out?**

> Split: memory in repo, index local

**57. What does "resume" mean for a file that was half-written when the lid closed?**

> Prompt me on resume

**58. Week one is skills plus one chat pane. What is week two?**

> Make sessions survive anything

**59. Where does this end up commercially — honestly?**

> Open source, build in public

**60. Orion is a browser, a security product, and an OpenAI codename. Does the name change?**

> Rename before anyone sees it
>
> Figure out an Astronomical name something to do with space idk

---

## The seven collisions

These are not slips. Each pair is a genuine fork, and the product has to pick a side in code.

### 1. Brokered claims vs. native transport — **advisory wins, and that is fine**

Answers 14 → 31 → 51 walk from "claims are mandatory" to "claims are advisory". The sequence is
consistent once you notice it is the same realisation three times: *you cannot veto a tool you do
not control.* A Codex subagent writes with its own file tools; Rigel can observe, log and warn, but
it cannot stand in front of the write.

What that leaves is narrower and honest:

- A claim is an **MCP tool any harness may call** — `claim(path)`, `release(path)`, `who(path)`.
- Rigel **detects violations** by diffing the tree against outstanding claims and attributing
  changes to agents. It reports collisions; it cannot prevent them.
- Atomic routing (answer 33) applies **only to writes Rigel itself performs** — patches it applies
  on an agent's behalf. Direct harness writes bypass it.

The UI must therefore never say "locked". It says **"claimed by X, unverified"** — because that is
what is true. This is the single most important honesty constraint in the design.

### 2. "No agent runs unregistered" is unenforceable for harnesses you do not own

Answer 53 wants enforcement; answers 34 and 51 hand the transport to the harness. Both cannot hold
for third-party subagents. They can hold for agents **Rigel spawns**, because Rigel chooses their
argv, their env and their working directory.

So: enforcement is a property of *spawned* agents, and a best-effort observation of everything else.
Nested agents inside a Codex process are visible only if Codex's event stream reports them. Whether
it does is a fact to verify in the Codex spike, not an assumption to design against.

### 3. "Cut nothing" is not a schedule

Answer 27 refuses to cut scope; answer 26 gives a week-one slice; answer 58 puts durability in week
two. Those are compatible only if "cut nothing" means *defer nothing from the plan* — while the
order still decides which guarantees exist on day one.

Day one, honestly: skills, one chat pane, budget caps, and advisory claims **with no enforcement**.
That is a product with no coordination guarantee. It is fine to ship — but the README must not
promise two agents on one file until week two lands.

### 4. Auto-install by popularity vs. no network installs unattended

Answer 41 says popularity thresholds authorise installs; answer 55 says never unattended. The only
consistent reading is a **mode-dependent policy**:

| Mode | Behaviour |
|---|---|
| You are present | Threshold-clearing installs proceed, logged in a ledger |
| Unattended | Installs queue and wait for you |
| Unknown provenance | Always a prompt |

"Popularity" needs a definition before it is a policy: downloads, stars, dependents, repo age,
whether the artifact is signed. Four numbers and an age floor is a defensible v1; "it seems
popular" is not.

### 5. Portable memory makes every thought a diff

Answer 47 puts memory in the repo and keeps the index local — the right split, and answer 56
confirms it. But it means agent memory is **code-review surface**. A memory file is a committed
artifact; a bad memory write pollutes the repo, a careless one leaks a path or a hostname.

Consequences: memory is plain text, append-mostly, one file per agent, with a schema that has no
free-form field for filesystem paths. Anything machine-specific is written to the local index, and
the two are joined by project-relative IDs.

### 6. "No interventions" is measured against a loop built on interventions

Answer 50 sets the metric as tasks completed without you. Answers 16, 32, 54, 57 spend your
attention on conflicts, stale-claim release, install approval and resume prompts. At v1 every task
will touch at least one of those.

The metric is still the right one — but measured as **interventions per completed task**, trending
down, not as a count of unattended tasks. Otherwise the first honest week reads as failure.

### 7. The scanner is gone, and nobody said so

Round one's answer to "what is this for" was *safety*. Answer 49 keeps safety, but relocates it:
skills are untrusted and their **capabilities** are gated. Grading an index of 2,737 SKILL.md files
is no longer the product's spine — sandboxing a skill the agent actually invokes is.

That is a better product and a different one. The 446-row index stops being the headline and
becomes the shelf the workspace draws from. Worth naming deliberately rather than discovering in
month three.

---

## What this forces

Ordered by what unblocks the rest, not by what is fun.

1. **A claim protocol that admits it is advisory.** MCP tools, a claim ledger, a violation
   detector that diffs the tree per agent. Never render the word "locked".
2. **Budget caps before multi-agent anything.** Cheapest safety in the list, and it is what makes
   answer 55 possible. No caps, no unattended mode.
3. **Skills as tools, not prompts.** Answer 38 settles the ambiguity: a skill becomes a callable
   the agent invokes, which forces a SKILL.md → tool wrapper and makes version pinning (39)
   meaningful.
4. **Codex first, others after.** Worktrees, event streams and spawned-agent control exist there
   natively. Claude, Cursor and Cline are week-three ports with weaker guarantees — say so.
5. **Split storage.** Committed memory, local index, joined by project-relative IDs. Nothing
   machine-specific crosses the boundary.
6. **An install ledger with four numbers.** Downloads, stars, dependents, age — plus a signature
   check where one exists. Interactive auto, unattended queue.
7. **The metric is interventions per task.** Surfaces the number in the status bar so the trend is
   visible from day one.

## What stays unresolved

- Whether Codex's event stream reports **nested** subagents. Spike it before designing the roster.
- Whether "one merged diff with attribution" can survive worktree mode (answer 52 puts spawned
  agents in worktrees, so the merge is across trees, not branches of one tree).
- What a session *is* when it "never really ends" (17) but must be replayable from messages, tool
  calls and diffs (46). Likely: a session is a log, and the UI shows windows onto it.
- The name.

---

## The name: Rigel

Answer 60 rejected Orion — a browser, a security vendor, and an OpenAI codename — and asked for an
astronomical replacement. **Rigel** won: the bright anchor star at the foot of the mark, five letters,
spelled the way it sounds, and effectively unclaimed by developer tools. `rigel add`, `rigel claim`.

The mark stays the chain of stars, unchanged, because Rigel is its brightest point. The rename is
complete: window title, favicon, rail, `package.json`, both READMEs. Storage keys still say `skilldb:`
so the index you already built does not vanish; they move during the SQLite migration.

| Considered | Why not |
|---|---|
| Asterism | Best meaning — a small useful pattern that is not official — but four syllables and a clumsy verb |
| Crux | Crispest CLI, but several product collisions and a slightly ominous ring |
| Alnilam | Keeps the belt literal; obscure and easy to misspell |
| Sidereal | Evokes long sessions; no short form that is not worse |

---

## Handoff prompt for ChatGPT

Paste as-is. It carries the facts, because ChatGPT cannot see this repo.

> You are helping design a local-first desktop app for orchestrating multiple coding agents on one
> repository. It is a personal tool that may become open source. It runs on Windows, macOS and
> Linux; the reference machine is Windows with PowerShell 7, WSL 2, Git and Node.
>
> **What it is:** one window where several agents (Codex, Claude Code, Cursor, Cline) work on the
> same project, with a shelf of reusable "skills" as the capability layer they call into. One
> project-level session that accumulates memory and never cleanly ends. One terminal per agent.
> One merged diff with agent attribution. Chat with any agent, upload files as context.
>
> **Decisions already made — do not relitigate:**
> - Claims on files are **advisory**: an MCP tool agents may call, plus violation detection by
>   diffing the tree. The product never claims to lock a file.
> - Only agents the app **spawns** can be forced to register; third-party subagents are observed
>   best-effort.
> - Spawned agents get **isolated worktrees**; the main session works in the live tree.
> - Writes the app performs are atomic; direct harness writes are not.
> - Skills are invoked as **tools**, and skill versions are pinned per session.
> - Budget caps are mandatory and stop agents automatically; unattended mode bans network installs
>   and schema migrations.
> - Dependencies install automatically only above tunable popularity thresholds, and only while the
>   user is present; the ledger records downloads, stars, dependents, age, signature.
> - Memory is committed per agent and portable; the metadata index is local-only. Nothing
>   machine-specific crosses that boundary. Credentials live in the OS keychain per agent.
> - Sessions must resume after sleep or network loss, and be replayable from messages, tool calls
>   and diffs.
> - Success metric: interventions per completed task, trending down.
>
> **Open questions I want your thinking on, in priority order:**
> 1. A claim protocol that stays useful while being unenforceable — what should `claim`, `release`
>    and `who` return, and what does the violation detector diff against?
> 2. Merging isolated worktrees back into one attributed diff when agents touch the same file.
> 3. How a session that "never ends" is still bounded: retention, replay windows, and what memory
>    entries look like as committed text.
> 4. A trust score for dependencies from four numbers plus a signature, with no false confidence.
> 5. The smallest week-one build: skills shelf plus one chat pane, budget caps, advisory claims.
>    What is the minimum schema?
>
> Answer with concrete interfaces and schemas, not principles. Flag anything above that you think is
> internally inconsistent — the design has already survived one round of contradictions and will
> survive another.

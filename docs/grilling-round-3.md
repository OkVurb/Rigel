# Grilling — round 3 (Q87–Q101)

Asked 22 Oct 2026, against `main` at `17377f9`. Rounds 1–2 (Q1–Q86) are in
[grilling-50.md](grilling-50.md), [interview-answers.md](interview-answers.md) and
[handoff.md](handoff.md) §15; their answers are D1–D69. None of these repeat a settled decision.

**Why now:** the plan is finished. Phases 0–6 are closed, Phase 4's five harnesses all run, and of
Phase 7 only P7.5 (a signed installer) and P7.9 (a clean-machine acceptance) are open — both blocked on
things outside the code. So these questions are not about what to build next. They are about what
"done" means, what the flagship actually requires of a stranger's machine, and which recorded
deviations you accept before anyone downloads this.

Answer out loud, by number. A "keep it as it is" answer is as useful as a change; write that if it is
true. Each question carries a recommendation — disagree with it freely.

If you only answer five: **Q90, Q91, Q94, Q97, Q100.**

---

## A. The release gate

### Q87 — The certificate

P7.5 and P7.9 both end at the same wall: `Get-ChildItem Cert:\CurrentUser\My,Cert:\LocalMachine\My
-CodeSigningCert` is empty, so `sign-installers.ps1` can only ever print `note signed but chain
untrusted`. The signing pipeline itself works (unsigned → exit 1, signed-untrusted → exit 0, no cert →
exit 2).

Options: a cloud code-signing service (e.g. Azure Trusted Signing) if your identity is eligible; an OV
certificate (identity validation, SmartScreen reputation accrues over weeks of downloads); an EV
certificate (hardware token, immediate reputation, real money); or ship unsigned with a published
SHA-256 and a SmartScreen warning the user has to click through.

➡️ Get the cheapest CA-issued route — a cloud signing service or an OV certificate — before the first
public binary, and keep the self-signed flow only in the exit test. An unsigned installer on a public
release page is the strongest "this is malware" signal a small project can send, and every download
while unsigned trains SmartScreen against you specifically.

**Answer:**

---

### Q88 — What 1.0 promises

The workspace is `0.1.0`. Both installers build (MSI 8.0 MiB, NSIS 5.4 MiB), one host install and
uninstall have run, and nothing has been installed on a machine you did not build on.

Options: ship this as a labelled beta (`0.1.0`, rough edges expected, an issue template for breakage),
hold 1.0 until the cert exists and P7.9 passes somewhere clean, or skip the interim and call the first
tag 1.0 because the plan is closed.

➡️ Ship `0.1.0` as a beta the moment the certificate lands, and reserve `1.0` for the day P7.9 passes
on a machine you did not build on. A version number is a promise about support, and you cannot support
1809 because you have never run on it.

**Answer:**

---

### Q89 — The Windows floor

`docs/PLATFORMS.md` commits to Windows 10 1809+ because WebView2 requires it. But the only Windows you
have tested is this host, and the Windows Sandbox run is still queued behind a restart that has not
happened (five days of uptime, `RebootPending` set, Fast Startup).

Options: keep advertising 1809 as supported; narrow the *tested* floor to Windows 10 22H2 / 11 and
label 1809 "should work, untested"; or drop 1809 and require a newer build outright.

➡️ Keep 1809 as the documented technical floor, and make the verified line explicit: "verified on
Windows 11 22H2+; 1809 is the WebView2 minimum and is untested." A support claim you cannot reproduce
is worse than a narrower one.

**Answer:**

---

## B. The room — the flagship is the least shippable part

### Q90 — Python and the gitignored clone

`PLATFORMS.md` promises the installer needs no Node, no Python and no package manager. But the Chat tab
boots `vendor/agentchattr/run.py` through `.venv/Scripts/python.exe` or a `python` on `PATH`, and
refuses with "no python found on PATH to boot the chat server" when neither exists — and
`vendor/agentchattr` is gitignored, so a clean clone has no room at all.

Options: freeze the server into a PyInstaller sidecar shipped inside the installer, with the vendor
clone kept as the developer path; make the room an explicit "advanced: needs Python and the vendor
clone" surface with setup help and nothing more; or rewrite the room's server in Rust.

➡️ Freeze it into a sidecar for the first public build. "Install Python 3, then clone a gitignored
directory into the repo" is not a setup step for anyone who is not you, and the room is the one thing
here no competitor has.

**Answer:**

---

### Q91 — D21 versus the subscriptions people actually have

D21 says API keys only, never a subscription login. That is correct for the native path: Rigel bills
those turns, so it has to hold the key. But the room's value is Claude Code and Codex, and most of
their users are subscription-first — requiring `ANTHROPIC_API_KEY` and `OPENAI_API_KEY` before the room
does anything excludes exactly the audience the feature is for. The roster already distinguishes what
the window can send to (`native`, `claude`).

Options: keep D21 everywhere; allow each harness CLI to use its own login for *external* agents while
Rigel bills nothing for those turns; or support both with a badge so the user knows which path is in
play.

➡️ Allow the harness's own auth for external agents, keep keys-only for the native path, and amend D21
rather than quietly violating it. The boundary that matters is "Rigel never handles the subscription
credential" — not "the user may not have one".

**Answer:**

---

### Q92 — Where a room transcript lives

The conversation is agentchattr's JSONL under `vendor/agentchattr/data`, outside Rigel's project
database. A resumed session therefore cannot show what the room said, and the Diff tab cannot attribute
anything that happened there.

Options: leave it in the vendor's data directory; ingest a read-only copy into the project DB so resume
shows it; or ingest it fully as claim/attribution events alongside native turns.

➡️ Ingest a read-only copy into the project DB, and explicitly *not* into the billing ledger. The
external CLIs bill themselves; inventing costs for them would be the first dishonest number in a
product whose pitch is that the bill is real.

**Answer:**

---

### Q93 — The room's trust boundary

The server is `http://127.0.0.1:8300` and the session token lives in Rigel's process — that is why a
restart leaves agents reconnecting to a dead room. Any process running as the same user can read that
token and post as an agent.

Options: loopback-only, refuse any non-loopback override in code, and say the local-user trust model
out loud in `SECURITY.md`; allow a LAN bind so two machines can share a room (which needs real auth);
or keep loopback but add per-agent tokens so one compromised harness cannot speak for another.

➡️ Loopback-only, refuse the override, and put one sentence in `SECURITY.md`: "the room trusts the local
user; any process running as you can post as an agent." That is honest. LAN rooms are a different
product with a different auth story, and they are not v1.

**Answer:**

---

## C. Debts the documents already admit

### Q94 — Per-chunk redaction

Every streaming delta is redacted as it arrives (`crate::redact` per chunk in `claude.rs`, `codex.rs`,
`acp.rs`), and the handoff records the hole: a credential split across two chunks reaches a transient
event before the completed message replaces it with redacted text.

Options: build a sliding-window redactor that holds back a partial-secret tail (at most the longest
secret's length minus one) and flushes on the next chunk or at end-of-stream; accept it and document
that the live stream is best-effort; or stop streaming redacted text to the UI at all and render only
the completed, redacted message.

➡️ Build the sliding window before the public beta. It is bounded, testable with the existing fixture
harnesses, and it is the one place a key could otherwise appear on screen in a product that promises
the key never leaves the keychain.

**Answer:**

---

### Q95 — The "three-way" conflict that is two hashes

P5.4 records the deviation: a conflict row carries `base_hash` and `observed_hash`, not rendered
base/ours/theirs text, and the resolve action records the choice without producing a merged file.

Options: build a real three-way render and merge; keep the hashes and rename the surface to what it is
("conflict record", with both hashes and an "open both copies" action); or drop the conflict strip and
leave conflicts to `rigel who`.

➡️ Rename it and add the side-by-side open. A half-tested merge engine is worse than an honest pointer,
and the Diff tab already shows the patch that caused the conflict.

**Answer:**

---

### Q96 — Worktrees nobody uses

P5.5 built `WorktreeService` and merge-back (clean merge, moved main tree, new/deleted files, identical
bytes, non-repositories, force-removal all tested), but native subagents are read-only by design, so
nothing in the product's normal flow creates one. It serves external and CLI agents only.

Options: make spawned subagents writable so the feature is on the main path; leave it as the
external-agent feature it is and say so; or remove the CLI surface until a writable spawn path exists.

➡️ Declare it "for external and CLI agents" and stop. Writable subagents change the safety story —
claims, snapshots, budgets, attribution — far more than they change the feature list.

**Answer:**

---

### Q97 — Five harnesses, two real turns

`scripts/p4-exit-test.sh` runs Codex and Gemini live; Cursor is subscription-only, Cline has no key, so
both run against `fake_acp`. Claude's adapter is exercised through `fake_claude`, and nobody has watched
a real Claude turn arrive in the running window. After `rigel agent trust` writes the trust files, the
six MCP tools have still not been re-observed inside Claude's interactive TUI.

Options: buy the narrowest set that closes the loop — an Anthropic key and a Cline key for one week, plus
whatever Cursor needs — and record every turn in `docs/qa/`; ship on the 15 golden fixtures plus a QA doc
that states exactly what is unproven; or cut Cursor and Cline from the matrix until a real turn is
possible.

➡️ Buy the Claude and Cline keys for one week and record the turns. The fixture story is good
engineering, but "five harnesses" is a claim a user checks by running one, and the unproven ones are the
two they are most likely to own.

**Answer:**

---

## D. The product after the build

### Q98 — Learning without telemetry

D37 says no telemetry, the updater's switch defaults off, and nothing checks on a timer. That leaves no
way to learn that `0.1.0` fails to start on a machine with an enterprise WebView2 policy.

Options: an opt-in "Export diagnostics" (doctor JSON + last turn + redacted log, copied to the
clipboard for an issue); a local crash log the user can attach; or genuinely nothing, and rely on people
describing the failure.

➡️ Build the export button and the local crash log, with no sender at all. It keeps D37 intact and turns
"it doesn't work" into a doctor transcript — which is the same artifact your own QA doc is made of.

**Answer:**

---

### Q99 — The updater's cadence

`crates/rigel-core/src/update.rs` verifies an Ed25519 manifest over `version / url / sha256` and then the
artifact's hash. The switch defaults off and only `rigel update enable` changes it.

Options: keep "Check for updates" as a manual menu item through the beta, matching D37; run a signed
check once a day after opt-in; or check at startup so a security fix reaches people who never open
Settings.

➡️ Manual through the beta, revisit at 1.0 when there is a security reason to push a fix. An
auto-checker in a no-telemetry product is a promise about what you send that you would then have to keep
perfectly.

**Answer:**

---

### Q100 — The first ten users, and the bar

Everything is built and almost nothing has been used by anyone but you.

Who are the first ten, where do you find them, and what counts as success in week two — downloads, one
completed task, or someone opening it again without being reminded?

➡️ Claude Code and Codex CLI users who already run two agents in two terminals, recruited one at a time
from places you are already in, with the bar "ran a real task twice in week two". Not a launch post: at
ten users you want conversations, not traffic.

**Answer:**

---

### Q101 — Whether there is a business

The licence is MIT/Apache-2.0, there is no telemetry, no account and no server. Is Rigel a tool you give
away and measure, or is there a paid surface in year one — team rooms across machines, a hosted claim
ledger, support?

➡️ Give it away for year one and measure. The only paid surface that fits this architecture is a shared
room/claims plane across machines, and that is a different security model. Building it before ten people
have used the local one is how the local one stops getting finished.

**Answer:**

---

## Answers (given 22 Oct 2026)

| Q | Answer | Decision |
|---|---|---|
| 87 | Ship unsigned with a published SHA-256 | D71 |
| 88 | `0.1.0` beta once the certificate lands | D72 |
| 89 | 1809 documented, verified 11 22H2+ | D73 |
| 90 | PyInstaller sidecar in the installer | D74 |
| 91 | Both paths, with a roster badge | D75 |
| 92 | Read-only copy of the room transcript into the project DB | D76 |
| 93 | LAN bind with real auth | D77 |
| 94 | Sliding-window redactor | D78 |
| 95 | Real three-way render and merge | D79 |
| 96 | Make spawned subagents writable | D80 |
| 97 | Cut Cursor and Cline from the matrix | D81 |
| 98 | Opt-in diagnostics export plus a local crash log | D82 |
| 99 | Check for updates at startup | D83 |
| 100 | A public launch post | D84 |
| 101 | Free OSS for year one, measure | D85 |

### Collisions these answers create

1. **Q87 vs Q88.** "Ship unsigned" and "beta once the certificate lands" cannot both be true; the
   beta either goes out unsigned or waits. Put to the user as **Q102**.
2. **Q93 vs D70.** D70 says nothing remote ships in v1 and the architecture only has to avoid
   precluding it; a LAN bind is remote access arriving early, and D77 would supersede D70. Put to the
   user as **Q103**.
3. **Q100 vs D65.** D65 defers public visibility to the 1.0 signed line; a public launch post in the
   beta is that visibility arriving early, and it lands while the installer is unsigned (Q87).

Two further scope notes, recorded rather than asked: **Q97** cuts Cursor and Cline from what is
*claimed and advertised*, not from the code — their adapters stay behind the fixture harnesses, and
`scripts/p4-exit-test.sh` keeps the rows but the docs stop calling five harnesses supported. **Q95**
and **Q96** are new implementation work, not renames: a merge engine and a writable spawn path are the
largest items this round produced.

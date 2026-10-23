# Grilling — round 5 (Q124–Q135): signing, limits, launch

Asked 22 Oct 2026. Evidence in [research-round-5.md](research-round-5.md). Round 3 settled *that* the
certificate gates the public launch (D86); this round settles *which* certificate, and the three safety
limits and launch choices the research says matter more than any remaining feature.

---

### Q124 — Which identity can sign?

This is a fact about you, not a preference, and it decides the route: Azure's individual Public Trust
route covers **US and Canada only**; an EU individual needs a CA that validates individuals (SSL.com IV,
Certum, Sectigo OV); a registered company can use Azure anywhere it operates.

- **A.** EU/EEA individual
- **B.** US or Canada individual
- **C.** UK individual
- **D.** A registered company (any region)

➡️ No recommendation — answer the fact. If it is A or C, Azure is out for the individual route and
SSL.com/Certum becomes the cheap option.

**Answer:**

---

### Q125 — Signing budget

SignPath Foundation signs qualifying OSS for free; SSL.com's individual certificate is $129/yr (plus a
$379 token unless cloud); Certum cloud is from €209; Sectigo reseller ~$319; Azure is $9.99/mo if
eligible. EV is $349–625 and **no longer skips SmartScreen**, so it buys nothing here.

➡️ **Apply to SignPath first, fall back to SSL.com/Certum under $150/yr.** The project is MIT/Apache and
public, which is exactly SignPath's audience; if that fails, $129 is less than a day of your time.

**Answer:**

---

### Q126 — Direct download or Microsoft Store?

Store packages are Microsoft-signed and show **no SmartScreen warning**; registration is free for
individuals; certification takes up to three business days. The costs: Tauri's Store route needs an
offline WebView2 installer (+127 MB), and the newer MSIX path (`winapp` CLI) requires Windows 11 to build.

➡️ **Direct signed MSI/EXE plus winget first; revisit Store after 1.0.** The Store is the cleanest answer
to SmartScreen, but it is a second packaging pipeline and a second update path before the first one has
been installed by anyone.

**Answer:**

---

### Q127 — Package managers at the beta

winget is free, needs a stable URL, a SHA-256 and a silent install (all of which exist). A Scoop bucket
is a git repo you control and pairs with a portable zip. Chocolatey reviews take days to weeks.

➡️ **winget plus a Scoop bucket at the first signed release.** They are the channels Windows developers
already use, and both are manifest files rather than new code.

**Answer:**

---

### Q128 — Launch channel

The evidence is consistent: launches produce attention, not users. PostHog manually onboarded ten people
before HN; Flexprice got 500+ upvotes and 50 signups; Show HN threads often publish no signups at all.

➡️ **Ten hand-onboarded users first, then Show HN, then Console.dev and the Rust/Tauri subreddits.**
Product Hunt is an extra, not the plan; paid promotion is what PostHog admits helped, and it is not
repeatable for a solo project.

**Answer:**

---

### Q129 — Concurrency cap

The failure evidence is about coordination load, not model quality: 41–86% task failure across 1 642
multi-agent traces, and a reported 13-worktree run where 8 of 13 failed on Git lock contention and
cleanup removed uncommitted work.

➡️ **Three writable agents at once, any number of read-only.** The cap is cheap, it matches what the
evidence tolerates, and it can be raised later with data rather than lowered after an incident.

**Answer:**

---

### Q130 — Plan gate

D98 adds plan mode for the native agent. The benchmark systems that perform well are staged
(MASAI, HyperAgent, Agentless), and Cognition's argument is about implicit decisions being lost when work
is split without a shared plan.

➡️ **Mandatory plan approval before a writable agent's first edit, skippable per task.** It is the
cheapest defence against the failure mode the evidence actually documents.

**Answer:**

---

### Q131 — Reviewer gate

Cognition reports a clean-context reviewer catching ~2 bugs per PR (~58% severe), on its own PRs. Rigel
already has the merge-review concept in D64; today it is advisory.

➡️ **Blocking by default for merges from writable agents, override with a recorded reason.** An advisory
reviewer is a reviewer that gets ignored; a block with a reason is the same feature with teeth.

**Answer:**

---

### Q132 — Global spend cap

D10 caps a session; D92 will add a per-task cap. The documented runaway case is a loop nobody bounded,
and a per-session cap does not bound a week of sessions.

➡️ **A default global cap of $20/day with a visible alert and a one-click raise.** It costs a query and a
setting, and it is the number a user would ask for after their first surprise bill.

**Answer:**

---

### Q133 — Imported instructions are untrusted input

D91 reads `AGENTS.md`, `CLAUDE.md` and `.cursor/rules`; D93 imports skills from skills.sh. The prompt
injection study reports 41–84% attack success against coding editors, including payloads delivered
through imported rules.

➡️ **Read them, show them, never treat them as authority.** The transcript marks which rule sources are
loaded, skills show their body before first use, and no skill's scripts run without the normal approval
path. D91 stays; its trust treatment is what this settles.

**Answer:**

---

### Q134 — Do you publish measurements?

Rigel can claim something no competitor can: bounded, reviewable, billed. There is no public benchmark
for that, and the ones that exist (SWE-bench) are under attack for test defects.

➡️ **Build `rigel bench` as an internal harness now; publish numbers only after 30 real tasks.** A
measurement you cannot reproduce is worse than no measurement, and the internal harness is also how the
D89–D98 work gets verified.

**Answer:**

---

### Q135 — Support channel

GitHub issues are searchable and indexable; Discussions handles questions; Discord is where answers go to
die and where a solo maintainer loses evenings.

➡️ **Issues plus Discussions, no chat server.** The diagnostics export (D82) makes an issue actionable,
which is the only support workflow that scales to one person.

**Answer:**

---

## Answers (given 22 Oct 2026)

| Q | Answer | Decision |
|---|---|---|
| 124 | US or Canada individual | D108 |
| 125 | $0 — stay unsigned | D109 |
| 126 | Direct signed MSI/EXE + winget first | D110 |
| 127 | winget + Scoop bucket | D111 |
| 128 | Ten hand-onboarded users, then Show HN, then Console.dev + subreddits | D112 |
| 129 | Three writable + unlimited read-only | D113 |
| 130 | Mandatory plan before a writable agent's first edit | D114 |
| 131 | Blocking clean-context reviewer, override with reason | D115 |
| 132 | $20/day global cap with alert | D116 |
| 133 | Read, display, never treat as authority | D117 |
| 134 | `rigel bench` internal, publish after 30 tasks | D118 |
| 135 | GitHub issues/Discussions, Discord optional | D119 |

### One collision, left open

**Q124 + Q126 contradict Q125.** A US/Canada individual is eligible for Azure Artifact Signing at
$9.99/month, and D110 assumes a signed installer for winget — while D109 says $0 and unsigned. The
cheapest coherent answer is Azure at $120/year, or SignPath free if the OSS application is accepted.
Put to the user as **Q136**; no public release step should rely on D109 until it is answered.

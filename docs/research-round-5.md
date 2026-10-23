# Research notes — round 5 (22 Oct 2026)

Three questions the feature round could not answer: what signing actually costs and how it is obtained,
what the published evidence says about multi-agent coding, and how a small Windows dev tool actually
reaches people. Sources are linked; prices are list or reseller prices on the day, not quotes.

---

## 1. Code signing in 2026

### The rules that changed

- **Hardware since June 2023.** Every publicly trusted code-signing key — OV included — must live in a
  compliant module (FIPS 140-2 L2 / CC EAL4+ or a CA cloud HSM). No more PFX on the build machine.
  [CA/Browser Forum](https://cabforum.org/working-groups/code-signing/requirements/)
- **EV no longer skips SmartScreen.** The old "EV gets instant reputation" shortcut is gone; a new
  publisher can warn regardless. EV remains relevant for drivers and some enterprise requirements.
  [Microsoft SmartScreen guidance](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)
- **Reputation is per publisher and per file hash.** Microsoft says no fixed threshold; expect weeks and
  hundreds of clean installs, and an unsigned new version restarts file reputation each time.
- **Certificate lifetime is capped at 460 days** (CSC-31), so this is an annual-or-more purchase and
  every release must be timestamped. [Ballot CSC-31](https://cabforum.org/2025/11/17/ballot-csc-31-maximum-validity-reduction/)
- **Timestamp everything** with RFC 3161 (`signtool /tr … /td SHA256`). A short-lived cloud certificate
  (Azure's are valid 72 hours) is worthless without it. Check with `signtool verify /v /pa` and never
  modify a signed file afterwards.
- **NSIS:** sign the setup exe *and* the generated uninstaller (NSIS ≥ 3.08 supports `!uninstfinalize`).

### The routes

| Route | Price | Who can use it | Notes |
|---|---|---|---|
| [Azure Artifact Signing](https://azure.microsoft.com/en-us/pricing/details/artifact-signing/) (ex Trusted Signing) | **$9.99/mo Basic**, 5 000 signatures | Organisations in US/CA/EU/UK; **individuals only in the US or Canada** | Key in Microsoft HSM, 72-hour certs, identity validation 1–20 business days, needs a paid Azure subscription |
| [SSL.com OV](https://www.ssl.com/products/software-integrity/code-signing/ov/) | **$129/yr**, YubiKey **$379** extra (or cloud/HSM) | Individuals via their **IV** product | 3–5 day validation; cheapest named option |
| [Certum Standard Cloud](https://shop.certum.eu/code-signing.html) | **from €209** (reseller lists ~$139/yr for individuals) | Individuals (ID + address evidence) | 3+ days; cloud signing |
| [Sectigo OV](https://www.sectigo.com/ssl-certificates-tls/code-signing) | reseller **~$319/yr** | **Individuals eligible** (ID/video check) | 1–7 days |
| [DigiCert OV](https://www.rapidsslonline.com/ssl-brands/digicert/code-signing.aspx) | reseller **~$445/yr** | Organisation only (per reseller) | Overkill for this |
| [GlobalSign](https://shop.globalsign.com/en/code-signing) | reseller ~£252/yr | Organisation only | Overkill |
| [SignPath Foundation](https://signpath.org/) | **free** | Qualifying OSS projects | Worth applying: Rigel is MIT/Apache and public |
| Unsigned + SHA-256 | $0 | — | Every install warns; enterprise policy may block outright |

**Consequence for D86/D71:** signing is not the expensive part of this release — it is $130–250 a year,
or $120/year via Azure if you are a US/Canada individual, or free via SignPath if the project qualifies.
The real cost is identity validation time, which should start now rather than after the beta.

**Microsoft Store is a separate answer to SmartScreen:** Store packages are signed by Microsoft and do
not show the download warning. Registration is free for individuals, certification up to 3 business days.
Tauri's MSI/NSIS Store route needs an offline WebView2 installer (+127 MB); Microsoft's newer `winapp`
CLI packages a Tauri app as MSIX but requires Windows 11 to build.
[Tauri Store guide](https://v2.tauri.app/distribute/microsoft-store/) ·
[winapp CLI](https://learn.microsoft.com/en-us/windows/apps/dev-tools/winapp-cli/guides/tauri)

---

## 2. Multi-agent coding: what is measured

**Nobody has published a clean head-to-head of "one agent" vs "several agents" holding model, tools and
budget constant.** The numbers below are the closest evidence, and each has a caveat.

| Source | Number | What it actually shows |
|---|---|---|
| [Anthropic, multi-agent research](https://www.anthropic.com/research/multi-agent-research-system) | +90.2% over a single agent on an internal *research* eval; **~15× tokens** vs chat; parallel queries 90% faster | Parallelism pays for breadth-first research with independent directions — and Anthropic explicitly says coding has fewer separable tasks and more shared context |
| [Cognition, Don't Build Multi-Agents](https://cognition.com/blog/dont-build-multi-agents) | — | Splitting work loses implicit decisions; individually plausible parts combine inconsistently. Recommends single-threaded writes |
| [Cognition follow-up](https://cognition.com/blog/multi-agents-working) | reviewer catches **~2 bugs/PR, ~58% severe** (company-reported) | A clean-context reviewer can work *while writes stay single-threaded* |
| [MAST](https://arxiv.org/html/2503.13657) | 1 642 traces, 7 systems, **41%–86.7% task failure**, 14 failure modes | Multi-agent systems fail a lot, in design, misalignment and verification — not a production-coding rate |
| [METR 2025](https://metr.org/blog/2025-07-10-early-2025-ai-experienced-os-dev-study/) | 16 devs, 246 tasks, **19% slower** with AI (CI +2% to +39%) | Experienced devs on their own repos; not a multi-agent trial |
| [METR 2026](https://metr.org/blog/2026-02-24-uplift-update/) | later estimates inconclusive; time tracking unreliable when devs ran several agents | The multi-agent measurement problem is unsolved |
| [Agentless](https://arxiv.org/html/2407.01489v2) | 32% SWE-bench Lite at **$0.70** | A simple staged pipeline beat many elaborate agents |
| [HyperAgent](https://arxiv.org/html/2409.16299v3) | 26% Lite / 33% Verified at **$1.82** | More roles cost more and did not dominate |
| [SWE-bench review](https://arxiv.org/html/2506.17208v2) | 80 approaches | No architecture consistently wins; most submissions under-report |
| [OpenAI on SWE-bench Verified](https://openai.com/index/why-we-no-longer-evaluate-swe-bench-verified/) | 59.4% of 138 failures had test/description defects | Leaderboard percentages are not reliability |
| [Prompt injection in coding editors](https://arxiv.org/html/2509.22040v2) | **41–84%** attack success across 314 payloads | Repo content and imported rules are an attack surface |

**Work-loss reports.** A Claude Code issue reports 13 parallel worktree agents: 5 committed, 8 failed on
Git lock contention, and cleanup removed uncommitted work
([#55724](https://github.com/anthropics/claude-code/issues/55724)). A second alleges worktree cleanup
destroyed a repository ([#48927](https://github.com/anthropics/claude-code/issues/48927)). Neither is a
postmortem, but both are the failure Rigel's worktree + merge path must not reproduce.

**What this implies for Rigel, in the language of its own decisions:**

1. **Cap concurrent writable agents.** Nothing in the evidence supports unbounded parallel writers; Git
   lock contention and merge load are real at 13. Three writable plus any number of read-only is the
   shape the evidence tolerates.
2. **Keep writes serialised at the merge point.** D64's review gate is the right instinct; the evidence
   says the reviewer must see a clean context, not the author's.
3. **Plan before writing** (D98) is cheaper than any recovery path, and the benchmark systems that do
   well are staged (MASEI, HyperAgent, Agentless) rather than free-for-all.
4. **Budgets must be hard and global**, not a dashboard: the $47k runaway story is research agents, but
   the mechanism — a loop nobody bounded — applies.
5. **Imported instructions are untrusted input.** D91 reads `AGENTS.md`/`CLAUDE.md` and skills from
   repos; the injection study is exactly about that. Reading is fine; treating it as authority is not.

---

## 3. Distribution and launch

### Channels

| Channel | Cost | Review | Fit |
|---|---|---|---|
| [winget](https://learn.microsoft.com/en-us/windows/package-manager/package/repository) | $0 | automated + moderator, no SLA | **Yes** — the default for Windows dev tools; needs a stable URL, SHA-256, silent install |
| [Scoop bucket](https://github.com/ScoopInstaller/Scoop/wiki/Buckets) | $0 | none for your own bucket | **Yes** — pairs with a portable zip |
| [Chocolatey](https://docs.chocolatey.org/en-us/community-repository/moderation/) | $0 | days to weeks | Optional; enterprise audience only |
| [Microsoft Store](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-certification-process) | $0 registration | ≤3 business days | Later; Microsoft-signed, no SmartScreen warning; MSIX via winapp CLI needs Win11 |

### Launch evidence

- **PostHog**: manual onboarding of the first ten, self-serve one-click deploy, then HN → 300
  deployments in days, 1 000 users in three months; later admitted paid promotion helped GitHub
  Trending. ([first 1 000](https://posthog.com/founders/first-1000-users))
- **Flexprice on Product Hunt**: #1 of the day, 500+ upvotes, **50+ signups**. Attention ≠ users.
- **Frigade Show HN**: 69 points, 31 comments, no published signups.
- **Console.dev**: 2–3 tools a week, submission-based, 68% of readers report signing up for a featured
  tool (publisher-reported).

The pattern is consistent: a launch produces attention, and attention converts only when the thing is
installable without hand-holding. Manual onboarding of the first ten is what every credible account
describes.

### Pricing comparables (2026)

| Product | Free | Paid |
|---|---|---|
| Cursor | limited | Plus $8, Premium $30, Ultra $200 |
| Windsurf | 25 credits | Pro $15, Teams $30 |
| Zed | editor + BYO keys | Pro $10, Business $30/seat |
| Warp | core | Build $20, Business $50/seat |
| Conductor | local Mac | Pro $50, Teams $60/seat |
| crystl.dev | worktrees, approvals | Guild $18/mo |
| Superset | local | Pro $20, Team $60, Ultra $200 |
| Raycast | core | Pro $10 |

The market's paid tier is almost always **remote/cloud + team controls**, not the local editor. That is
consistent with D85 (no paid surface in year one): the only thing worth charging for later is the
cross-machine plane, which is exactly what D87 starts.

---

## What this changes

- **Start identity validation now** (D86 already orders the certificate in parallel). It takes 1–20 days
  at Azure, 1–7 at a CA, and the beta's public gate is the certificate.
- **Apply to SignPath Foundation.** Free signing for qualifying OSS is the only route that costs nothing
  and it fits the licence already chosen.
- **Add a global daily cap and a concurrency cap** to the safety story before the beta; both are cheap
  and both are what the failure evidence is about.
- **Do not present multi-agent as faster.** The honest claim is "bounded, reviewable and billed", which
  the product can actually prove.

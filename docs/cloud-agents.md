#
rigel cloud runs lists what is waiting, and rigel cloud show <id> prints one run in full. Both
read the same documents back through the same rules, so a run that exists and cannot be listed
is a rules or path bug, not a display bug.
 Cloud agents, paid for by someone else


rigel cloud cancel <id> takes an unstarted run out of the queue, and rigel cloud retry <id> puts a
failed or cancelled one back with its prompt intact. Neither command can write leased, running,
done or failed: a client that could claim those states could bill for work nobody did, so the
transition table lives in one tested function and refuses anything a runner owns.
The idea: a user without a provider key can send one turn to Rigel's runner, watch it stream back
into the same window, and pay nothing — a sponsor covers the tokens. This is the Freebuff shape,
applied to agent runs rather than chat.

This document is the design and the arithmetic. **No runner, ad slot or credit ledger exists yet**;
what exists is below, plus `scripts/cloud-economics.mjs`, which is what any of it has to clear.

## The arithmetic comes first

`node scripts/cloud-economics.mjs` (defaults: $4 CPM, 60% fill, $0.18/run):

```
one impression earns $0.0024
one run needs 75.0 impressions
1,000 impressions fund 13.3 runs
at 2 slots per session, 1,000 impressions fund 26.7 user sessions

break-even impressions per run
run cost | CPM $1 | CPM $4 | CPM $12
$0.05    |     83|     20|      6
$0.18    |    300|     75|     25
$0.60    |   1000|    250|     83
```

Two readings, and they point in opposite directions:

- A **cheap** run (routing, caching, short context, ~$0.02–0.05) can be paid for by a handful of
  impressions. Display advertising is viable there.
- A **real coding turn** ($0.18–0.60 today) needs 75–250 impressions, i.e. several minutes of
  someone's attention per run. Banner ads will never cover that.

So the model is not "show ads, run agents". It is "sell sponsorship of runs, and keep the run cheap
enough that a sponsor's money goes far".

## Three funding models, in order of realism

| Model | What the user sees | What the sponsor buys | Why it works |
|---|---|---|---|
| **Sponsored runs** | a one-line "this run is sponsored by X" above the transcript | N runs/month at a fixed price | one line item a human approves; no targeting, no tracking, no CPM |
| **Contextual commerce** | "the tool this skill wraps has a free tier" beside a catalogue row | affiliate revenue per signup | tied to intent, measurable, survives low CPMs |
| **Paid tier** | no sponsor line | subscription | the only model that always works; keeps the free tier honest |

At $2,000/month a sponsor buys **11,111 runs** (~370/day) at $0.18. That is a real number to take to
a tool company, and it needs no ad network, no cookie and no bid stream.

## Architecture

Reuses what is already in this repo; nothing here requires a new vendor.

1. **Identity** — the existing Firebase project and `@freebuff`-style auth already in `functions/`.
   Guests get a device token; the desktop is the only client that can request a run.
2. **Queue** — one Firestore document per run (`runs/{id}`) with a lease, the same transactional
   pattern as `claims`: `queued → leased → running → done|failed`, lease expiry requeues.
3. **Runner** — one Cloud Run job container per run. The image bundles the harness CLIs; provider
   credentials live in Secret Manager and never reach the desktop. A per-run token ceiling is
   enforced both in the runner and by a Firestore budget counter checked before each lease.
4. **Workspace** — an ephemeral volume built from a tarball the desktop uploads (or a read-only
   clone), destroyed with the container. Egress allow-listed to the provider and GitHub.
5. **Credits** — `credits/{uid}` in minutes. Debited on completion from measured tokens, refunded
   on failure. Free minutes come from a sponsor's bucket, not from an ad impression.
6. **Sponsor inventory** — `sponsors/{id}` holds a name, a logo, a remaining-run count and dates.
   A run is tagged with a sponsor at creation. Reporting is aggregate counts only.

## The constraints that make this Rigel and not an ad company

- **No ad SDK, no third-party tags, no ad cookies.** A sponsor impression is a server-side count on
  run start. That is why CPM in this document is a *revenue* estimate, not a tracking plan.
- **Prompts are never used for targeting.** The sponsor sees "N runs", never content.
- **The desktop stays offline-first.** Cloud runs are opt-in per turn, and the local shelf, agents
  and transcripts keep working with the network off.
- **The spend kill switch is not optional.** A global daily ceiling in Firestore is checked before
  every lease; exceeding it fails closed with a visible message, not a surprise bill.

## What is built, and what is not

Built:
- this design
- `scripts/cloud-economics.mjs` — the break-even calculator, runnable now
- the identity and attestation pieces in `functions/` (the `contact` function is live; `appCheck`
  is written but undeployed and its secret is empty)

Not built, in dependency order:
1. Firestore schema and rules for `runs`, `credits`, `sponsors`.
2. `rigel cloud run "<prompt>"` that enqueues and streams status — testable end to end against
   `queued`, before any runner exists.
3. The Cloud Run runner image and its sandbox.
4. Sponsor inventory and the sponsor line in the transcript.

Step 2 is the honest first slice: it is observable, it fails loudly when there is no runner, and it
does not require anyone to approve a bill before it can be tested.

## The queue document

A run is one Firestore document at

```
users/{uid}/runs/{id}
  prompt:       string      the turn, stored once and never used for targeting
  state:        string      queued | leased | running | done | failed
  maxMinutes:   integer     ceiling the runner enforces, recorded at enqueue time
  createdAtMs:  integer     when the desktop queued it
  sponsor:      null        set only when a sponsor's bucket pays for the run
```

No rules change was needed: firestore.rules already allows an account to read and write its
own subtree and lets a token carrying the admin claim read any subtree, which is exactly the
shape a runner needs to lease work without impersonating anyone.

rigel cloud run "..." --minutes 15 writes this document and prints its id. With no runner
attached the state stays queued, which is the honest result and the useful test: the write
path, the rules and the auth are exercised before any bill can exist.

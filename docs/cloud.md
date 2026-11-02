# Rigel Cloud — your own Firebase project

Rigel is offline by default. Rigel Cloud is the one opt-in path that makes it online, and it runs
against **a Firebase project you own**, not a server this repository operates. There is no Rigel
account, no telemetry and no anonymous identifier: the project id and web API key point at your
project, and every Firestore request carries your own Firebase ID token.

What it gives you:

- **Accounts** — email/password sign-in, account creation, password reset and email verification
  through Firebase Authentication.
- **A synced shelf** — push the local skill catalogue's metadata to Firestore; pull it on another
  machine. A row deleted locally becomes a **tombstone**, so the next pull removes it everywhere.
  Both directions stamp a last-sync time. Bodies stay local.
- **Published sessions** — one explicit action publishes a session's prompts and the assistant's
  own words, and the iPhone client reads them from the same project.
- **App Check (optional)** — when the project enforces it, Rigel sends a short-lived App Check
  token from an attestation endpoint you run. A desktop build has no built-in Firebase
  attestation; the endpoint is where you decide which clients are yours.
- **An API another client can read** — the document shape below is the contract, and
  [`firestore.rules`](../firestore.rules) is the access control. The iPhone client or a script can
  read the same collection with the same rules.

What it deliberately does **not** do: no skill bodies, project files, screenshots, tool calls,
command output or file changes leave the machine, there is no realtime listener, and nothing is
sent unless you press the button. A session is only published when you press **Publish to phone**
(or run `rigel cloud publish`); the settings page never uploads a transcript on its own.

## Setup, once

1. **Create the project.** In the [Firebase console](https://console.firebase.google.com) create a
   project (or pick one you already own). The project id is the lowercase id on Project settings,
   not the display name.

2. **Enable Email/Password sign-in.** Authentication → Sign-in method → Email/Password → Enable.
   Accounts can be created from Settings → Cloud → **Create account** (or
   `rigel cloud sign-up`), or added by hand under Authentication → Users → Add user. Enabling the
   provider also means anyone holding the project's public web API key can create an account; if
   the project is open to the internet, restrict sign-up with App Check or a blocking function, or
   keep the project private and add users by hand.

3. **Create Firestore.** Build → Firestore Database → Create database. Production mode is the right
   start.

4. **Deploy the rules.**

   ```bash
   npx firebase-tools login
   npx firebase-tools use --add            # pick the project; writes a local .firebaserc
   npx firebase-tools deploy --only firestore:rules
   ```

   `.firebaserc` and the CLI's debug log are gitignored: the project id is yours, not this
   repository's.

5. **Give Rigel the project id and web API key.** Project settings → General → Your apps → Web app
   → config, or Project settings → General → Web API key. In the window: **Settings → Cloud**. In
   the terminal:

   ```bash
   cargo run -q -p rigel-cli -- cloud configure --project <project-id>
   # reads the web API key from stdin, so it is not in the process list
   ```

   The web API key is public by design — it identifies the project, it does not authorise a read.
   The Firestore rules do that. It is saved in `cloud.json` beside `index.db`;
   `RIGEL_FIREBASE_PROJECT_ID` and `RIGEL_FIREBASE_API_KEY` are the headless alternative.

6. **Sign in and mirror.**

   ```bash
   # First run: create the account, or add it under Authentication → Users.
   printf '%s' "$RIGEL_CLOUD_PASSWORD" | cargo run -q -p rigel-cli -- cloud sign-up --email you@example.com
   # Afterwards: sign in, or ask for a reset link if the password is gone.
   printf '%s' "$RIGEL_CLOUD_PASSWORD" | cargo run -q -p rigel-cli -- cloud login --email you@example.com
   cargo run -q -p rigel-cli -- cloud reset --email you@example.com
   cargo run -q -p rigel-cli -- cloud verify              # email a verification link
   cargo run -q -p rigel-cli -- cloud verify --check      # after opening it
   cargo run -q -p rigel-cli -- cloud push
   cargo run -q -p rigel-cli -- cloud pull
   cargo run -q -p rigel-cli -- cloud publish <session-id>
   cargo run -q -p rigel-cli -- cloud status --json
   ```

   The window does the same from Settings → Cloud: **Create account**, **Sign in**, **Send a reset
   link**, **Send verification email** (then **Resend**), **I've verified — check again**,
   **Push shelf** and **Pull shelf**. A new account is signed in immediately. A reset link is
   emailed by Firebase, and the answer is deliberately identical for an unknown address —
   Firebase's `EMAIL_NOT_FOUND` is hidden, so the page cannot be used to probe for accounts.

7. **Optionally, turn on App Check.** It is off until you give Rigel an attestation endpoint; see
   [App Check](#app-check). Configure and test it on every client **before** you press Enforce in
   the Firebase console, or the un-attested clients stop working.

## Verifying a live project

`scripts/cloud-verify.mjs` runs the checks above against your project and fails on the first
answer that disagrees with the contract. It is the same sequence as a hand verification, with the
assertions written down, so a regression names the step that broke instead of looking like a new
outage.

Prerequisites, once: Email/Password enabled, a Firestore database created, and `firestore.rules`
deployed — setup steps 1–4. Then, from the repository root:

```bash
export RIGEL_FIREBASE_PROJECT_ID=<project-id>
read -r -s -p 'Web API key: ' RIGEL_FIREBASE_API_KEY; echo; export RIGEL_FIREBASE_API_KEY
read -r -s -p 'Test account password: ' RIGEL_CLOUD_PASSWORD; echo; export RIGEL_CLOUD_PASSWORD
export RIGEL_CLOUD_EMAIL='you+rigeltest@example.com'

node scripts/cloud-verify.mjs --wait-verified 180
```

| Step | What it proves |
|---|---|
| `preflight` | The four inputs are present and the CLI is rebuilt from this tree |
| `configure` | Two throwaway data directories take the project and start signed out |
| `account` | Sign-up creates the account (or the existing account signs in) and the uid matches `cloud status` |
| `duplicate` | A second sign-up for the same address is refused and the session survives |
| `email verification` | A link is sent, and `--check` reads the answer back once it is opened |
| `password reset` | A known and an unknown address get the identical sentence; with `RIGEL_CLOUD_NEW_PASSWORD` set, the old password stops working and the new one signs in |
| `firestore` | The `(default)` database exists, read through the Firebase CLI |
| `clean cloud` | Optional `--clean-cloud` empties `users/<uid>` before seeding |
| `seed shelf` | `skills index` puts real rows with bodies in directory A |
| `push` | Every row is mirrored, with no tombstone and no truncation |
| `pull` | Directory B receives the same rows, **with no bodies**, on the shared keychain session |
| `tombstone` | Removing a row locally and pushing deletes it in B, at the same document path |
| `sync clocks` | Each directory's `last_sync_at` follows its own last push or pull |
| `cleanup` | Signs out and deletes `users/<uid>` unless `--keep` is passed; also runs after a failed step |

The run is destructive by design: it uses `tmp/cloud-verify/` for both directories, deletes the
account's Firestore subtree when it finishes, and sends up to two real reset emails. Point it at a
disposable account, not the one you use daily. The Firebase Authentication user itself has no CLI
delete, so the summary names the console page that removes it.

The Firebase session is one OS keychain entry shared by every data directory — that sharing is
what lets directory B reuse directory A's sign-in — so the run **signs this machine out of Rigel
Cloud** when it starts and again at cleanup. Sign in again afterwards, or pass `--keep` to leave
the test session in place. Set `RIGEL_BIN` to test a prebuilt CLI instead of rebuilding from the
current tree.

`--keep` leaves both directories, the Firestore documents and the session in place, which is what
you want when inspecting the documents in the console; `--skip-mail` drops both mail steps;
`--json` prints one machine-readable summary and is what a wrapper script should parse. Exit `0`
means every step passed, `1` means a step failed, `2` means the script could not run at all.

What it cannot check, because a script cannot click: the emailed links (unopened, the
verification step is reported as skipped), the two-account rules check in the Rules Playground,
and the window's own Settings → Cloud page. Those stay in the runbook above.

To exercise the script without a project at all, start the Auth and Firestore emulators and point
`RIGEL_FIREBASE_IDENTITY_BASE`, `RIGEL_FIREBASE_TOKEN_BASE` and `RIGEL_FIREBASE_FIRESTORE_BASE` at
their ports; the CLI reads those overrides from the environment, and a `demo-` project id keeps the
whole run offline. The Auth emulator needs an `auth` block in `firebase.json` — or a `--config` of
its own — before it will start.

## Where things live

| Thing | Location | Secret? |
|---|---|---|
| Project id and web API key | `<data dir>/cloud.json` (`%APPDATA%\Rigel` on Windows) | No; the key is public by design |
| Refresh token, account and verification state | OS keychain, entry `rigel/firebase-session` | Yes — never printed, never sent to the renderer |
| ID token | Memory only; refreshed within 30 s of expiry | Yes |
| App Check attestation URL | `<data dir>/cloud.json` | No; the endpoint is not a secret |
| App Check client token | OS keychain (`rigel/firebase-app-check-client`); the phone's keychain | Yes — never printed, serialized or sent to the renderer |
| App Check rate-limit buckets | Firestore, `rigelAppCheckLimits`; hashed label/address and counts | No; no credential or raw address is stored |
| App Check tokens | Memory only, one per hour on average; refreshed 60 s before expiry | Yes — never printed or serialized |
| Shelf metadata | Firestore, `users/{uid}/skills` | Protected by `firestore.rules` |
| Shelf manifest and sync clock | `<data dir>/cloud-state.json` | No; holds ids, not bodies or tokens |
| Published sessions | Firestore, `users/{uid}/sessions` | Protected by `firestore.rules`; prompts and replies only |

Signing out deletes the local refresh token. It does **not** revoke the token server-side — use
Authentication → Users → Revoke refresh tokens in the console if a machine is lost.

### Email verification

Firebase marks an address verified when its owner opens the link; Rigel stores that answer in the
keychain session and shows it on the account page. It is an account property here, not an access
gate — see the last point below.

- **Send** calls `accounts:sendOobCode` with `requestType: VERIFY_EMAIL` and the account's ID
  token. Firebase mails its own template; the page only learns the address the link went to.
- **Resend** is the same call, and the button becomes a resend once this sitting has sent one.
  Firebase mints a new link each time; the template decides how long a link stays valid.
- **Check** calls `accounts:lookup` and keeps the answer in the keychain session. It does not read
  the stored ID token's claim: that claim stays `false` until a new token is minted, so a link
  clicked a minute ago would still look unverified. An unchanged answer writes nothing, which is
  why a check is one request and no keychain write.
- A session written before this check existed reads as unverified until the first check;
  `rigel cloud status` reports `email_verified` either way.
- **Verification is not access control here.** `firestore.rules` checks the uid, not the address,
  so an unverified account can still push, pull and publish. A deployment that wants verified-only
  access can add `request.auth.token.email_verified == true` to those rules — and must force a
  token refresh after verifying, because the old ID token keeps the old claim until it expires.

### App Check

App Check makes Firebase verify, on every request, that the caller is a client your project
vouched for. The web API key is public by design, so without App Check anyone who finds it can
create accounts in your project and spend its quota. With enforcement on, an un-attested request
is refused before it reaches Firestore or Authentication.

A desktop binary cannot attest itself the way an App Store app can: there is no Play Integrity
equivalent, and any secret shipped in the build is extractable. Rigel therefore uses App Check's
**custom provider**. The repository ships that provider in [`functions/index.js`](../functions/index.js): a Cloud
Functions v2 endpoint that verifies a per-install credential, rate-limits it in Firestore, and
mints a token with the Firebase Admin SDK. Rigel fetches and caches that token and attaches
`X-Firebase-AppCheck` to every Firebase request it sends.

**The endpoint's check is the entire security boundary.** It does not accept the project id, the
web API key, or anything else that ships in the binary. Each installation gets a random credential
that you hand it out of band; the endpoint stores only its SHA-256, and the client stores the
credential in the OS keychain. An endpoint that mints a token for anyone adds no protection.

#### 1. Deploy the endpoint

The function needs the **Blaze** plan, the Firestore database from setup step 3, and a web app id
from Project settings → Your apps. From the repository root:

```bash
cd functions
npm install
cp .env.example .env
# Set RIGEL_FIREBASE_APP_ID in .env, e.g. 1:1234567890:web:abcdef…
```

Create one credential per installation. The token goes to the client; only its hash goes into the
secret:

```bash
TOKEN=$(openssl rand -hex 32)
printf '%s' "$TOKEN" | sha256sum      # the 64 hex characters, not the token
firebase functions:secrets:set RIGEL_ATTEST_CLIENTS
# paste one line per installation:  desktop-1:<sha256>
firebase deploy --only functions
```

The deploy runs the endpoint's tests first, then prints the function's URL. In the default region
it is `https://us-central1-<project>.cloudfunctions.net/appCheck`; change the region in
`functions/index.js` if you deploy it elsewhere. It answers:

| Status | Meaning |
|---|---|
| `200` | `{"token": "…", "expires_at": <unix seconds>}` |
| `401` | no credential, or one not in `RIGEL_ATTEST_CLIENTS` |
| `429` | this installation or address spent its bucket; `Retry-After` says when |
| `503` | `RIGEL_FIREBASE_APP_ID` or the client secret is missing or malformed |

The credential is compared in constant time before Firestore is touched. An accepted request then
spends two token buckets in one transaction: 12 requests/hour with a burst of 4 per installation,
and 60/hour with a burst of 12 per source address. Both are generous for a client that fetches one
token an hour; tune them in `functions/.env` (`RIGEL_ATTEST_CLIENT_PER_HOUR`,
`RIGEL_ATTEST_CLIENT_BURST`, `RIGEL_ATTEST_IP_PER_HOUR`, `RIGEL_ATTEST_IP_BURST`). Bucket
documents live in `rigelAppCheckLimits` and hold a hash of the label or address, never the
credential or the raw address. `firestore.rules` denies clients access; the Admin SDK bypasses
the rules. A TTL policy on `expires_at` clears stale buckets:

```bash
gcloud firestore fields ttls update expires_at \
  --collection-group=rigelAppCheckLimits --enable-ttl --project <project>
```

The function's service account needs permission to mint App Check tokens. The Compute Engine
default service account usually has broad Editor access; a locked-down project should grant
`roles/firebaseappcheck.admin` explicitly. A `PERMISSION_DENIED` in the function log is that
missing role, not a client problem.

Rotation is one line: add `desktop-2:<hash>`, deploy the secret, switch that installation, then
remove the old line. A token already minted stays valid until its hour is up.

#### 2. Point the clients at it

```bash
rigel cloud app-check --url https://REGION-PROJECT.cloudfunctions.net/appCheck
printf '%s' "$TOKEN" | rigel cloud app-check --client-token
rigel cloud app-check          # URL, and whether a client token is stored
rigel cloud app-check --clear  # stop sending tokens; the client token stays
rigel cloud app-check --clear-client-token
```

Settings → Cloud → **App Check** has both fields and a **Forget token** button. The URL must be
`https`; `http` is accepted only for a loopback test endpoint, because the client token is a
bearer secret. The URL is saved in `cloud.json`; `RIGEL_FIREBASE_APP_CHECK_URL` is the headless
alternative. The client token is a
bearer secret and goes to the OS keychain under `rigel/firebase-app-check-client` — never
`cloud.json`, never the status payload, never the renderer. `cloud status` reports `app_check` as
`{"attest_url": …, "client_token_present": true|false}` and nothing more. The headless
alternative is `RIGEL_FIREBASE_APP_CHECK_CLIENT`.

On the iPhone, the Cloud screen has the same optional URL and **App Check client token** fields.
The URL is stored in the app's preferences; the token goes to the phone's keychain. Both clients
must carry a credential before enforcement is turned on.

#### 3. Enforce, Firestore first

Firebase console → App Check → APIs. Enforce **Cloud Firestore**, confirm push, pull, publish,
resend-verification and sign-in still work from the desktop and the phone, and only then enforce
**Authentication** (labelled preview). Authentication enforcement applies to every client in the
project, so an old build or a script without a token stops signing in as soon as you press it.

Order matters: configuring enforcement before the clients have the URL and token locks out Rigel's
own writes. If that happens, turn enforcement off, fix the token flow, and try again. With App
Check configured but unreachable, Rigel fails closed and names the endpoint in the error rather
than sending an un-attested request that Firebase would reject anyway. A `401` from the endpoint
is reported as “refused this client”, and names the missing or rejected client token.

## The Firestore document shape

Every document is rooted at the signed-in account. No shared collection exists.

```
users/{uid}                         the account: email, app, version, platform, updated_at, schema
users/{uid}/skills/{document_id}    one row per skill
users/{uid}/sessions/{session_id}   one published session header
users/{uid}/sessions/{session_id}/turns/{turn_id}   one published turn
```

`document_id` is the lowercase hex SHA-256 of the skill id. Skill ids contain `/`
(`github:acme/skills:skills/pdf/SKILL.md`), which a Firestore document id cannot; a hash is stable
across machines and path-safe. The real id is in the `id` field.

| Field | Firestore type | Meaning |
|---|---|---|
| `id` | string | The shelf id, e.g. `github:owner/repo:path/SKILL.md` |
| `name` | string | Skill name |
| `description` | string | One-line description |
| `source` | string | `github`, `skillsmp`, `skillhub`, `local`, … |
| `repo`, `path`, `url` | string, optional | Where it came from |
| `grade`, `grade_scope` | string, optional | The static review's result and what it covered |
| `license` | string, optional | As the source declared it |
| `stars`, `installs`, `tokens` | integer, optional | The advertised numbers, unchanged |
| `capabilities` | string, optional | The static review inventory as a JSON string |
| `simhash` | string, optional | 16 hex digits, for variant clusters |
| `updated_at` | timestamp | When this machine last pushed the row |
| `deleted`, `deleted_at` | boolean / timestamp, optional | Present only on a tombstone; see below |
| `app`, `schema` | string / integer | `Rigel`, and `1` for this shape |

A row **never** carries a body or a `body_sha`. That is deliberate: the blob store is local, and a
pull must not claim a body this machine does not have.

### Deletion tombstones

A deleted row is **rewritten at the same document path**, not removed from Firestore:

```json
{
  "fields": {
    "id":        { "stringValue": "github:acme/skills:skills/pdf/SKILL.md" },
    "deleted":   { "booleanValue": true },
    "deleted_at": { "timestampValue": "2026-10-30T19:42:00Z" },
    "updated_at": { "timestampValue": "2026-10-30T19:42:00Z" },
    "app":       { "stringValue": "Rigel" },
    "schema":    { "integerValue": "1" }
  }
}
```

It keeps `id` so a pulling client can find the local row, and deliberately carries no name,
description, grade or source. A client that predates tombstones skips it rather than inventing a
skill; this client recognises it and deletes the row.

Tombstones are retained. A device that was offline still has to learn about a deletion, so the
marker is not pruned by this client. Re-indexing the same skill later writes a normal live document
over the tombstone, which is how a row returns.

A machine that has never pushed (a fresh install, or one that predates the manifest) has no
record of what it mirrored, so a row deleted before its first push cannot be tombstoned. The
manifest starts with that first push.

### Reading it from another client

Any Firebase client with the same account can read the collection; the rules decide access. The
REST path is:

```
GET https://firestore.googleapis.com/v1/projects/{project}/databases/(default)/documents/users/{uid}/skills
Authorization: Bearer {firebase-id-token}
```

Firestore wraps every value (`{"fields":{"name":{"stringValue":"pdf"}}}`); the table above is the
logical shape. A client that only needs the catalogue can flatten `fields` and ignore the rest.

### Published sessions

A session header is written **after** its turn batches, so a client that lists sessions never sees a
header whose turns did not land.

| Session field | Firestore type | Meaning |
|---|---|---|
| `id` | string | The local session id |
| `title` | string | As named on the computer, or `Untitled session` |
| `project` | string | The project folder's **name**, not its path |
| `status` | string | `active`, `idle`, `interrupted` or `ended` |
| `created_at`, `ended_at` | integer, ms since epoch | When the session began and ended |
| `spend_micros`, `budget_cents` | integer | Spend and the optional cap |
| `turn_count`, `agent_count` | integer | What the header counted at publish time |
| `published_at` | timestamp | When this machine published it |
| `app`, `schema` | string / integer | `Rigel`, and `1` for this shape |

| Turn field | Firestore type | Meaning |
|---|---|---|
| `id`, `order`, `seq` | string / integer | The turn, and its position in publication order |
| `prompt` | string | What the person asked; clipped at 8 000 characters |
| `reply` | string | The assistant's own text, joined in order; clipped at 40 000 characters |
| `status`, `stop_reason` | string, optional | How the turn ended |
| `started_at`, `completed_at` | integer, optional | Milliseconds since epoch |
| `harness`, `role`, `model` | string, optional | Which agent answered |
| `cost_micros`, `input_tokens`, `output_tokens` | integer, optional | The turn's bill |
| `subagent` | boolean | The turn belongs to a subagent |
| `truncated` | boolean | A prompt or reply hit its limit; the document says so instead of pretending |
| `published_at`, `app`, `schema` | | As above |

A turn document has **no tool calls, reasoning, commands or file changes**. Command arguments are
where tokens and diffs live, so they are never part of the published shape.

## Publishing a session

From the window, open a session and press **Publish to phone** beside **Export session**. From the
terminal:

```bash
rigel cloud publish <session-id> --project .
rigel cloud publish <session-id> --json     # the report, for a script
```

Publishing is an upsert: publishing again after more turns rewrites the header and every turn. A
turn deleted on the computer is not deleted in Firestore — deletion sync is not built, and the phone
would otherwise lose history on a rename. Sessions are independent of the room: the phone can read
them with the computer off.

### Reading it from the iPhone

The phone has its own Rigel Cloud screen (Connect → **Sessions published from your computer**, or
the room's settings). Sign in with the same Firebase account and project; the refresh token goes to
the phone's keychain. It lists `sessions` ordered by `created_at desc` and reads one session's
`turns` ordered by `order`, both with the ID token in the `Authorization` header.

```
GET /v1/projects/{project}/databases/(default)/documents/users/{uid}/sessions?pageSize=50&orderBy=created_at%20desc
GET /v1/projects/{project}/databases/(default)/documents/users/{uid}/sessions/{id}/turns?pageSize=300&orderBy=order
Authorization: Bearer {firebase-id-token}
```

Both queries use a single field and need no composite index. The phone decodes `fields` exactly as
written above; its tests are in `ios/RigelPhone/Tests/RigelRoomKitTests/CloudKitTests.swift`.

## What a sync does

- **Push** writes the account document, then every local shelf row in commits of 400 writes. It is
  an upsert by document id: last writer wins, per row. It then writes a tombstone for every id
  that was in this machine's manifest and is no longer in the shelf.
- **Pull** lists up to 20 pages of 300 documents and upserts every live row by `id`. A row already
  in the local index keeps its body and its local grade (`write_skill` uses `COALESCE`); a new row
  arrives with its source, grade and evidence intact. A tombstone deletes the matching row, its
  evidence and its version history; the body blob is left to `rigel skills gc`.
- **Deletion is a tombstone.** Remove a row with `rigel skills remove <id>`, then push: the id was
  in the manifest, is absent from the shelf, and is written as `deleted: true`. The next pull on
  another machine removes it there too. A first push from a fresh machine has no manifest, so it
  can never delete rows the account already has. Pulling also adds the rows it received to the
  manifest, which is why a row deleted on a machine that only pulled still propagates.
- **The manifest is per account.** `cloud-state.json` records the project id, the uid and the ids
  this machine has mirrored. If you sign in to another account, the file is ignored and rebuilt;
  its timestamps are not shown against the new account. Delete the file to reset the manifest —
  the next push simply rebuilds it and no tombstones are written.
- **Push before pull.** A pull brings the cloud's live rows down again, so a row removed locally
  but not yet pushed can be restored by a pull. `rigel cloud sync` performs the push first; use
  the two buttons in that order when deleting.
- **Tombstone propagation pauses above the cap.** Push copies the first 5 000 rows in
  `last_seen_at` order. If the shelf is larger, a truncated catalogue cannot tell a deleted row
  from one that simply did not fit, so no tombstones are written and the report says `truncated`.
  The last complete manifest is kept.
- **The sync clock is visible.** Push and pull both stamp `cloud-state.json`; **Settings → Cloud**
  shows the last sync and each direction, and `rigel cloud status` prints the same. `last_sync_at`
  is the later of the two clocks, taken from this machine's clock when the request finished — not
  a Firestore server timestamp. The window renders it with the same UTC `when()` helper as every
  other timestamp.
- **Bodies stay local.** A second machine can search and read metadata, not the body, until it
  fetches the body itself (`rigel skills index`).

Because a push rewrites every row, repeated pushes cost one write per row. The Spark plan's free
daily quota is usually enough for a personal shelf; watch the Firestore usage page if you sync a
5 000-row index on every launch.

## Troubleshooting

| Message | Fix |
|---|---|
| `no Firebase project configured` | Settings → Cloud, or `rigel cloud configure --project <id>` |
| `the email or password is wrong` | Check the account exists; **Create account** adds one |
| `that email already has an account` | Sign in instead, or send a reset link |
| `email/password sign-in is disabled` | Enable the provider in Authentication → Sign-in method |
| `this project has no email/password sign-in` | Enable Email/Password in Authentication → Sign-in method |
| `permission denied: deploy firestore.rules` | `npx firebase-tools deploy --only firestore:rules` |
| `Firestore is not enabled in this project` | Create the Firestore database in the console |
| `the Firebase web API key is not valid` | Copy the Web API key from Project settings again |
| `App Check rejected this request` | The project enforces App Check and the token was missing or stale. Set the endpoint in Settings → Cloud → App Check or `rigel cloud app-check --url`, and check the endpoint answers with `token` and Unix-second `expires_at` |
| `the attestation endpoint is not reachable` | Rigel fails closed instead of sending an un-attested request. Check the URL, the function's deployment and its authorisation; `rigel cloud app-check` prints the stored URL |
| `the attestation endpoint refused this client (401)` | The installation's client token is missing or not in `RIGEL_ATTEST_CLIENTS`. Store the raw token with `rigel cloud app-check --client-token` (or Settings → Cloud → App Check), and confirm its SHA-256 is in the secret |
| `the attestation endpoint answered 429` | The installation or address spent its token bucket. `Retry-After` is in the body; wait, or raise the limits in `functions/.env` and redeploy |
| `the attestation endpoint answered 503` | The function has no `RIGEL_FIREBASE_APP_ID`, or `RIGEL_ATTEST_CLIENTS` is empty or malformed. Read the function logs; the value itself is never logged |
| `expires_at is more than seven days away` | The endpoint returned milliseconds. It must be Unix **seconds**, matching the token's TTL |
| Sign-in works, then every write is rejected after enforcing App Check | Authentication enforcement is separate from Firestore enforcement; make sure both clients are updated and configured, or temporarily turn Authentication enforcement off in App Check → APIs |
| No reset email arrives | Check spam; the address's domain is not blocked; and the project has a sending domain under Authentication → Templates |
| `the stored Firebase session is unreadable` | `rigel cloud logout`, then sign in again |
| The verification email never arrives | Check spam, then **Resend verification email** or `rigel cloud verify`. The sender and template are under Authentication → Templates |
| Still “not verified” after opening the link | **I've verified — check again**, or `rigel cloud verify --check`. Opening the link on any device verifies the account; the page only learns it when it asks |
| `this account's email is already verified` | The session already carries the flag. `rigel cloud status` and the account page show the stored answer; the send control is hidden while it is true |
| `too many attempts; try again later` | Firebase rate-limits verification and reset emails; wait before resending |
| `cloud-state.json is unreadable` | Delete `<data dir>/cloud-state.json`; the next push rebuilds the manifest. This machine's deletion history is lost, so rows removed before the reset will not be tombstoned |
| A deleted row came back | Re-indexing the id writes a live document over the tombstone; that is intentional. Remove it again and push |
| A tombstone did not delete a row | Tombstones exist only after the machine that removed the row pushes. Pull on the other machine, and check its `tombstones` / `deleted` counts |
| Last sync says “never” | The clock lives in `cloud-state.json` for this account; `rigel cloud status` shows what the current sign-in can see |
| `no session on the phone` | Press **Publish to phone** in that session; a new account has published nothing |
| `permission denied` on the phone | Deploy `firestore.rules`; the signed-in uid must be the subtree's uid |
| `the Firebase web API key is not valid` on the phone | The key may be restricted to HTTP referrers, which a native app does not send; allow the iOS app or use an unrestricted web key |
| Phone says the token expired | Sign out and back in on the phone; a revoked refresh token cannot be renewed |

## Environment variables

| Variable | Purpose |
|---|---|
| `RIGEL_FIREBASE_PROJECT_ID` | Project id, when `cloud.json` is absent |
| `RIGEL_FIREBASE_API_KEY` | Web API key, when `cloud.json` is absent |
| `RIGEL_FIREBASE_APP_CHECK_URL` | Attestation endpoint, when `cloud.json` is absent or has no App Check URL |
| `RIGEL_FIREBASE_APP_CHECK_CLIENT` | This installation's App Check client token, when the keychain has none (headless/CI) |
| `RIGEL_CLOUD_EMAIL` | `scripts/cloud-verify.mjs`: the disposable test account |
| `RIGEL_CLOUD_PASSWORD` | `scripts/cloud-verify.mjs`: that account's password, and the pre-reset one |
| `RIGEL_CLOUD_NEW_PASSWORD` | `scripts/cloud-verify.mjs`: the password set through a reset link |
| `RIGEL_FIREBASE_IDENTITY_BASE` | Override the Identity Toolkit host (tests/gateway) |
| `RIGEL_FIREBASE_TOKEN_BASE` | Override the secure-token host (tests/gateway) |
| `RIGEL_FIREBASE_FIRESTORE_BASE` | Override the Firestore host (tests/gateway) |

The three `*_BASE` variables exist for the local test server in
`crates/rigel-core/tests/cloud.rs`; a normal install never sets them.

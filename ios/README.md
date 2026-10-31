# RigelPhone — the room, on your phone

An iOS client for the room agentchattr serves — the same room the desktop app's Chat tab drives.
You point it at an address and it shows you what the room is doing: the timeline, the board, the
rule book, who is in it, what it calls itself. It writes twice, both behind a confirmation card —
one chat line at a time, and one answer to a decision card an agent is blocked on — and it can put
photos and documents into the room's own uploads folder. Moving a job, activating a rule, starting
an agent and tool approvals stay on the desktop (handoff D103, narrowed for decisions by D167).

## Layout

```
ios/
  RigelPhone/                     the xtool package
    Sources/RigelRoomKit/         wire format, HTTP client, room socket — Foundation only, so Linux builds it
    Sources/RigelPhone/           the SwiftUI app
    Resources/Icon/               every icon slot, drawn by tools/make-icon.py
    Tests/RigelRoomKitTests/      tests, plus Fixtures/ captured from a real room
    Info.plist                    ATS local-networking keys and the local-network usage string
    xtool.yml                     bundle id: dev.rigel.phone
  tools/capture-fixtures.py       records the REST fixtures from a running room
  tools/ws-frames/main.swift      records the socket frames from a running room
  tools/make-icon.py              draws the icon once and exports every slot
  tools/package-ipa.sh            build, inject the icon, write an installable IPA
  tools/patch-icon-plist.py       names those icon files in the built Info.plist
```

The kit is deliberately free of SwiftUI and of Apple-only frameworks: the wire format is the part
that can be wrong in ways no compiler catches, so it is the part that must be testable wherever a
Swift toolchain exists.

## The look

The phone is deliberately a *different* instrument from the desktop, not a shrunk copy of it. The
desktop is a dense working surface; a phone is carried, glanced at and put away, so this one is
built around one bright thing at a time.

- **Palette** — near-black with a cold midnight above and one amber light, the desktop's `#e9a23b`.
  The palette is in `Sources/RigelPhone/Theme.swift`; the lanes and agents borrow colour from the
  room, so the board is readable before it is read.
- **One background** — `AuroraBackground` draws a slow-drifting night sky (gradients plus a fixed,
  seeded star field) instead of shipping an image. No bundle cost, every screen size, and it cannot
  band the way a large JPEG would.
- **Two type voices** — rounded for anything that speaks (titles, messages), monospaced for
  anything that labels (eyebrows, counts, times, room addresses). That split carries the hierarchy.
- **Glass** — every surface is a real `.ultraThinMaterial` panel with a light-catching gradient
  border and a soft shadow (`GlassCard`), over the aurora rather than on flat black.
- **One voice for problems** — `NoticeBanner`: a tinted glass strip with an icon, used for a
  refused address, a dropped stream and a line that would not send, so a warning looks the same
  wherever it is raised. The timeline, the job sheet's thread and the decision cards all speak
  with the same coins, agent colours and tinted bubbles, so opening a job does not read as a
  different app.
- **Motion** — spring press feedback on every tappable card, a pulsing live dot, a breathing star,
  staggered rule arrival, spring transitions on new lines, and haptic selection on the tab bar.
  Motion is feedback, never decoration on a loop; the three looping ones (sky drift, breathing
  star, live pulse) are turned static when Settings → Accessibility → Reduce Motion is on.
- **The mark** — the star from Rigel's logo, traced from `ui/src/mark.tsx` instead of redrawn:
  four concave points in the brand amber, on transparency with a soft bloom (`StarMark`). In the
  room header the same star is the settings control — a tap opens the room's settings, and holding
  it charges the glyph, closes a ring around it and bursts before the sheet appears.

The icon is generated, not drawn in a design tool: `python ios/tools/make-icon.py` renders a
4096 px master (star, bloom, orbit, dust), downsamples it into every slot iOS asks for, and writes
`AppIcon-1024.png` plus the `AppIcon…` PNGs and `LaunchStar.png` into `Resources/Icon/`. It is
text-free by rule — an icon that spells its own name is unreadable at 29 px. The 1024 file is
opaque RGB, which is what the App Store requires.

xtool does not run `actool`, so there is no `Assets.car` to name. The slots are copied beside the
executable and named in `CFBundleIcons` by `tools/patch-icon-plist.py`; `UILaunchScreen` points at
`LaunchStar.png`. `ios/tools/package-ipa.sh` does all of it in one step:

```bash
bash ios/tools/package-ipa.sh ~/Downloads/RigelPhone.ipa   # build + icon + zip
```

## Rigel Cloud on the phone

Besides the live room, the phone can read **sessions the computer published** to the user's own
Firebase project. It is independent of the room: the computer can be off, and a session sent this
morning is still readable.

- **Where** — the connect screen's **Sessions published from your computer**, or the room settings
  sheet's **Published sessions**.
- **Sign-in** — the project id and the public web API key from Firebase Project settings, then the
  same email and password the desktop uses. The password is used once; the refresh token goes to
  the phone's keychain, and the ID token is renewed from it at launch.
- **What it reads** — `users/{uid}/sessions` ordered by `created_at desc`, and one session's
  `turns` ordered by `order`. The decoder is in `Sources/RigelRoomKit/CloudKit.swift`; the document
  shape is `docs/cloud.md` on the desktop side.
- **What it shows** — the prompt and the assistant's own words, per turn, with role, model, time
  and cost. Tool calls, command output, file changes and the project path are never published, so
  there is nothing hidden to expand.

The Firebase project id and web API key are stored in `UserDefaults` (neither is a secret); the
account blob is a keychain item under `firebase-session`. Firebase's rules still decide access:
every read is a request with the signed-in user's ID token, and no other account can name this
subtree.

## What stays on the desktop

The phone is a remote control for the room, not a second admin console. The line is drawn by what
an action can do when the screen is five inches and the person is somewhere else:

- **Authority over runs** — starting or stopping agents, choosing a harness, provider, model,
  effort or budget, and holding the API keys. These decide what spends money and what acts.
- **Approvals** — the tools an agent asks to run are approved where the diff, the worktree and the
  command are visible. A phone cannot show enough to answer safely, so it never offers.
- **Board writes** — moving or reordering jobs rewrites a whole lane's `sort_order`; the phone reads
  the board and answers what a job is blocked on, which is a decision card in the timeline.
- **Rules** — activating, archiving or proposing a standing order changes every agent's context for
  every job from then on. The phone reads the rule book, with the reason it is read-only on screen.
- **Code and git** — sessions, worktrees, patches, apply, commit, push, pull requests and the
  terminal. All of it writes to the computer; none of it is reachable here.
- **The room itself** — booting it, choosing the project, export/import, updates, diagnostics and
  signing. The phone joins a room that is already running.

What the phone may do is short on purpose: read the room, say one line, answer one decision card,
and put files into the room. Uploads are the exception because they add context rather than
authority: the file lands in the room's uploads folder, is served by the room, and the phone never
runs it — images are drawn, documents are a link. The room keeps photos and the documents listed
beside `ALLOWED_UPLOAD_EXTS` in `agentchattr/app.py`; anything else is refused before the bytes
leave the phone. The desktop Chat does not draw attachments yet: agents see them through the
room's MCP bridge, and the phone shows them on the line that carried them.

## Tests

No Apple toolchain needed — this runs in WSL:

```bash
wsl -d Ubuntu -- bash -lc 'cd /mnt/c/Users/cr0od/Documents/GitHub/rigel/ios/RigelPhone && RIGEL_LINUX_TEST=1 swift test'
```

`RIGEL_LINUX_TEST=1` exists because `swift test` plans every target in the package, and the app
target needs SwiftUI and `Security` — which do not exist on Linux — while xtool builds that target
*on* Linux. Without the variable the manifest is the ordinary xtool shape: one library product,
the app. The tests decode captured payloads and cover address parsing, token handling, the socket's
frames, the board, the rule book, decision cards, the line the phone sends and every error
sentence; 72 tests, currently green — seven of them skip themselves unless they are pointed at a
room, and three of those write to it.

One test talks to a real server if you point it at one, which is how transport (headers, query
encoding, the 403 path) gets checked rather than assumed:

```bash
RIGEL_LINUX_TEST=1 RIGEL_ROOM_BASE=http://192.168.4.126:8311 RIGEL_ROOM_TOKEN=<token> \
  swift test --filter LiveRoomTests
```

`LiveStreamTests` opens the real socket against the same room, which is how the upgrade handshake,
the token in the query string and the snapshot get checked rather than assumed. Add
`RIGEL_ROOM_LIVE_PUSH=1` and it also posts a probe line while the socket is open, which is the only
way to prove a push end to end:

```bash
RIGEL_LINUX_TEST=1 RIGEL_ROOM_BASE=http://192.168.4.126:8311 RIGEL_ROOM_TOKEN=<token> \
  RIGEL_ROOM_LIVE_PUSH=1 swift test --filter LiveStreamTests
```

`LiveSendTests` is the phone's own send path against the same room: it writes a line through
`RoomConnection.send`, waits for the room to echo it back down the socket it wrote on, and then
asks the room's log for it — behind the same switch, because it writes too:

```bash
RIGEL_LINUX_TEST=1 RIGEL_ROOM_BASE=http://192.168.4.126:8311 RIGEL_ROOM_TOKEN=<token> \
  RIGEL_ROOM_LIVE_PUSH=1 swift test --filter LiveSendTests
```

That switch is opt-in because it writes: point it at a room of your own, not one someone is using.

The same works over a Tailscale name — `RIGEL_ROOM_BASE=https://<device>.<tailnet>.ts.net` — which
is the transport the phone actually uses, and it passes here: WSL resolves MagicDNS names, so the
whole TLS path, the socket upgrade included, can be exercised without a device in hand.

## The socket

The room pushes; the phone does not poll. On connect the client opens
`ws(s)://<room>/ws?token=…` — the room's own web-UI channel, so the token goes in the query string
and a wrong one is answered with a close code. The room then sends the whole room as frames:
`settings`, the frames this client has no use for (`agents`, `base_colors`, `todos`, `rules`,
`hats`, `jobs`, `schedules`), saved history in batches, `history_complete`, and finally `status`.
After that every new line (`message`), deletion (`delete`), clear (`clear`) and roster change
(`status`) arrives as it happens.

Three decisions worth knowing:

- **A frame the client cannot read is ignored, not fatal.** The socket is one long-lived
  connection, and a phone that drops it because the room learned a new trick loses the
  conversation. The room's other business arrives as a named `.ignored` frame, so a test can tell
  "ignored on purpose" from "unreadable".
- **A reconnect re-reads; it does not patch.** Every connection starts with the room's snapshot, so
  the model collects frames into a buffer and swaps it in at `history_complete`: the timeline never
  blanks while a new socket fills.
- **Reconnection is one socket at a time, decided in `RoomModel`**: 1 s, then 2/4/8/15 s backoff,
  and each attempt adopts the token from the landing page, because every room restart mints a new
  one. Leaving the foreground closes the socket; coming back opens a fresh one.

`RoomStream` makes one connection per `connect()` call, and a line is written on that same
connection rather than on one of its own: the room answers a fresh socket with the whole snapshot
again, so a second connection would re-download the history once per line. See *Sending one line*,
below.

## Sending one line

A phone that can read a room but never answer it is a phone you stop carrying, so the composer at
the bottom of the room does send — through a confirmation card, one line at a time.

- **The line goes on the room's own socket.** `RoomConnection.send` writes the frame the room's web
  UI writes (`{"type": "message", "text": …, "channel": …}`) on the connection the timeline is
  already reading. It carries no `sender`: the room attributes a line to the session that sent it,
  which is why nothing here can write as an agent.
- **The confirmation card is the step.** The paper-plane stages the line
  (`RoomModel.proposeSend`); the card then shows it exactly as the room will store it, names the
  channel and the room, and says who sees it. Nothing is written until Send on that card, and
  dismissing the card any way at all — Cancel, or a downward swipe — cancels the send. A blank
  draft is not a line and never reaches the card.
- **The room's echo is the confirmation.** Nothing is drawn optimistically: the line appears in the
  timeline when the room broadcasts it back, which is the frame every other client sees. What is on
  screen stays the room's record of what was said rather than the phone's hope about it.
- **A failed send keeps the draft.** The room's own sentence — or `RoomError.notConnected` for a
  socket that has gone — appears under the composer, and the line stays typed, so a retry is one
  confirmation away.
- **What is still off-limits.** Nothing here approves a tool call or starts an agent. The one
  other write is a decision card's answer, and it goes through the room's own endpoint rather than
  as prose — see *Answering a decision*, below. A line that *mentions* an agent is a normal thing
  to say in a room whose agents auto-trigger on mention, and the card says so before it goes — the
  phone's version of the room's own warning.

## Board, rules and decisions

Three surfaces the socket was already carrying and the client used to throw away. They share the
connection the timeline reads: `jobs` and `rules` arrive whole on connect, then `job` and `rule`
one item at a time, so switching tabs costs nothing and nothing is polled.

- **Board** — every job grouped into the room's lanes (`open`, `done`, `archived`), with its
  assignee, channel and thread; tap one for the brief and its conversation. Read-only: the room's
  own board moves a job by dragging, which rewrites the whole lane's `sort_order`.
- **Rules** — the standing orders, active first, each with who proposed it and why. Read-only:
  an active rule is injected into every agent's context, so activating one is a desktop decision.
- **Answering a decision** — a message of type `decision` carries `metadata.choices`, and the card
  is drawn in the timeline with one button per choice. The answer is confirmed first, then sent to
  `POST /api/messages/{id}/resolve_decision`; the room checks the choice against the card, records
  it, posts `@sender <choice>` in the channel and broadcasts `message_update`. The phone settles on
  that broadcast, exactly as it settles on the room's echo of a line — it never draws an answer the
  room has not recorded.

`LiveWorkTests` is the same two writes against a real room. It needs a decision card to answer, and
nothing on the HTTP surface creates one (agents make them through the room's tools), so the test
skips on a room without one rather than seeding it itself.

## Running a room to talk to

The stock room is on `127.0.0.1:8300` and only the laptop can reach that address, so use a copy
and a port of your own. The embedded runtime the sidecar ships is enough — no system Python:

```bash
# from the repository root, in Git Bash or WSL
repo="$PWD"
scratch="${TMPDIR:-/tmp}/rigel-room"
rm -rf "$scratch"
cp -r crates/rigel-app/resources/sidecar/agentchattr "$scratch"
cd "$scratch"
sed -i 's/^host = "127.0.0.1"/host = "0.0.0.0"/' config.toml      # bind the network
printf 'YES\n' | "$repo/crates/rigel-app/resources/sidecar/python/python.exe" \
  run.py --allow-network --port 8311
```

The runtime is `python/python.exe` on Windows and `python/bin/python3` elsewhere, relative to
`crates/rigel-app/resources/sidecar/` — use the one your platform has. The `YES` is not
decoration: `run.py` refuses to listen on anything but loopback without `--allow-network`, and
then asks you to accept its warning in so many words. It prints `Session token: …` on startup —
that is the token the phone wants.

## Building the app

xtool 1.24.0 and Swift 6.4.1 are installed in WSL Ubuntu, and **the Darwin SDK is installed**
(`xtool sdk status` → `darwin.artifactbundle`), so `xtool dev build` compiles the app target here
rather than syntax-checking it. On a fresh machine the SDK is the one step that needs a file only
you can fetch: `Xcode.xip`, from <https://developer.apple.com/download/all/?q=Xcode> — roughly
8 GB, and the download wants your Apple ID in a browser. There is no prebuilt `darwin.xtoolsdk`
to install instead: xtool's own releases carry only the AppImage and a dSYM.

```bash
xtool sdk status                                              # should name the darwin bundle
xtool sdk install /path/to/Xcode.xip --slim                   # or Xcode.app, or darwin.xtoolsdk
```

The project itself is already in xtool's shape, and that was checked against xtool 1.24 rather
than guessed, because it is the one class of failure the SDK cannot reveal. `xtool.yml` matches
`PackSchemaBase`: `version`, `bundleID`, and a *top-level* `infoPath` — so the ATS and
local-network keys in `Info.plist` really are applied, and are not being ignored the way they would
look if the key were the extensions-only one (`xtool`'s own examples carry only the first two
keys). `Package.swift` is the shape its examples and its app-extension guide use: one library
product, a plain `.target`, `.iOS(.v17)`, the package name and the product name agreeing. The one
file `xtool new` writes that this package does not have is `.bsp/xtool.json` — the LSP support
file — and xtool creates that itself on the next build (`skipLSP` in the schema is how you opt
out).

Copy the xip into the WSL filesystem before installing it
(`cp /mnt/c/Users/<you>/Downloads/Xcode.xip ~/`): xtool unpacks something like 10 GB out of it,
and the Windows mount is the slow way to read that much. It lands in `~/.swiftpm/swift-sdks`, so
`swift sdk list` prints `darwin`; `xtool sdk build <xip> <dir>` writes a portable
`darwin-<version>.xtoolsdk` instead, if you would rather unpack once and keep the bundle.

The install does not have to come first: a build with no SDK asks for the same path itself
(`xtool dev build` prints `Path to Xcode.xip:` and waits), so the first build can be the install.
From the package directory:

```bash
xtool dev build                    # build only — no device needed
xtool dev                          # build, install and run on an attached device
xtool devices                      # attached devices
xtool install <ipa>                # sideload
xtool dev generate-xcode-project   # if you would rather build on a Mac
```

Signing for a device needs `xtool auth login` (an Apple ID) once. Expect one more unverified step
for a physical phone: xtool on WSL talks to the device over USB, so the phone has to be visible to
the Linux kernel — `usbipd-win` is the usual way to attach it. None of this has been exercised
yet; the fixtures and the live tests are the only parts verified against reality so far — the app
target above them is syntax-checked (`swiftc -parse`) and read, not built.

## What the phone needs from the desktop

Three facts, in the order they will bite:

1. **The room listens on loopback.** `[server] host` in the room's `config.toml` is `127.0.0.1`,
   so the room the desktop spawns is not reachable from anywhere else — not from the phone, not
   from another machine on the LAN.
2. **Exposing it is deliberate.** The room requires a non-loopback `host`, the `--allow-network`
   flag *and* an interactive `YES`. On this machine the address a phone would type is the Ethernet
   address `192.168.4.126` (its Hamachi address, `25.0.41.206`, is the other candidate if you
   have a VPN client on the phone).
3. **The room's own warning is the accurate one.** Plain HTTP, no TLS, and whoever holds the
   token can `@mention` agents — with auto-approve enabled, that is remote code execution from
   anyone on the network. Home networks only. The phone's "leave the token blank and fetch it from
   the room" convenience is exactly as private as the network it runs on, since the room serves
   its own token to any page reader.

Keeping the room on loopback and tunnelling to it is safer and needs no desktop change:
`tailscale serve` on the desktop (HTTPS over the tailnet, room untouched) or an SSH tunnel from
the phone (`ssh -L 8300:127.0.0.1:8300 desktop`) both work without touching the room's config.
If the desktop app grows a "share over the network" switch, it should carry this same warning;
that is a desktop-side decision and has not been made here.

### Away from home: Tailscale (serving, verified over TLS)

Tailscale 1.104.2 is installed here (`C:\Program Files\Tailscale\tailscale.exe`), signed in, and
this machine is a node on the tailnet. The room on `127.0.0.1:8300` is published on the tailnet
over HTTPS:

```
$ tailscale serve status
https://<device>.<tailnet>.ts.net (tailnet only)
|-- / proxy http://127.0.0.1:8300
```

```bash
tailscale serve --bg 8300    # publish the room (already done; re-run after a room restart)
tailscale serve status       # the exact name, and what is being served
tailscale serve reset        # stop serving
```

Verified through that name: the landing page carries its token, `/api/settings`, `/api/status` and
`/api/messages` answer `200` with the token and `403` without one (and with a wrong one), plain
`http://` on the same name is refused — the proxy only listens with TLS — and the kit's own live
tests pass against it over TLS (`swift test --filter 'LiveRoomTests|LiveStreamTests'` with
`RIGEL_ROOM_BASE` set to the name), so the client, the route and the socket upgrade all agree. Two things worth knowing: the first request after
publishing can take ~20 s because the certificate is issued on first use, and Serve is an
account-level feature that had to be enabled once at `https://login.tailscale.com/f/serve` before
any of this would start.

The room keeps listening on `127.0.0.1`: no `--allow-network`, no firewall rule, no LAN exposure,
and the token never crosses a plaintext hop off the machine. The address to type in Rigel is the
`https://…ts.net` name `serve status` prints — the app accepts a full URL and speaks TLS, so
nothing else changes. Install the Tailscale app on the phone and sign in with the same account
first; tailnet membership is the phone's permission to reach the room. If `serve` then asks about
HTTPS certificates, that toggle is on the admin console's DNS page. `tailscale funnel` is the
public-internet version of this and is not for this room.

### Two names for one room

The app keeps two addresses for the same room: at home its LAN address, away from home the
`tailscale serve` name. It tries them in order and uses the first that answers, and the one that
answered is promoted to the front — so the slow attempt against a network the phone has left is
made once rather than on every launch. Nothing about the token changes: a blank token is still read
from that address's own landing page, and a token the room refuses is re-adopted there before the
next address is tried, because every room restart mints a new one.

The promotion is not only a connect-time affair. Leaving the house with the app open is the case
this exists for: the socket notices that nothing answers at all, asks both addresses again and
moves to the one that does, so the timeline carries on instead of retrying a network that is gone.

## What it does today

- **Connect** — address as someone would type it (`192.168.1.42`, `192.168.1.42:8300`,
  `studio.local`, or a whole pasted `http://…` URL) and a token. Leave the token blank to read it
  from the room's landing page and prove it through `/api/settings` before trusting it. A second
  field holds the room's other name — see *Two names for one room*, above — and whichever answers
  leads the next attempt.
- **Room** — channel picker, message timeline (narration lines set apart, fenced code rendered,
  newlines preserved), roster with presence, and a live socket: a line appears as it is said, a
  roster change as it happens.
- **Board** — the room's jobs, lane by lane, with each job's brief and thread. Read-only.
- **Rules** — the rule book, active first. Read-only.
- **Answer a decision** — a card in the timeline is answerable in one tap, confirmed, and settled
  from the room's own `message_update`.
- **Say one line** — type it, confirm the card, and it goes on the room's socket; the room's own
  echo is what appears in the timeline. Approvals and starting an agent stay unreachable.
- **Settings sheet** — what the room calls itself, its channels, its agents.
- **Rigel Cloud** — sign in to the user's Firebase project and read the sessions the computer
  published, turn by turn. Read-only; publishing happens on the desktop.
- The room token and the Firebase refresh token live in the keychain, and nothing is written to a log.

Not there yet: tool approvals, moving jobs or activating rules, editing or deleting a line, replies,
attachments, notifications, background refresh.

## Fixtures

`Tests/RigelRoomKitTests/Fixtures/` holds real payloads: `/api/settings`, `/api/status`,
`/api/messages`, one `since_id` answer, the room's 403 body, the landing page's token line with the
token redacted, and two socket captures — `ws_snapshot.jsonl`, the frames a connection opens with,
and `ws_push.jsonl`, a roster change and a line posted while the socket was open — one raw frame
per line. Regenerate the REST payloads from a running room:

```bash
python3 ios/tools/capture-fixtures.py \
  --base http://127.0.0.1:8311 --log <room log> --out ios/RigelPhone/Tests/RigelRoomKitTests/Fixtures
```

`--base` is deliberately required: the stock port 8300 belongs to a real room, and a capture run
registers two agents and posts a message as each. The fixtures are only deterministic on a room
reset to a single seeded line, and the script's docstring carries the reset recipe. It writes
`settings.json`, `roster.json`, `messages.json`, `messages_since.json`, `room_error.json` and
`landing_script.html`.

The two socket captures come from the same throwaway room, recorded through the shipping client —
the tool compiles against the kit's sources, so what it prints is what the app decodes:

```bash
swiftc -O ios/tools/ws-frames/main.swift \
       ios/RigelPhone/Sources/RigelRoomKit/*.swift -o /tmp/ws-frames

# the frames a connection opens with
/tmp/ws-frames 192.168.4.126:8311 <token> 4 \
  > ios/RigelPhone/Tests/RigelRoomKitTests/Fixtures/ws_snapshot.jsonl
```

For `ws_push.jsonl`, run the same command in the background with a longer window and register an
agent or post a line while it waits — the pushed frames land in the file, and the decoded frame
types go to stderr so a capture run doubles as a check that the decoder agrees with the server.
Neither capture can carry the token: the frames do not contain it.

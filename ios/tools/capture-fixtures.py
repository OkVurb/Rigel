#!/usr/bin/env python3
"""Capture room API fixtures from a running agentchattr room.

The iOS protocol tests decode real payloads rather than hand-written ones, so
they keep matching what the server actually sends. Run this against a
throwaway room — a temp copy of `crates/rigel-app/resources/sidecar/agentchattr`
with its own data dir — because it registers two agents and posts one message
as each of them.

usage:
  python ios/tools/capture-fixtures.py \
      --base http://127.0.0.1:8311 \
      --log  C:/Users/<you>/AppData/Local/Temp/rigel-room/room.log \
      --out  ios/RigelPhone/Tests/RigelRoomKitTests/Fixtures

what it writes, and from which endpoint:

  settings.json        GET /api/settings   title, subtitle, channels, username
  roster.json          GET /api/status     the roster plus `paused`
  messages.json        GET /api/messages   the whole log (three lines)
  messages_since.json  GET /api/messages?since_id=2   one line, the cursor case
  room_error.json      GET /api/settings   with a wrong token (403 body)
  landing_script.html  GET /               the token line, with the token redacted

The room's socket frames are captured separately, by `ios/tools/ws-frames` — see the README.

`messages.json` is only deterministic on a clean room: one seeded line from the
user (id 1), then one line each from `claude` and `codex` (ids 2 and 3). Reset
the throwaway room before capturing, or the ids and senders drift:

  rm -rf <room>/data/* && printf '%s\n' '<one user line>' > <room>/data/agentchattr_log.jsonl
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import urllib.error
import urllib.request

# The session token is replaced by this before the landing-page snippet is
# written, so a captured fixture never carries a usable secret.
REDACTED_TOKEN = "0123456789abcdef" * 4

MESSAGES = {
    "claude": "On it. Two things stand out in the storage layer; I will post the diff after lunch.",
    "codex": "Agreed. The WAL settings are the other half:\n\n```\njournal_mode = WAL\nsynchronous = NORMAL\n```",
}


def call(base: str, path: str, token: str | None = None, method: str = "GET",
         body: dict | None = None, bearer: str | None = None) -> tuple[int, bytes]:
    request = urllib.request.Request(base + path, method=method)
    if token:
        request.add_header("x-session-token", token)
    if bearer:
        request.add_header("Authorization", "Bearer " + bearer)
    data = None
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request, data, timeout=5) as answer:
            return answer.status, answer.read()
    except urllib.error.HTTPError as problem:
        return problem.code, problem.read()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True,
                        help="room origin; required, so a capture run can never land "
                             "on a real room by accident (the stock port is a live room's)")
    parser.add_argument("--token", help="session token; read from --log when omitted")
    parser.add_argument("--log", help="the room's stdout log, for the token line")
    parser.add_argument("--out", required=True, help="fixtures directory")
    args = parser.parse_args()

    token = args.token
    if not token:
        if not args.log:
            parser.error("pass --token or --log")
        log = pathlib.Path(args.log).read_text(encoding="utf-8", errors="replace")
        match = re.search(r"Session token: ([0-9a-fA-F]{32,128})", log)
        if not match:
            parser.error("no 'Session token:' line in " + args.log)
        token = match.group(1)

    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    # Two agents as the desktop would have them: registered by a wrapper,
    # then posting through the real send path with their own bearer tokens.
    registered = []
    for name in ("claude", "codex"):
        code, body = call(args.base, "/api/register", method="POST",
                          body={"base": name, "label": name.capitalize()})
        if code != 200:
            raise SystemExit(f"register {name} failed: {code} {body[:200]!r}")
        instance = json.loads(body)
        if "token" not in instance:
            raise SystemExit(f"register {name} returned no token: {sorted(instance)}")
        registered.append(instance)
        print(f"registered {instance.get('name')} ({instance.get('label')})")

    for instance in registered:
        base_name = instance.get("name", "").split("-")[0]
        text = MESSAGES.get(base_name, "Hello from the capture run.")
        code, body = call(args.base, "/api/send", method="POST",
                          bearer=instance["token"], body={"text": text, "channel": "general"})
        if code != 200:
            raise SystemExit(f"send as {base_name} failed: {code} {body[:200]!r}")

    settings = call(args.base, "/api/settings", token=token)
    roster = call(args.base, "/api/status", token=token)
    messages = call(args.base, "/api/messages?limit=50", token=token)
    since = call(args.base, "/api/messages?since_id=2&limit=50", token=token)
    denied = call(args.base, "/api/settings", token="not-the-token")
    if denied[0] != 403:
        raise SystemExit(f"a wrong token was not rejected: {denied[0]} {denied[1][:200]!r}")

    _, page = call(args.base, "/")
    html = page.decode("utf-8", errors="replace")
    found = re.search(r'<script>window\.__SESSION_TOKEN__="[0-9a-fA-F]{32,128}";</script>', html)
    snippet = found.group(0) if found else "<script>no token found</script>"
    snippet = re.sub(r'"[0-9a-fA-F]{32,128}"', f'"{REDACTED_TOKEN}"', snippet)

    def write(name: str, payload) -> None:
        (out / name).write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

    write("settings.json", json.loads(_checked(settings, "settings")))
    write("roster.json", json.loads(_checked(roster, "status")))
    write("messages.json", json.loads(_checked(messages, "messages")))
    write("messages_since.json", json.loads(_checked(since, "messages since")))
    write("room_error.json", json.loads(denied[1]))
    (out / "landing_script.html").write_text(snippet + "\n", encoding="utf-8")

    agents = sorted(name for name in json.loads(roster[1]) if name != "paused")
    print(f"agents captured: {', '.join(agents) or '(none)'}")
    print(f"channels captured: {json.loads(settings[1]).get('channels')}")
    print(f"messages captured: {len(json.loads(messages[1]))}")
    print(f"wrote fixtures to {out}")


def _checked(answer: tuple[int, bytes], what: str) -> bytes:
    code, body = answer
    if code != 200:
        raise SystemExit(f"{what} answered {code}: {body[:200]!r}")
    return body


if __name__ == "__main__":
    main()

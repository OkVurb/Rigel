#!/usr/bin/env bash
# Phase 4 exit test (P4.12): three harnesses, three real turns, one killed.
#
#   1. doctor sees all four harnesses in JSON
#   2. Codex runs a turn that records a command item with its exit code and input tokens
#   3. Cursor and Cline run ACP turns through the same client, with their own keys
#   4. killing one agent mid-turn leaves the others running; resume marks only that turn
#
# The turns are driven through fixture binaries so the script is deterministic and needs no
# vendor account. Point --exe at the real CLIs (or drop the flag) to run the same script live.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== build =="
cargo build -q -p rigel-cli --bin rigel
cargo build -q -p rigel-adapters --bin fake-codex --bin fake-acp
cli="$PWD/target/debug/rigel"
codex_fake="$PWD/target/debug/fake-codex.exe"
acp_fake="$PWD/target/debug/fake-acp.exe"

project="$(mktemp -d)"
logs="$(mktemp -d)"
trap 'rm -rf "$project" "$logs"' EXIT

"$cli" migrate --project "$project" >/dev/null
session="$("$cli" session start --project "$project" --budget-cents 500)"
echo "session $session"

echo "== 1: doctor lists every harness =="
set +e
"$cli" doctor --json --project "$project" > "$logs/doctor.json"
doctor_code=$?
set -e
[ "$doctor_code" -le 1 ] || { echo "doctor failed ($doctor_code)"; cat "$logs/doctor.json"; exit 1; }
python - "$logs/doctor.json" <<'PY'
import json, sys
doc = json.load(open(sys.argv[1], encoding="utf-8"))
names = {row["harness"] for row in doc["harnesses"]}
missing = {"codex", "cursor", "cline"} - names
assert not missing, f"doctor did not report {missing}: {names}"
assert all(row.get("path") or row.get("install_hint") for row in doc["harnesses"])
print(f"   {len(doc['harnesses'])} harness rows; {sorted(names)}")
PY

echo "== 2: codex records a command item and its exit code =="
cat > "$logs/doctor-command.jsonl" <<'JSON'
{"jsonrpc":"2.0","method":"item/started","params":{"item":{"id":"c1","type":"commandExecution","command":"rigel doctor --json","status":"inProgress"}}}
{"jsonrpc":"2.0","method":"item/completed","params":{"item":{"id":"c1","type":"commandExecution","command":"rigel doctor --json","status":"completed","exitCode":0}}}
{"jsonrpc":"2.0","method":"item/completed","params":{"item":{"type":"agentMessage","text":"doctor is green."}}}
{"jsonrpc":"2.0","method":"turn/completed","params":{"turn":{"id":"tn-1","status":"completed"},"usage":{"input_tokens":420,"cached_input_tokens":0,"output_tokens":9}}}
JSON
FAKE_CODEX_FIXTURE="$logs/doctor-command.jsonl" \
  "$cli" codex run --project "$project" --session "$session" \
  --exe "$codex_fake" --prompt "run rigel doctor --json" --json \
  > "$logs/codex.json" 2> "$logs/codex.err"
python - "$logs/codex.json" "$session" <<'PY'
import json, sys
run = json.load(open(sys.argv[1], encoding="utf-8"))
assert run["ok"], run
assert run["session"] == sys.argv[2], f"recorded against {run['session']}, not {sys.argv[2]}"
assert run["turn"], run
assert run["estimate_before_send"], "the estimate must be printed before the request"
assert run["estimate"]["input_tokens"] > 0, run["estimate"]
commands = [i for i in run["items"] if i["kind"] == "command"]
assert commands, f"no command item: {run['items']}"
assert commands[0]["body"]["command"] == "rigel doctor --json", commands
assert commands[0]["body"]["exit_code"] == 0, commands
assert run["recorded_usage"]["input_tokens"] == 420, run["recorded_usage"]
print("   command exit 0; recorded 420 input tokens; estimate printed before send")
PY

echo "== 3: Cursor and Cline run through the same client, each with its own key =="
export CURSOR_API_KEY="ck-rigel-exit-test-not-real"
export CLINE_API_KEY="sk-rigel-exit-test-not-real"
FAKE_ACP_MODE=doctor "$cli" agent acp --project "$project" --session "$session" --harness cursor \
  --exe "$acp_fake" --prompt "run rigel doctor --json" --approval allow --json \
  > "$logs/cursor.json" 2> "$logs/cursor.err" &
cursor_pid=$!
FAKE_ACP_MODE=doctor "$cli" agent acp --project "$project" --session "$session" --harness cline \
  --exe "$acp_fake" --prompt "run rigel doctor --json" --approval allow --json \
  > "$logs/cline.json" 2> "$logs/cline.err" &
cline_pid=$!

echo "== 4: a fourth turn is killed mid-flight =="
FAKE_ACP_MODE=wait-cancel "$cli" agent acp --project "$project" --session "$session" \
  --harness cursor --agent hung --exe "$acp_fake" --prompt "hang" \
  --approval allow --timeout-secs 60 --json \
  > "$logs/hung.json" 2> "$logs/hung.err" &
hung_pid=$!
# Wait for the turn to be on disk rather than a fixed sleep: on a loaded machine the CLI can still
# be starting three seconds in, and killing it then leaves `resume` with nothing to close.
for _ in $(seq 1 120); do
  python - "$project/.rigel/rigel.db" "$session" <<'PY' && break
import sqlite3, sys
db, session = sys.argv[1], sys.argv[2]
rows = sqlite3.connect(db).execute(
    "SELECT COUNT(*) FROM turns t JOIN agents a ON a.id = t.agent_id "
    "WHERE a.session_id = ? AND t.status = 'running'",
    (session,),
).fetchone()[0]
raise SystemExit(0 if rows else 1)
PY
  sleep 0.25
done
kill -9 "$hung_pid" 2>/dev/null || true
set +e
wait "$hung_pid" 2>/dev/null
hung_code=$?
set -e
[ "$hung_code" -ne 0 ] || { echo "the hung turn was expected to die"; exit 1; }

wait "$cursor_pid" || true
wait "$cline_pid" || true
for name in cursor cline; do
  python - "$logs/$name.json" "$logs/$name.err" <<'PY'
import json, sys
run = json.load(open(sys.argv[1], encoding="utf-8"))
err = open(sys.argv[2], encoding="utf-8", errors="replace").read()
assert run["ok"], f"{run} | {err}"
assert run["status"] == "completed", run
assert run["session"], run
print(f"   {run['harness']} turn {run['turn'][:8]} completed")
PY
done

echo "== 5: resume closes only the killed turn =="
"$cli" session resume --project "$project" --id "$session" --force --json > "$logs/resume.json"
python - "$logs/resume.json" <<'PY'
import json, sys
out = json.load(open(sys.argv[1], encoding="utf-8"))
closed = out["closed_turns"]
assert len(closed) == 1, f"expected exactly the killed turn, got {closed}"
assert closed[0]["stop_reason"] == "interrupted", closed[0]
print(f"   closed {closed[0]['id'][:8]} as interrupted")
PY

echo "== 6: every harness left a command item in the transcript =="
python - "$project/.rigel/rigel.db" "$session" <<'PY'
import json, sqlite3, sys
db, session = sys.argv[1], sys.argv[2]
rows = sqlite3.connect(db).execute(
    """
    SELECT a.harness, t.status, COUNT(i.id),
           COALESCE(SUM(CASE WHEN i.body_json LIKE '%rigel doctor --json%' THEN 1 ELSE 0 END), 0)
    FROM turns t
    JOIN agents a ON a.id = t.agent_id
    LEFT JOIN items i ON i.turn_id = t.id AND i.kind = 'command'
    WHERE a.session_id = ?
    GROUP BY t.id
    ORDER BY a.harness
    """,
    (session,),
).fetchall()
by_harness: dict[str, list[tuple[str, int, int]]] = {}
for harness, status, count, doctor in rows:
    by_harness.setdefault(harness, []).append((status, count, doctor))
for harness in ("codex", "cursor", "cline"):
    assert harness in by_harness, f"{harness} ran no turn: {by_harness}"
    turns = by_harness[harness]
    # Cursor ran twice in this session: the completed turn and the one killed on purpose.
    assert any(status == "completed" for status, _, _ in turns), f"{harness} did not complete: {turns}"
    assert any(count >= 1 for _, count, _ in turns), f"{harness} recorded no command item: {turns}"
    assert any(doctor >= 1 for _, _, doctor in turns), (
        f"{harness} did not record the doctor command in any command item: {turns}"
    )
print(f"   {by_harness}")
PY

echo
echo "P4 exit test: ok"

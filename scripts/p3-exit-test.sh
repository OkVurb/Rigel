#!/usr/bin/env bash
#
# Phase 3 exit test (P3.11).
#
# One throwaway git repository, one $0.25 session:
#   1. a scripted turn reads three files and journals a patch, then "dies" exactly after the
#      write-ahead journal (exit 70) — the same on-disk state a kill leaves: turn running,
#      patch proposed, file untouched, no checkpoint;
#   2. a new process resumes: the running turn closes as cancelled, the patch is reported
#      pending, and nothing is applied;
#   3. the patch is applied once; applying it again is refused and the diff is byte-identical;
#   4. the session continues, and a clean resume succeeds;
#   5. a second session proves the cap: a $0.31 turn stops after its first step and the next
#      turn is refused before it starts.
#
# Usage:
#   scripts/p3-exit-test.sh                 # builds target/debug/rigel if needed
#   RIGEL_BIN=C:/rigel-target/debug/rigel.exe scripts/p3-exit-test.sh
#
# Exit 0 means every assertion passed. Any failed assertion prints FAIL and exits 1.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixtures="$root/scripts/fixtures"
PY="${PYTHON:-python}"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

step() {
  printf '\n== %s\n' "$*"
}

jsonq() {
  # jsonq FILE 'expression over d'
  "$PY" - "$1" "$2" <<'PY' || return 1
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)
value = eval(sys.argv[2], {"__builtins__": {}}, {"d": data, "len": len, "min": min, "max": max, "abs": abs, "sorted": sorted})
print(value if isinstance(value, str) else json.dumps(value))
PY
}

expect_json() {
  local got
  got="$(jsonq "$1" "$2")" || fail "could not read $2 from $1"
  [ "$got" = "$3" ] || fail "$(basename "$1"): $2 is $got, expected $3"
}

# ------------------------------- the binary --------------------------------

rigel="${RIGEL_BIN:-}"
if [ -z "$rigel" ]; then
  target="${CARGO_TARGET_DIR:-$root/target}"
  for candidate in "$target/debug/rigel.exe" "$target/debug/rigel"; do
    if [ -x "$candidate" ] || [ -f "$candidate" ]; then
      rigel="$candidate"
      break
    fi
  done
fi
if [ -z "$rigel" ]; then
  step "building rigel-cli"
  cargo build -p rigel-cli
  target="${CARGO_TARGET_DIR:-$root/target}"
  for candidate in "$target/debug/rigel.exe" "$target/debug/rigel"; do
    if [ -f "$candidate" ]; then
      rigel="$candidate"
      break
    fi
  done
fi
[ -n "$rigel" ] || fail "no rigel binary; set RIGEL_BIN"
echo "rigel: $rigel"

work="$(mktemp -d "${TMPDIR:-/tmp}/rigel-p3-XXXXXX")"
cleanup() {
  rm -rf "$work"
}
trap cleanup EXIT

repo="$work/repo"
mkdir -p "$repo/src"
printf '.rigel/\n' >"$repo/.gitignore"
cat >"$repo/README.md" <<'EOF'
# Demo
Three files, one patch.
EOF
cat >"$repo/src/lib.rs" <<'EOF'
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn sub(a: i32, b: i32) -> i32 {
    a - b
}
EOF
cat >"$repo/src/main.rs" <<'EOF'
fn main() {
    println!("{}", demo::add(2, 3));
}
EOF

git -C "$repo" init -q
git -C "$repo" config user.email "exit-test@rigel.local"
git -C "$repo" config user.name "Rigel exit test"
git -C "$repo" add .
git -C "$repo" commit -qm "initial tree"

cd "$repo"
echo "repo: $repo"

# --------------------------- 1. a killed turn ------------------------------

step "session start (cap \$0.25)"
sid="$("$rigel" session start --project "$repo" --title "P3 exit" --budget-cents 25)"
[ -n "$sid" ] || fail "session start printed no id"
"$rigel" session list --project "$repo" --json >"$work/sessions.json"
expect_json "$work/sessions.json" 'd[0]["budget_cents"]' '25'
expect_json "$work/sessions.json" 'd[0]["spend_micros"]' '0'
echo "session: $sid"

step "one turn: three reads, one journaled patch, killed after the journal"
rc=0
"$rigel" agent run \
  --project "$repo" \
  --session "$sid" \
  --prompt "Make sub overflow-safe. Read first, then patch only src/lib.rs." \
  --script "$fixtures/p3-turn.json" \
  --crash-after-journal \
  --json >"$work/crash.ndjson" 2>"$work/crash.err" || rc=$?
[ "$rc" -eq 70 ] || { cat "$work/crash.err" >&2; fail "crash turn exited $rc, expected 70"; }

tail -n 1 "$work/crash.ndjson" >"$work/crash.json"
expect_json "$work/crash.json" 'd["stop"]' 'crash_simulated'
expect_json "$work/crash.json" 'd["status"]' 'running'
expect_json "$work/crash.json" 'd["tool_calls"]' '4'
expect_json "$work/crash.json" 'd["steps"]' '2'
[ -z "$(git status --porcelain)" ] || fail "the killed turn changed the tree before resume"
echo "tree unchanged; turn left running"

"$rigel" patch list --project "$repo" --session "$sid" --json >"$work/patches-before.json"
expect_json "$work/patches-before.json" 'len(d)' '1'
expect_json "$work/patches-before.json" 'd[0]["status"]' 'proposed'
patch_id="$(jsonq "$work/patches-before.json" 'd[0]["id"]')"
echo "patch: $patch_id proposed"

# ------------------------- 2. relaunch and resume --------------------------

step "relaunch: resume reconciles, applies nothing"
rc=0
"$rigel" session resume --project "$repo" --id "$sid" --json >"$work/resume1.json" 2>"$work/resume1.err" || rc=$?
[ "$rc" -eq 2 ] || { cat "$work/resume1.err" >&2; fail "interrupted resume exited $rc, expected 2"; }
expect_json "$work/resume1.json" 'd["clean"]' 'false'
expect_json "$work/resume1.json" 'd["session"]["status"]' 'interrupted'
expect_json "$work/resume1.json" 'len(d["closed_turns"])' '1'
expect_json "$work/resume1.json" 'd["closed_turns"][0]["status"]' 'cancelled'
expect_json "$work/resume1.json" 'len(d["patches"])' '1'
expect_json "$work/resume1.json" 'd["patches"][0]["state"]' 'pending'
expect_json "$work/resume1.json" 'd["reconcile"]["ok"]' 'true'
[ -z "$(git status --porcelain)" ] || fail "resume applied a patch by itself"
echo "resume closed the turn; patch still pending"

# ------------------------ 3. apply once, never twice -----------------------

step "apply the pending patch exactly once"
"$rigel" patch apply --project "$repo" --id "$patch_id" --json >"$work/apply.json"
expect_json "$work/apply.json" 'd["status"]' 'applied'
grep -q "a.saturating_sub(b)" src/lib.rs || fail "the patch did not reach src/lib.rs"

git diff --no-color >"$work/diff1.patch"
[ -s "$work/diff1.patch" ] || fail "git diff is empty after applying the patch"
diff_hash_1="$("$PY" -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$work/diff1.patch")"
adds="$(grep -c '^+.*saturating_sub' "$work/diff1.patch" || true)"
[ "$adds" -eq 1 ] || fail "expected one added saturating_sub line, found $adds"

# Apply is idempotent on purpose: a replay sees `applied` and touches nothing. The test is
# the tree and the journal, not an error code.
"$rigel" patch apply --project "$repo" --id "$patch_id" --json >"$work/apply2.json"
expect_json "$work/apply2.json" 'd["status"]' 'applied'
git diff --no-color >"$work/diff2.patch"
diff_hash_2="$("$PY" -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$work/diff2.patch")"
[ "$diff_hash_1" = "$diff_hash_2" ] || fail "the second apply changed the diff"
echo "diff byte-identical: ${diff_hash_1:0:16}…"

"$rigel" patch list --project "$repo" --session "$sid" --json >"$work/patches-after.json"
expect_json "$work/patches-after.json" 'len(d)' '1'
expect_json "$work/patches-after.json" 'd[0]["status"]' 'applied'

# --------------------------- 4. continue cleanly ---------------------------

step "continue on the resumed tree, then resume clean"
"$rigel" session resume --project "$repo" --id "$sid" --force --json >"$work/resume-force.json"
expect_json "$work/resume-force.json" 'd["session"]["interventions"]' '1'

"$rigel" agent run \
  --project "$repo" \
  --session "$sid" \
  --prompt "Confirm you are still working on the same tree." \
  --script "$fixtures/p3-done.json" \
  --json >"$work/done.ndjson"
tail -n 1 "$work/done.ndjson" >"$work/done.json"
expect_json "$work/done.json" 'd["status"]' 'completed'
expect_json "$work/done.json" 'd["stop"]' 'completed'

"$rigel" session resume --project "$repo" --id "$sid" --json >"$work/resume2.json"
expect_json "$work/resume2.json" 'd["clean"]' 'true'
expect_json "$work/resume2.json" 'len(d["closed_turns"])' '0'

# ----------------------------- 5. the bill --------------------------------

step "the session bill: under the cap, one task, one intervention"
"$rigel" session list --project "$repo" --json >"$work/sessions-final.json"
expect_json "$work/sessions-final.json" 'd[0]["budget_cents"]' '25'
expect_json "$work/sessions-final.json" 'd[0]["tasks_done"]' '1'
expect_json "$work/sessions-final.json" 'd[0]["interventions"]' '1'
# Every step is billed as it lands — that is how the cap can stop mid-turn — so the killed
# turn keeps the $0.0013 its two steps cost and the completed turn adds $0.0002.
expect_json "$work/sessions-final.json" 'd[0]["spend_micros"]' '1500'
spend="$(jsonq "$work/sessions-final.json" 'd[0]["spend_cents"]')"
[ "$spend" -le 25 ] || fail "spend $spend¢ exceeds the 25¢ cap"
echo "spend: ${spend}¢ of 25¢"

# --------------------------- 6. the cap stops ----------------------------

step "a fresh session: a \$0.31 turn against a \$0.25 cap"
sid2="$("$rigel" session start --project "$repo" --title "P3 overbudget" --budget-cents 25)"
rc=0
"$rigel" agent run \
  --project "$repo" \
  --session "$sid2" \
  --prompt "Spend past the cap." \
  --script "$fixtures/p3-overbudget.json" \
  --json >"$work/over.ndjson" 2>"$work/over.err" || rc=$?
[ "$rc" -eq 3 ] || { cat "$work/over.err" >&2; fail "overbudget turn exited $rc, expected 3"; }
tail -n 1 "$work/over.ndjson" >"$work/over.json"
expect_json "$work/over.json" 'd["stop"]' 'budget_exceeded'
expect_json "$work/over.json" 'd["steps"]' '1'
expect_json "$work/over.json" 'd["session_spend_micros"]' '310000'

"$rigel" session list --project "$repo" --json >"$work/sessions-over.json"
"$PY" - "$work/sessions-over.json" "$sid2" <<'PY' || fail "the overbudget session was not charged"
import json
import sys

rows = json.load(open(sys.argv[1], encoding="utf-8"))
row = next(item for item in rows if item["id"] == sys.argv[2])
assert row["spend_micros"] == 310_000, row
assert row["spend_cents"] == 31, row
PY

rc=0
"$rigel" agent run \
  --project "$repo" \
  --session "$sid2" \
  --prompt "This must be refused." \
  --script "$fixtures/p3-done.json" \
  --json >"$work/refused.ndjson" 2>"$work/refused.err" || rc=$?
[ "$rc" -eq 3 ] || { cat "$work/refused.err" >&2; fail "refused turn exited $rc, expected 3"; }
grep -q "budget exhausted" "$work/refused.err" || fail "refused turn failed for the wrong reason"
refused_turns="$(jsonq "$work/sessions-over.json" 'len(d)')"
[ "$refused_turns" -ge 2 ] || fail "expected both sessions to be listed"
echo "next turn refused before it started"

step "PASS — Phase 3 exit test"

#!/usr/bin/env bash
# Phase 5 exit test (P5.8): two agents on one file, a conflict inside two seconds, and the
# product's one forbidden word nowhere in what it shows.
#
#   1. the ledger is advisory and keeps history      — claims tests
#   2. a write by another agent is a conflict row    — attribute tests
#   3. it surfaces inside two seconds                 — attribute::a_conflict_surfaces_within_two_seconds
#   4. an agent can ask and answer through MCP        — rigel-mcp claim tools
#   5. the CLI can list and release                   — a real project in a temp dir
#   6. nothing says "locked"                          — a word-boundary grep over every surface
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== 1–4: ledger, attribution, MCP =="
cargo test -p rigel-core --lib claims
cargo test -p rigel-core --lib attribute
cargo test -p rigel-mcp claim

echo "== 5: rigel claim list and release =="
cargo build -q -p rigel-cli
project="$(mktemp -d)"
trap 'rm -rf "$project"' EXIT
./target/debug/rigel migrate --project "$project" >/dev/null
list="$(./target/debug/rigel claim list --project "$project" --json)"
echo "$list"
case "$list" in
  *'"ok":true'*) ;;
  *) echo "claim list did not answer with ok" >&2; exit 1 ;;
esac
case "$list" in
  *'"claims":[]'*) ;;
  *) echo "claim list did not return an empty ledger" >&2; exit 1 ;;
esac
# Releasing a path nobody holds is a refusal, not a crash.
if ./target/debug/rigel claim release src/app.tsx --project "$project" 2>/dev/null; then
  echo "releasing an unheld path should fail" >&2
  exit 1
fi

echo "== 6: the word is nowhere =="
if grep -rnE '\blocked\b|\blocking\b' ui/src; then
  echo "a surface says 'locked'; claims are advisory (N1)" >&2
  exit 1
fi
if ./target/debug/rigel claim list --project "$project" | grep -qiE '\blocked\b'; then
  echo "the CLI says 'locked'" >&2
  exit 1
fi

echo
echo "P5 exit test: ok"

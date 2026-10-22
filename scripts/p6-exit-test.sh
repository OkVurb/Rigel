#!/usr/bin/env bash
# Phase 6 exit test (P6.8): a missing tool shows the line that asked for it, the install runs
# only after approval, the next shell sees the binary, and unattended mode queues instead.
#
# The "machine without ripgrep" is a temporary PATH holding a shim `winget` and nothing else.
# The shim creates `rg` when it is run, so a test that sees `rg` afterwards proves the install
# actually executed — and a test that does not see it proves the queue did not.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
exe="$root/target/debug/rigel"
[ -f "$exe.exe" ] && exe="$exe.exe"
if [ ! -f "$exe" ]; then
  echo "p6: build the CLI first (cargo build -p rigel-cli)" >&2
  exit 2
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export RIGEL_INDEX_DIR="$tmp/data"
shim="$tmp/bin"
mkdir -p "$shim" "$tmp/project"
log="$tmp/shim.log"
: > "$log"

# A package manager that "installs" ripgrep by dropping a working `rg` next to itself.
if command -v cmd >/dev/null 2>&1; then
  cat > "$shim/winget.cmd" <<'SHIM'
@echo off
echo ran>>"%RIGEL_SHIM_LOG%"
> "%~dp0rg.cmd" echo @echo off
>>"%~dp0rg.cmd" echo echo ripgrep 14.1.0
exit /b 0
SHIM
  cat > "$shim/rg.cmd" <<'RG'
@echo off
echo ripgrep 14.1.0
RG
  rm -f "$shim/rg.cmd"   # start without it
  sys="/c/Windows/System32:/c/Windows"
else
  cat > "$shim/winget" <<'SHIM'
#!/bin/sh
echo ran >> "$RIGEL_SHIM_LOG"
printf '#!/bin/sh\necho ripgrep 14.1.0\n' > "$(dirname "$0")/rg"
chmod +x "$(dirname "$0")/rg"
SHIM
  chmod +x "$shim/winget"
  sys="/usr/bin:/bin"
fi
export RIGEL_SHIM_LOG="$log"
# Only Rigel sees the reduced PATH; the script itself still needs cat, grep and rm.
rpath="$shim:$sys"
rigel_run() { env PATH="$rpath" RIGEL_INDEX_DIR="$RIGEL_INDEX_DIR" "$exe" "$@"; }

# A skill whose body says what it needs, in a fence, on a line we can point at.
cat > "$tmp/project/SKILL.md" <<'SKILL'
---
name: audit-tree
description: Find TODOs across a repository.
allowed-tools: Read
---
# Audit

Run the search:

```bash
rg -n "TODO" src/
```
SKILL

fail=0
check() {
  if [ "$2" = "$3" ]; then
    echo "  ok    $1"
  else
    echo "  FAIL  $1 (expected $3, got $2)" >&2
    fail=1
  fi
}

json() { rigel_run "$@" 2>/dev/null | tail -1; }

echo "p6: detection"
check "ripgrep starts missing" "$(json tools which ripgrep --host windows --json | grep -c '"present":false' || true)" "1"

echo "p6: requirement inference"
req="$(json tools requirements --file "$tmp/project/SKILL.md" --json)"
check "the requirement is ripgrep" "$(printf '%s' "$req" | grep -c '"tool":"ripgrep"' || true)" "1"
check "it names line 11" "$(printf '%s' "$req" | grep -c '"line":11' || true)" "1"
check "it quotes the command" "$(printf '%s' "$req" | grep -c 'rg -n' || true)" "1"

echo "p6: the trust arithmetic is printed"
trust="$(json trust explain ripgrep --host windows --json)"
check "the score is trusted" "$(printf '%s' "$trust" | grep -c '"band":"trusted"' || true)" "1"
check "five factors are shown" "$(printf '%s' "$trust" | grep -o '"key":' | wc -l | tr -d ' ')" "5"
plain="$(rigel_run trust explain ripgrep --host windows 2>/dev/null)"
check "the human output shows the multiplication" "$(printf '%s' "$plain" | grep -c '^  \(curated\|publisher\|manager\|adoption\|command\) ' || true)" "5"

echo "p6: unattended queues, and nothing runs"
queued="$(json tools install ripgrep --host windows --unattended --json)"
check "the decision is queued" "$(printf '%s' "$queued" | grep -c '"decision":"queued"' || true)" "1"
check "the shim was not run" "$(wc -l < "$log" | tr -d ' ')" "0"
check "ripgrep is still missing" "$(json tools which ripgrep --host windows --json | grep -c '"present":false' || true)" "1"

echo "p6: approval runs the shown command"
installed="$(json tools install ripgrep --host windows --yes --json)"
check "the install succeeded" "$(printf '%s' "$installed" | grep -c '"exit_code":0' || true)" "1"
check "the shim ran once" "$(wc -l < "$log" | tr -d ' ')" "1"
check "the exit code is in the ledger" "$(json tools ledger --json | grep -c '"exit_code":0' || true)" "1"
check "the duration is in the ledger" "$(json tools ledger --json | grep -c '"duration_ms":' || true)" "1"
check "an approval is recorded as such" \
  "$(json tools ledger --json | grep -c '"decision":"user-approved"' || true)" "1"

echo "p6: the next shell sees it"
check "ripgrep is present now" "$(json tools which ripgrep --host windows --json | grep -c '"present":true' || true)" "1"
check "with the version the shim reports" \
  "$(json tools which ripgrep --host windows --json | grep -c 'ripgrep 14.1.0' || true)" "1"
check "and a hash to pin" "$(json tools which ripgrep --host windows --json | grep -c '"sha256":"' || true)" "1"

echo "p6: a refusal is a row and nothing more"
printf 'n\n' | "$exe" tools install jq --host windows >/dev/null 2>&1 || true
check "the denial is recorded" "$(json tools ledger --json | grep -c '"decision":"denied"' || true)" "1"
check "and jq is still absent" "$(json tools which jq --host windows --json | grep -c '"present":false' || true)" "1"

echo "p6: hashing the binary it installed"
check "verify pins the installed bytes" \
  "$(rigel_run tools verify ripgrep --host windows --json 2>/dev/null | grep -c '"state":"pinned"' || true)" "1"

if [ "$fail" -ne 0 ]; then
  echo "p6 failed" >&2
  exit 1
fi
echo "p6 passed"

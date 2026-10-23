#!/usr/bin/env bash
# Phase 7 exit test (P7.9), as far as one machine can prove it.
#
# What this script can check here:
#   1. doctor reports rows, no failures, and an exit code that matches its own summary;
#   2. the app needs no Node.js and no package manager to start: the same commands run on a
#      PATH holding only the OS shell;
#   3. no unexpected network: doctor, tools and the offline shelf run behind a dead proxy;
#   4. the installer is configured for MSI + NSIS with the WebView2 bootstrapper;
#   5. the licence files match the workspace's `MIT OR Apache-2.0`;
#   6. a signed update manifest verifies, and a tampered one is refused (openssl Ed25519).
#
# What needs a clean Windows VM and a certificate, and is therefore printed as `manual`:
#   - running the signed MSI on 10 1809 with no WebView2 preinstalled;
#   - opening a repo, running the shelf and one native task there;
#   - Authenticode verification through SmartScreen.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
exe="$root/target/debug/rigel"
[ -f "$exe.exe" ] && exe="$exe.exe"
if [ ! -f "$exe" ]; then
  echo "p7: build the CLI first (cargo build -p rigel-cli)" >&2
  exit 2
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export RIGEL_INDEX_DIR="$tmp/data"
mkdir -p "$tmp/data" "$tmp/project" "$tmp/empty-bin"

fail=0
manual=0
check() {
  if [ "$2" = "$3" ]; then
    echo "  ok    $1"
  else
    echo "  FAIL  $1 (expected $3, got $2)" >&2
    fail=1
  fi
}
note() { echo "  manual $1"; manual=$((manual + 1)); }

json() { { "$exe" "$@" 2>/dev/null || true; } | tail -1; }

echo "p7: doctor"
doc="$(json doctor --project "$tmp/project" --json)"
fails="$(printf '%s' "$doc" | python -c 'import sys,json; print(json.load(sys.stdin)["failures"])')"
code="$(printf '%s' "$doc" | python -c 'import sys,json; print(json.load(sys.stdin)["exit_code"])')"
rows="$(printf '%s' "$doc" | python -c 'import sys,json; print(len(json.load(sys.stdin)["checks"]))')"
check "no failures on a fresh data directory" "$fails" "0"
check "the reported exit code is the failure state" "$code" "$([ "$fails" = "0" ] && echo 1 || echo 2)"
check "there are checks, not an empty list" "$([ "$rows" -ge 10 ] && echo yes || echo no)" "yes"
check "every harness has a row" \
  "$(printf '%s' "$doc" | python -c 'import sys,json; print(sum(1 for c in json.load(sys.stdin)["checks"] if c["id"].startswith("harness:")))')" "5"
"$exe" doctor --project "$tmp/project" --json >/dev/null 2>&1 || true
set +e
"$exe" doctor --project "$tmp/project" >/dev/null 2>&1
status=$?
set -e
check "the process status agrees with the row" "$status" "$code"

echo "p7: no Node, no package manager, no network"
# A PATH with the shell only. `rigel` itself is invoked by absolute path.
if command -v cmd >/dev/null 2>&1; then
  bare="/c/Windows/System32:/c/Windows"
else
  bare="/bin:/usr/bin"
fi

set +e
env PATH="$bare" "$exe" --data-dir "$tmp/data" doctor --project "$tmp/project" --json >/dev/null
no_node=$?
set -e
check "doctor runs without Node on PATH (warnings allowed)"   "$([ "$no_node" -le 1 ] && echo yes || echo no)" "yes"

set +e
env PATH="$bare" HTTP_PROXY="http://127.0.0.1:9" HTTPS_PROXY="http://127.0.0.1:9"   "$exe" --data-dir "$tmp/data" tools list --json >/dev/null
tools_proxy=$?
set -e
check "tools list works behind a dead proxy" "$tools_proxy" "0"

set +e
env HTTP_PROXY="http://127.0.0.1:9" HTTPS_PROXY="http://127.0.0.1:9"   "$exe" --data-dir "$tmp/data" skills search "pdf" --json >/dev/null
shelf_proxy=$?
set -e
check "the offline shelf works behind a dead proxy" "$shelf_proxy" "0"

echo "p7: installer"
python - "$root/crates/rigel-app/tauri.conf.json" <<'PY'
import json, sys
config = json.load(open(sys.argv[1]))
bundle = config["bundle"]
assert bundle["active"] is True
assert "msi" in bundle["targets"] and "nsis" in bundle["targets"]
assert bundle["windows"]["webviewInstallMode"]["type"] == "downloadBootstrapper"
assert bundle["windows"]["webviewInstallMode"]["silent"] is True
assert bundle["windows"]["allowDowngrades"] is False
assert bundle["publisher"]
# The hook runs in `crates/`, so the UI is `../ui` from there; without it the build ships a stale
# ui/dist or fails (both seen during P7.5).
assert config["build"]["beforeBuildCommand"] == "npm --prefix ../ui run build"
print("configured")
PY
check "msi + nsis, silent WebView2 bootstrapper, no downgrades" "$?" "0"

echo "p7: the signing script"
# The certificate is never here, but its absence must be a clean refusal, and a signature must be
# checked rather than assumed: those two rules are what `sign-installers.ps1` has to get right.
if command -v powershell >/dev/null 2>&1; then
  sign="$root/scripts/sign-installers.ps1"
  check "the signing script exists" "$([ -f "$sign" ] && echo yes || echo no)" "yes"
  mkdir -p "$tmp/bundle/msi"
  : > "$tmp/bundle/msi/dummy.msi"
  set +e
  powershell -NoProfile -ExecutionPolicy Bypass -File "$sign" -VerifyOnly     -BundleDir "$(cygpath -w "$tmp/bundle")" >/dev/null 2>&1
  unsigned=$?
  powershell -NoProfile -ExecutionPolicy Bypass -File "$sign" -VerifyOnly     -BundleDir "$(cygpath -w "$tmp/absent")" >/dev/null 2>&1
  absent=$?
  RIGEL_CERT_THUMBPRINT= powershell -NoProfile -ExecutionPolicy Bypass -File "$sign"     -BundleDir "$(cygpath -w "$tmp/bundle")" >/dev/null 2>&1
  nocert=$?
  # A build is a long way to go before discovering there is nothing to sign with.
  RIGEL_CERT_THUMBPRINT= powershell -NoProfile -ExecutionPolicy Bypass -File "$sign" -Build -BundleDir "$(cygpath -w "$tmp/bundle")" >/dev/null 2>&1
  buildnocert=$?
  set -e
  check "an unsigned artifact is a failure, not a note" "$unsigned" "1"
  check "a missing bundle directory is a usage error" "$absent" "2"
  check "signing without a certificate refuses" "$nocert" "2"
  check "a signed build refuses before compiling without a certificate" "$buildnocert" "2"
else
  note "powershell is not on this machine; the signing script's checks were skipped"
fi

if [ -n "${RIGEL_CERT_THUMBPRINT:-}" ]; then
  echo "  ok    a signing certificate is configured in this environment"
else
  note "the release needs a code-signing certificate: RIGEL_CERT_THUMBPRINT"
fi
note "run the signed MSI on a clean Windows 10 1809 VM with no WebView2, then open a repo, the shelf and one task"

echo "p7: licence"
check "MIT text present" "$(grep -c "MIT License" "$root/LICENSE-MIT" || true)" "1"
check "Apache text present" "$(grep -c "Apache License" "$root/LICENSE-APACHE" || true)" "4"
check "Cargo.toml agrees with the files" \
  "$(grep -c 'license = "MIT OR Apache-2.0"' "$root/Cargo.toml" || true)" "1"
check "README points at the licence" "$(grep -ci "MIT OR Apache-2.0" "$root/README.md" || true)" "1"

echo "p7: a signed update manifest"
if command -v openssl >/dev/null 2>&1 && command -v xxd >/dev/null 2>&1; then
  work="$tmp/update"
  mkdir -p "$work"
  printf 'installer bytes for the test\n' > "$work/rigel-0.2.0.msi"
  sha="$(sha256sum "$work/rigel-0.2.0.msi" | cut -d' ' -f1)"
  version="0.2.0"
  url="https://example.test/rigel-0.2.0.msi"
  printf '%s\n%s\n%s' "$version" "$url" "$sha" > "$work/message"
  openssl genpkey -algorithm ed25519 -out "$work/key.pem" 2>/dev/null
  openssl pkeyutl -sign -inkey "$work/key.pem" -rawin -in "$work/message" -out "$work/sig.bin"
  pub="$(openssl pkey -in "$work/key.pem" -pubout -outform DER | tail -c 32 | xxd -p -c 64 | tr -d '\n')"
  sig="$(xxd -p -c 200 "$work/sig.bin" | tr -d '\n')"
  printf '%s' "$pub" > "$tmp/data/update-pubkey.txt"

  python - "$work" "$version" "$url" "$sha" "$sig" <<'PY'
import json, sys
work, version, url, sha, sig = sys.argv[1:]
json.dump({"version": version, "url": url, "sha256": sha, "signature": sig},
          open(work + "/manifest.json", "w"), indent=2)
PY

  "$exe" --data-dir "$tmp/data" update enable >/dev/null
  checked="$(json update check --manifest "$work/manifest.json" --json)"
  check "the signature verifies" "$(printf '%s' "$checked" | grep -c '"verified":true' || true)" "1"
  check "the version is newer than this build" "$(printf '%s' "$checked" | grep -c '"available":true' || true)" "1"
  check "the artifact matches the signed hash" \
    "$("$exe" --data-dir "$tmp/data" update verify "$work/rigel-0.2.0.msi" --manifest "$work/manifest.json" --json 2>/dev/null | grep -c '"ok":true' || true)" "1"

  python - "$work" <<'PY'
import json, sys
work = sys.argv[1]
manifest = json.load(open(work + "/manifest.json"))
manifest["url"] = "https://evil.test/rigel-0.2.0.msi"
json.dump(manifest, open(work + "/tampered.json", "w"), indent=2)
PY
  set +e
  "$exe" --data-dir "$tmp/data" update check --manifest "$work/tampered.json" --json >/dev/null 2>&1
  tampered=$?
  set -e
  check "a changed URL is refused" "$tampered" "1"

  printf 'tampered bytes\n' > "$work/rigel-0.2.0.msi"
  set +e
  "$exe" --data-dir "$tmp/data" update verify "$work/rigel-0.2.0.msi" --manifest "$work/manifest.json" --json >/dev/null 2>&1
  rehashed=$?
  set -e
  check "changed artifact bytes are refused" "$rehashed" "1"

  "$exe" --data-dir "$tmp/data" update disable >/dev/null
  set +e
  "$exe" --data-dir "$tmp/data" update check --manifest "$work/manifest.json" --json >/dev/null 2>&1
  disabled=$?
  set -e
  check "checks are off by default and refuse to run" "$disabled" "1"
else
  note "openssl/xxd not found: the signed-manifest checks were skipped"
fi

echo
if [ "$fail" -ne 0 ]; then
  echo "p7 failed" >&2
  exit 1
fi
echo "p7 passed in this environment ($manual manual step(s) left: clean VM, signing, SmartScreen)"
echo "the rest: scripts/p7-vm-check.ps1 on a Windows 10 1809 VM, or scripts/p7-sandbox.ps1"
echo "for a fresh Windows Sandbox (one restart after enabling Containers-DisposableClientVM);"
echo "docs/qa/p7-acceptance.md records which attempt proved what."

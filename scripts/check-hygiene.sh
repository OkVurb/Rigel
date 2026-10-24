#!/usr/bin/env bash
# ACTIVE-WORK.md and tmp/ are local-only. .gitignore stops an ordinary
# `git add`, but `git add -f` overrides it silently, so this looks at the paths
# git actually has rather than trusting the ignore rules. CI and the pre-commit
# hook both run this script so the two checks cannot drift.
set -euo pipefail

offending=$(git ls-files | grep -E '^(ACTIVE-WORK\.md|tmp/)' || true)

if [ -z "$offending" ]; then
  echo "ok: no ACTIVE-WORK.md or tmp/ path is tracked"
  exit 0
fi

if [ -n "${GITHUB_ACTIONS:-}" ]; then
  echo "::error::These paths are local-only and must not be committed:"
  printf '%s\n' "$offending"
else
  echo "error: these paths are local-only and must not be committed:" >&2
  printf '%s\n' "$offending" >&2
  echo "hint: git rm --cached <path> unstages it; the file stays on disk." >&2
fi

exit 1

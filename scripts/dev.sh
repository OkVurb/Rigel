#!/usr/bin/env bash
#
# Local development (P4 dev loop): start the desktop shell with the dev environment's secrets
# already in the process environment.
#
# The app reads provider keys and paths from the environment (`std::env::var`), so Infisical
# injects exactly what a local .env used to supply — no application code changes, and no .env
# file needed in the repository. The project id comes from .infisical.json, written by
# `infisical init`. See INFISICAL.md.
#
# usage:
#   bash scripts/dev.sh
#
# The script always reads Development (`--env=dev`), which is where local keys belong. Another
# environment is a deliberate, explicit act: `infisical run --env=staging -- cargo run -p rigel-app`.
#
# Production entry points are deliberately not wrapped: they need a machine identity with
# Universal Auth, not the interactive dev CLI.

set -euo pipefail

cd "$(dirname "$0")/.."

# A fresh clone has no hook enabled. Point git at the committed .githooks so the hygiene check
# runs before every commit (see CONTRIBUTING.md). An existing value is left alone: another hook
# manager may own it. Requiring this directory's own .git keeps a tarball unpacked inside some
# other repository from rewriting that repository's config. This comes before the Infisical
# check because it has nothing to do with secrets: a developer who has not installed the CLI
# yet still gets the hook.
if command -v git >/dev/null 2>&1 && [ -e .git ]; then
  hooks_path=$(git config --get core.hooksPath || true)
  if [ -z "$hooks_path" ]; then
    if git config core.hooksPath .githooks; then
      echo "dev: pre-commit hook enabled (core.hooksPath=.githooks)"
    else
      echo "dev: could not enable the pre-commit hook; commits will skip the hygiene check" >&2
    fi
  elif [ "$hooks_path" != ".githooks" ]; then
    echo "dev: core.hooksPath is '$hooks_path', so the hygiene hook will not run" >&2
  fi
fi

if ! command -v infisical >/dev/null 2>&1; then
  echo "dev: the Infisical CLI is not on PATH." >&2
  echo "  install it once: https://infisical.com/docs/cli/overview" >&2
  echo "  then: infisical login && infisical init (in this folder)" >&2
  echo "  the steps are in INFISICAL.md, under Finish setup" >&2
  exit 127
fi

exec infisical run --env=dev -- cargo run -p rigel-app

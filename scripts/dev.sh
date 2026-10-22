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

if ! command -v infisical >/dev/null 2>&1; then
  echo "dev: the Infisical CLI is not on PATH." >&2
  echo "  install it once: https://infisical.com/docs/cli/overview" >&2
  echo "  then: infisical login && infisical init (in this folder)" >&2
  echo "  the steps are in INFISICAL.md, under Finish setup" >&2
  exit 127
fi

cd "$(dirname "$0")/.."

exec infisical run --env=dev -- cargo run -p rigel-app

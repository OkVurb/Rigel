#!/usr/bin/env bash
#
# Refresh the local Rigel skill index from every repository in a source list.
#
# The index is one SQLite file shared by the app and the CLI (`rigel skills stats`
# shows its size; on Windows it is %APPDATA%\Rigel\index.db). Bodies sit next to it
# as content-addressed blobs, so a refresh downloads only what changed; `--force`
# revalidates every cached URL instead of trusting the ten-minute freshness window.
#
# usage:
#   scripts/refresh-skills.sh
#   scripts/refresh-skills.sh --limit 100 --sweep-pages 2
#   scripts/refresh-skills.sh --data-dir /tmp/rigel-index --dry-run
#   RIGEL_GITHUB_TOKEN=github_pat_... scripts/refresh-skills.sh
#
# GitHub allows 60 API calls an hour without a token, and a repository costs two
# (metadata and one tree call). The script stops at the first rate-limit error
# rather than logging a 403 for every repository that is left.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
LIST="$ROOT/scripts/skills-sources.txt"
LIMIT=300
SWEEP_PAGES=0
DRY_RUN=0
DATA_DIR=""

usage() {
    cat <<'TEXT'
usage: scripts/refresh-skills.sh [--list FILE] [--limit N] [--data-dir DIR]
                                 [--sweep-pages N] [--dry-run]

Re-index every repository in scripts/skills-sources.txt (or --list FILE) into
Rigel's local skill index, then print its size. Set RIGEL_BIN to choose the
rigel executable; RIGEL_GITHUB_TOKEN raises the GitHub API quota.
TEXT
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --list)
            [ "$#" -ge 2 ] || { echo "refresh-skills: --list needs a file" >&2; exit 2; }
            LIST="$2"
            shift 2
            ;;
        --limit)
            [ "$#" -ge 2 ] || { echo "refresh-skills: --limit needs a number" >&2; exit 2; }
            LIMIT="$2"
            shift 2
            ;;
        --sweep-pages)
            [ "$#" -ge 2 ] || { echo "refresh-skills: --sweep-pages needs a number" >&2; exit 2; }
            SWEEP_PAGES="$2"
            shift 2
            ;;
        --data-dir)
            [ "$#" -ge 2 ] || { echo "refresh-skills: --data-dir needs a path" >&2; exit 2; }
            DATA_DIR="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "refresh-skills: unknown argument $1 (try --help)" >&2
            exit 2
            ;;
    esac
done

case "$LIMIT" in
    ''|*[!0-9]*) echo "refresh-skills: --limit must be a positive integer" >&2; exit 2 ;;
esac
case "$SWEEP_PAGES" in
    ''|*[!0-9]*) echo "refresh-skills: --sweep-pages must be a non-negative integer" >&2; exit 2 ;;
esac

[ -f "$LIST" ] || { echo "refresh-skills: no source list at $LIST" >&2; exit 1; }

BIN="${RIGEL_BIN:-}"
if [ -z "$BIN" ]; then
    for candidate in \
        "$ROOT/target/release/rigel.exe" \
        "$ROOT/target/release/rigel" \
        "$ROOT/target/debug/rigel.exe" \
        "$ROOT/target/debug/rigel"; do
        if [ -x "$candidate" ]; then
            BIN="$candidate"
            break
        fi
    done
fi
[ -n "$BIN" ] || BIN="rigel"
if ! command -v "$BIN" >/dev/null 2>&1; then
    echo "refresh-skills: no rigel binary; build one with 'cargo build -p rigel-cli'" >&2
    echo "  or set RIGEL_BIN to point at it" >&2
    exit 1
fi

DATA_ARGS=()
if [ -n "$DATA_DIR" ]; then
    DATA_ARGS=(--data-dir "$DATA_DIR")
fi

echo "list   $LIST"
echo "binary $BIN"
echo "limit  $LIMIT per repository"
echo

repos=0
failed=0
while IFS= read -r line || [ -n "$line" ]; do
    repo=${line%%#*}
    repo=$(printf '%s' "$repo" | tr -d '\r' | xargs)
    [ -n "$repo" ] || continue
    repos=$((repos + 1))
    echo "-- $repo"
    if [ "$DRY_RUN" -eq 1 ]; then
        printf '   %q' "$BIN"
        for arg in "${DATA_ARGS[@]}"; do
            printf ' %q' "$arg"
        done
        printf ' skills index %q --force --limit %s --json\n' "$repo" "$LIMIT"
        continue
    fi
    if ! out=$("$BIN" "${DATA_ARGS[@]}" skills index "$repo" --force --limit "$LIMIT" --json 2>&1); then
        printf '%s\n' "$out"
        case "$out" in
            *"rate limited"*|*"403"*|*"API rate"*)
                echo >&2
                echo "refresh-skills: GitHub's API quota ran out after $repos repositories." >&2
                echo "  set RIGEL_GITHUB_TOKEN (or GITHUB_TOKEN) and run again, or wait for" >&2
                echo "  the hourly window to reset. Everything indexed so far is kept." >&2
                exit 1
                ;;
        esac
        echo "refresh-skills: skipped $repo; what an earlier run indexed is kept" >&2
        failed=$((failed + 1))
        continue
    fi
    printf '%s\n' "$out"
done < "$LIST"

if [ "$repos" -eq 0 ]; then
    echo "refresh-skills: $LIST lists no repositories" >&2
    exit 1
fi

if [ "$DRY_RUN" -eq 0 ] && [ "$SWEEP_PAGES" -gt 0 ]; then
    echo
    echo "-- marketplace metadata (skillhub $SWEEP_PAGES page(s), skillsmp 1 page)"
    "$BIN" "${DATA_ARGS[@]}" skills sweep \
        --skillhub-pages "$SWEEP_PAGES" \
        --skillsmp-pages 1 || true
fi

if [ "$DRY_RUN" -eq 0 ]; then
    echo
    echo "-- index"
    "$BIN" "${DATA_ARGS[@]}" skills stats
fi

if [ "$failed" -gt 0 ]; then
    echo "refresh-skills: $failed of $repos repositories failed; the rest are current" >&2
    exit 1
fi

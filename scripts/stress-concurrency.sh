#!/usr/bin/env bash
#
# Repeat the tests that hit one database from several threads, N times.
#
# These tests catch failures that are timing-dependent by nature: a second
# connection losing the journal-mode race, a deferred transaction whose read
# snapshot is invalidated, a budget row double-counted by two writers. Any one
# of them can pass a hundred runs and then fail the one after the change that
# reintroduced the race, so a single green `cargo test` is not evidence about
# them. Repetition is: 25 runs per push is cheap, and a failure names the
# iteration and the test.
#
# usage:
#   scripts/stress-concurrency.sh                 # 25 runs of every test below
#   RIGEL_STRESS_RUNS=200 scripts/stress-concurrency.sh
#
# exit status is 0 only when every run of every test passed, and 1 when a test
# failed or was renamed out from under this list (a filter that matches nothing
# exits 0, which would otherwise look like a pass).

set -euo pipefail

RUNS="${RIGEL_STRESS_RUNS:-25}"

say() { printf '%s\n' "$*"; }
die() { printf 'stress: %s\n' "$*" >&2; exit 1; }

case "$RUNS" in
    ''|*[!0-9]*) die "RIGEL_STRESS_RUNS must be a positive integer, got '$RUNS'" ;;
esac
[ "$RUNS" -ge 1 ] || die "RIGEL_STRESS_RUNS must be at least 1"

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$ROOT"
[ -f Cargo.toml ] || die "no Cargo.toml at $ROOT"

command -v cargo >/dev/null 2>&1 \
    || die "cargo is not on PATH"

# A thread checkout sits about 160 characters deep, and the MSVC linker refuses
# to create a file past MAX_PATH even when Windows allows long paths. Build
# somewhere short when the default target directory would overflow.
if [ -z "${CARGO_TARGET_DIR:-}" ] && [ "${#ROOT}" -gt 120 ]; then
    CARGO_TARGET_DIR="${TMPDIR:-/tmp}/rigel-stress-target"
    mkdir -p "$CARGO_TARGET_DIR"
    export CARGO_TARGET_DIR
fi

# binary|test name — only tests that share a database between threads belong
# here. A test that merely uses tokio does not.
TESTS="schema|concurrent_first_open_applies_each_migration_once
native|sibling_subagents_overlap_their_model_calls
native|stopping_a_parent_stops_the_children_it_spawned
native|concurrent_usage_is_counted_once_per_charge
guarantees|budget_stops_within_one_turn"

count=$(printf '%s\n' "$TESTS" | wc -l | tr -d ' ')

# Compile once, so a build error is reported once and the loop only runs tests.
say "stress  building the test binaries"
cargo test --no-run -p rigel-core --test schema --test native --test guarantees \
    || die "the test binaries did not build"

run=1
while [ "$run" -le "$RUNS" ]; do
    while IFS='|' read -r binary filter; do
        if ! output=$(cargo test -q -p rigel-core --test "$binary" "$filter" 2>&1); then
            printf 'stress: run %d/%d failed: %s %s\n' "$run" "$RUNS" "$binary" "$filter" >&2
            printf '%s\n' "$output" | tail -40 | sed 's/^/  /' >&2
            exit 1
        fi
        if ! printf '%s' "$output" | grep -q 'test result: ok\. [1-9]'; then
            printf 'stress: run %d/%d matched no test: %s %s\n' "$run" "$RUNS" "$binary" "$filter" >&2
            die "a test this list names no longer exists; update scripts/stress-concurrency.sh"
        fi
    done <<EOF
$TESTS
EOF
    say "stress  run $run/$RUNS ok"
    run=$((run + 1))
done

say "stress  $RUNS runs of $count concurrency tests passed"

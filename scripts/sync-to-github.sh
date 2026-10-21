#!/usr/bin/env bash
#
# Mirror this checkout into the GitHub clone, then push — without rewriting
# history on either side.
#
# Freebuff gives every thread a private clone under
# ~/.config/freebuff-desktop/local-projects/.../threads/<id>, with no remote.
# Commits made there never reach the folder GitHub Desktop watches, and the
# difference is invisible until someone goes looking. This script is the
# bridge: the checkout it lives in is the source of truth, the target clone is
# where the world looks, and the two end up on the same commit.
#
# It refuses to guess. A dirty source, a dirty target, a target branch that has
# local commits origin does not, or unrelated histories without an explicit
# flag all stop it before anything is moved. Before moving anything it runs the
# workspace's checks — fmt, clippy with warnings denied, and the test suite —
# so a broken thread cannot become a broken GitHub branch.
#
# usage:
#   scripts/sync-to-github.sh                 # merge into the target and push
#   scripts/sync-to-github.sh --dry-run       # say what it would do
#   scripts/sync-to-github.sh --no-push       # merge locally, push later
#   scripts/sync-to-github.sh --allow-unrelated
#
# options:
#   --target PATH        target clone (default $RIGEL_SYNC_TARGET or
#                        ~/Documents/GitHub/rigel)
#   --branch NAME        branch to sync (default: the source's current branch)
#   --no-push            stop after the merge, do not push
#   --no-ff-source       leave this checkout where it is (see below)
#   --allow-unrelated    permit a merge between histories with no common
#                        ancestor, taking the source's tree where they disagree
#   --dry-run            print the plan and exit; changes nothing
#   -h, --help           this text
#
# After a merge the source is fast-forwarded onto the new target commit, so the
# next run has nothing to reconcile. That is a fast-forward of a clean tree to
# an identical tree; --no-ff-source turns it off.

set -euo pipefail

TARGET_OPT=""
BRANCH_OPT=""
PUSH=1
DRY_RUN=0
ALLOW_UNRELATED=0
FF_SOURCE=1
RUN_CHECKS=1

usage() {
    cat <<'USAGE'
Mirror this checkout into the GitHub clone, then push, without rewriting
history on either side.

usage:
  scripts/sync-to-github.sh                 merge into the target and push
  scripts/sync-to-github.sh --dry-run       say what it would do
  scripts/sync-to-github.sh --no-push       merge locally, push later
  scripts/sync-to-github.sh --allow-unrelated

options:
  --target PATH        target clone (default $RIGEL_SYNC_TARGET or
                       ~/Documents/GitHub/rigel)
  --branch NAME        branch to sync (default: the source's current branch)
  --no-push            stop after the merge, do not push
  --no-checks          mirror without running fmt, clippy and the tests
  --no-ff-source       leave this checkout where it is
  --allow-unrelated    permit a merge between histories with no common
                       ancestor, taking the source's tree where they disagree
  --dry-run            print the plan and exit; changes nothing
  -h, --help           this text

When the source is a Cargo workspace, the checks run before anything is merged
or pushed: cargo fmt --check, cargo clippy -D warnings, cargo test. A failure
leaves both sides untouched. --no-checks is for mirroring a commit you have
already verified yourself.

After a merge the source is fast-forwarded onto the new target commit, so the
next run has nothing to reconcile.
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --target) TARGET_OPT="${2:-}"; shift 2 ;;
        --target=*) TARGET_OPT="${1#*=}"; shift ;;
        --branch) BRANCH_OPT="${2:-}"; shift 2 ;;
        --branch=*) BRANCH_OPT="${1#*=}"; shift ;;
        --no-push) PUSH=0; shift ;;
        --no-checks) RUN_CHECKS=0; shift ;;
        --no-ff-source) FF_SOURCE=0; shift ;;
        --allow-unrelated) ALLOW_UNRELATED=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) printf 'sync-to-github: unknown argument %s (try --help)\n' "$1" >&2; exit 2 ;;
    esac
done

say()  { printf '%s\n' "$*"; }
note() { printf '  %s\n' "$*"; }
die()  { printf 'sync-to-github: %s\n' "$*" >&2; exit 1; }

# Push only when the target is ahead of origin, so a --no-push run can be
# finished by the next one instead of blocking it.
push_if_ahead() {
    if [ "$PUSH" -eq 0 ]; then
        say "skipped push (--no-push)"
        return 0
    fi
    local ahead
    ahead=$(git -C "$TARGET" rev-list --count "origin/$BRANCH..$BRANCH")
    if [ "$ahead" -eq 0 ]; then
        say "origin/$BRANCH already has everything"
        return 0
    fi
    if ! git -C "$TARGET" push origin "$BRANCH" 2>&1 | sed 's/^/  /'; then
        die "push failed; the remote may have moved. Re-run to fetch and merge again"
    fi
    say "pushed $BRANCH ($ahead commit(s))"
}

# Move this checkout onto the commit the target ended on, when that is a plain
# fast-forward. Keeps the two lines identical, so the next run is a no-op.
ff_source_onto() {
    local onto="$1" src_head
    [ "$FF_SOURCE" -eq 1 ] && [ "$SOURCE_BRANCH" = "$BRANCH" ] || return 0
    src_head=$(git -C "$SOURCE_ROOT" rev-parse HEAD)
    [ "$src_head" != "$onto" ] || return 0
    git -C "$TARGET" merge-base --is-ancestor "$src_head" "$onto" || return 0
    git -C "$SOURCE_ROOT" fetch --quiet "$TARGET" "$BRANCH" \
        || die "could not fetch the target back"
    git -C "$SOURCE_ROOT" merge --quiet --ff-only FETCH_HEAD \
        || die "source could not fast-forward onto the synced commit"
    say "source fast-forwarded to $(git -C "$SOURCE_ROOT" rev-parse --short HEAD)"
}

# Run the workspace's own gates against the source. Nothing has moved yet, so a
# failure costs only the wait; the target and origin stay exactly as they were.
run_checks() {
    if [ "$RUN_CHECKS" -eq 0 ]; then
        say "checks  skipped (--no-checks)"
        return 0
    fi
    if [ ! -f "$SOURCE_ROOT/Cargo.toml" ]; then
        say "checks  none (no Cargo.toml at the source root)"
        return 0
    fi
    command -v cargo >/dev/null 2>&1 \
        || die "cargo is not on PATH, so the mirror would be unverified (--no-checks to override)"

    local name cmd output
    while IFS='|' read -r name cmd; do
        if output=$(cd "$SOURCE_ROOT" && eval "$cmd" 2>&1); then
            say "checks  $name ok"
        else
            printf '%s\n' "$output" | tail -40 | sed 's/^/  /' >&2
            die "$name failed in the source; nothing was merged or pushed"
        fi
    done <<'CHECKS'
fmt|cargo fmt --all -- --check
clippy|cargo clippy --workspace --all-targets -- -D warnings
test|cargo test --workspace
CHECKS
}

# --- where ----------------------------------------------------------------

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
SOURCE_ROOT=$(git -C "$script_dir/.." rev-parse --show-toplevel 2>/dev/null) \
    || die "scripts/ is not inside a git repository"
SOURCE_ROOT=$(cd -- "$SOURCE_ROOT" && pwd -P)

DEFAULT_TARGET="${RIGEL_SYNC_TARGET:-$HOME/Documents/GitHub/rigel}"
TARGET="${TARGET_OPT:-$DEFAULT_TARGET}"
[ -d "$TARGET" ] || die "target $TARGET does not exist (pass --target)"
TARGET=$(cd -- "$TARGET" && pwd -P)

[ "$TARGET" != "$SOURCE_ROOT" ] || die "source and target are the same checkout"

git -C "$TARGET" rev-parse --git-dir >/dev/null 2>&1 \
    || die "$TARGET is not a git repository"

SOURCE_BRANCH=$(git -C "$SOURCE_ROOT" symbolic-ref --quiet --short HEAD) \
    || die "source is on a detached HEAD; check out a branch first"
BRANCH="${BRANCH_OPT:-$SOURCE_BRANCH}"

# --- preconditions --------------------------------------------------------

source_dirty=$(git -C "$SOURCE_ROOT" status --porcelain)
if [ -n "$source_dirty" ]; then
    printf '%s\n' "$source_dirty" >&2
    die "source has uncommitted changes; commit them first (this script never sweeps for you)"
fi

target_branch=$(git -C "$TARGET" symbolic-ref --quiet --short HEAD 2>/dev/null) \
    || die "target is on a detached HEAD; check out $BRANCH there first"
[ "$target_branch" = "$BRANCH" ] \
    || die "target is on $target_branch, expected $BRANCH (pass --branch to override)"

target_dirty=$(git -C "$TARGET" status --porcelain)
if [ -n "$target_dirty" ]; then
    printf '%s\n' "$target_dirty" >&2
    die "target has uncommitted changes; this refuses to merge over them"
fi

git -C "$TARGET" remote get-url origin >/dev/null 2>&1 \
    || die "target has no origin remote"

say "source  $SOURCE_ROOT  ($SOURCE_BRANCH)"
say "target  $TARGET  ($BRANCH)"

git -C "$TARGET" fetch --quiet origin "$BRANCH" \
    || die "could not fetch origin/$BRANCH"
origin_head=$(git -C "$TARGET" rev-parse --verify --quiet "origin/$BRANCH") \
    || die "origin has no $BRANCH"

target_head=$(git -C "$TARGET" rev-parse HEAD)
if [ "$target_head" != "$origin_head" ]; then
    if git -C "$TARGET" merge-base --is-ancestor "$target_head" "$origin_head"; then
        say "fast-forwarding the target onto origin/$BRANCH first"
        [ "$DRY_RUN" -eq 1 ] || git -C "$TARGET" merge --quiet --ff-only "origin/$BRANCH"
        target_head=$origin_head
    elif git -C "$TARGET" merge-base --is-ancestor "$origin_head" "$target_head"; then
        say "target is $(git -C "$TARGET" rev-list --count "origin/$BRANCH..$target_head") commit(s) ahead of origin/$BRANCH; they will be pushed too"
    else
        die "target has diverged from origin/$BRANCH; settle that by hand before syncing"
    fi
fi

# --- what the target would gain ------------------------------------------

temp_remote="rigel-thread-sync"
cleanup() {
    git -C "$TARGET" remote remove "$temp_remote" >/dev/null 2>&1 || true
}
trap cleanup EXIT

cleanup
git -C "$TARGET" remote add "$temp_remote" "$SOURCE_ROOT"
git -C "$TARGET" fetch --quiet "$temp_remote" HEAD \
    || die "could not fetch the source checkout"
source_head=$(git -C "$TARGET" rev-parse FETCH_HEAD)

if git -C "$TARGET" merge-base --is-ancestor "$source_head" "$target_head"; then
    say "target already contains $SOURCE_BRANCH at $(git -C "$SOURCE_ROOT" rev-parse --short HEAD); nothing to merge"
    if [ "$DRY_RUN" -eq 1 ]; then
        say "dry run: nothing changed"
        exit 0
    fi
    if [ "$PUSH" -eq 1 ] \
        && [ "$(git -C "$TARGET" rev-list --count "origin/$BRANCH..$target_head")" -gt 0 ]; then
        run_checks
    fi
    push_if_ahead
    ff_source_onto "$target_head"
    exit 0
fi

if git -C "$TARGET" merge-base --is-ancestor "$target_head" "$source_head"; then
    plan="fast-forward"
elif git -C "$TARGET" merge-base "$target_head" "$source_head" >/dev/null 2>&1; then
    plan="merge"
else
    plan="unrelated"
fi

case "$plan" in
    fast-forward)
        say "plan    fast-forward to $(git -C "$TARGET" rev-parse --short "$source_head")"
        git -C "$TARGET" diff --stat "$target_head" "$source_head" | tail -3 | sed 's/^/  /'
        ;;
    merge)
        say "plan    merge $(git -C "$TARGET" rev-list --count "$target_head..$source_head") commit(s) in"
        git -C "$TARGET" diff --stat "$target_head" "$source_head" | tail -3 | sed 's/^/  /'
        ;;
    unrelated)
        say "plan    unrelated histories: $(git -C "$TARGET" rev-list --count HEAD) + $(git -C "$SOURCE_ROOT" rev-list --count HEAD) commits, no common ancestor"
        note "the source tree wins wherever the two disagree"
        git -C "$TARGET" diff --stat "$target_head" "$source_head" | tail -3 | sed 's/^/  /'
        if [ "$ALLOW_UNRELATED" -ne 1 ]; then
            die "refusing to join unrelated histories; re-run with --allow-unrelated if that is what you want"
        fi
        ;;
esac

if [ "$DRY_RUN" -eq 1 ]; then
    [ "$RUN_CHECKS" -eq 1 ] && say "checks  fmt, clippy, test would run first"
    say "dry run: nothing changed"
    exit 0
fi

# --- checks --------------------------------------------------------------

run_checks

# --- move ----------------------------------------------------------------

merge_abort() {
    git -C "$TARGET" merge --abort >/dev/null 2>&1 || true
}

case "$plan" in
    fast-forward)
        git -C "$TARGET" merge --quiet --ff-only "$source_head" \
            || die "fast-forward failed; the target may have moved"
        ;;
    merge)
        if ! git -C "$TARGET" merge --no-ff --no-commit "$source_head" >/dev/null 2>&1; then
            printf 'conflicting paths:\n' >&2
            git -C "$TARGET" diff --name-only --diff-filter=U | sed 's/^/  /' >&2
            merge_abort
            die "the two lines disagree in a way only a human can settle; merge by hand, then re-run"
        fi
        ;;
    unrelated)
        if ! git -C "$TARGET" merge --allow-unrelated-histories -X theirs \
                --no-ff --no-commit "$source_head" >/dev/null 2>&1; then
            printf 'conflicting paths:\n' >&2
            git -C "$TARGET" diff --name-only --diff-filter=U | sed 's/^/  /' >&2
            merge_abort
            die "even taking the source side left conflicts; merge by hand, then re-run"
        fi
        ;;
esac

# --- prove the mirror -----------------------------------------------------
#
# The target may hold files the source never had (an upload can add one); those
# are allowed to survive. A path the source does ship must match it exactly, or
# the "mirror" claim is false and the merge is undone before it is committed.

drift=""
while IFS= read -r path; do
    [ -n "$path" ] || continue
    if git -C "$SOURCE_ROOT" cat-file -e "$source_head:$path" 2>/dev/null; then
        drift="$drift$path"$'\n'
    fi
done < <(git -C "$TARGET" diff --name-only "$source_head")

if [ -n "$drift" ]; then
    printf 'paths that would not match the source:\n' >&2
    printf '%s' "$drift" | sed 's/^/  /' >&2
    merge_abort
    die "the mirror check failed; nothing was committed or pushed"
fi

git_dir=$(git -C "$TARGET" rev-parse --absolute-git-dir)
if [ -f "$git_dir/MERGE_HEAD" ]; then
    source_short=$(git -C "$TARGET" rev-parse --short "$source_head")
    git -C "$TARGET" commit --quiet -m "$(cat <<EOF
Sync $BRANCH with the Freebuff thread at $source_short

Mirrored by scripts/sync-to-github.sh from
$SOURCE_ROOT.
EOF
)"
fi

new_head=$(git -C "$TARGET" rev-parse HEAD)
say "target now at $(git -C "$TARGET" rev-parse --short "$new_head")"

# --- push -----------------------------------------------------------------

push_if_ahead

# --- keep the two on one commit ------------------------------------------

ff_source_onto "$new_head"

say "done"

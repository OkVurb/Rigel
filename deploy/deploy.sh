#!/usr/bin/env bash
# Build the site, validate the local deployment contract, bootstrap the VPS if
# needed, upload the source and built site, then run the remote compose stack.
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"

HOST="${VPS_HOST:-}"
SSH_USER="${VPS_USER:-rigel}"
BOOTSTRAP_USER="${VPS_BOOTSTRAP_USER:-root}"
SSH_KEY="${VPS_SSH_KEY:-$HOME/.ssh/rigel_vps}"
ENV_FILE="${RIGEL_ENV_FILE:-$SCRIPT_DIR/.env.production}"
SITE_DIR="${RIGEL_SITE_DIR:-$ROOT_DIR/.deploy}"
DRY_RUN=0
SKIP_BOOTSTRAP=0
BUILD_SITE=1

usage() {
  cat <<'EOF'
Usage: ./deploy/deploy.sh --host <ip-or-host> [options]

Options:
  --host <host>             VPS IPv4 address or hostname (or VPS_HOST=)
  --user <name>             deploy user (default: rigel)
  --bootstrap-user <name>   first-login user (default: root)
  --ssh-key <path>          private key (default: ~/.ssh/rigel_vps)
  --env-file <path>         production env file (default: deploy/.env.production)
  --site <path>             built static site (default: .deploy)
  --skip-site-build         use the existing .deploy instead of rebuilding it
  --skip-bootstrap          fail instead of running the root bootstrap
  --dry-run                 validate locally and print the remote steps only
  -h, --help                show this help
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --host) HOST="${2:?--host requires a value}"; shift 2 ;;
    --host=*) HOST="${1#*=}"; shift ;;
    --user) SSH_USER="${2:?--user requires a value}"; shift 2 ;;
    --user=*) SSH_USER="${1#*=}"; shift ;;
    --bootstrap-user) BOOTSTRAP_USER="${2:?--bootstrap-user requires a value}"; shift 2 ;;
    --bootstrap-user=*) BOOTSTRAP_USER="${1#*=}"; shift ;;
    --ssh-key) SSH_KEY="${2:?--ssh-key requires a value}"; shift 2 ;;
    --ssh-key=*) SSH_KEY="${1#*=}"; shift ;;
    --env-file) ENV_FILE="${2:?--env-file requires a value}"; shift 2 ;;
    --env-file=*) ENV_FILE="${1#*=}"; shift ;;
    --site) SITE_DIR="${2:?--site requires a value}"; shift 2 ;;
    --site=*) SITE_DIR="${1#*=}"; shift ;;
    --skip-site-build) BUILD_SITE=0; shift ;;
    --skip-bootstrap) SKIP_BOOTSTRAP=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done

log() { printf '\n[rigel-deploy] %s\n' "$*"; }
fail() { printf '\n[rigel-deploy] ERROR: %s\n' "$*" >&2; exit 1; }

[ -n "$HOST" ] || { usage >&2; fail "set --host or VPS_HOST"; }
[ -f "$SSH_KEY" ] || fail "private key not found: $SSH_KEY"
[ -f "$SSH_KEY.pub" ] || fail "public key not found: $SSH_KEY.pub"
[ -f "$ENV_FILE" ] || fail "env file not found: $ENV_FILE (run: node deploy/init-env.mjs)"
[ -d "$SITE_DIR" ] || [ "$BUILD_SITE" -eq 1 ] || fail "site directory not found: $SITE_DIR"
command -v ssh >/dev/null || fail "ssh is required"
command -v tar >/dev/null || fail "tar is required"
command -v node >/dev/null || fail "node is required"

if [ "$BUILD_SITE" -eq 1 ]; then
  log "building the static site"
  (
    set -a
    # shellcheck disable=SC1091
    . "$ENV_FILE"
    set +a
    cd "$ROOT_DIR"
    node scripts/build-web.mjs
  )
fi
[ -f "$SITE_DIR/index.html" ] || fail "site build is missing $SITE_DIR/index.html"

log "validating compose, Caddyfile, env, and site"
node "$SCRIPT_DIR/check.mjs" --env "$ENV_FILE" --site "$SITE_DIR"

if [ "$DRY_RUN" -eq 1 ]; then
  cat <<EOF

[rigel-deploy] dry run complete; nothing was uploaded or changed remotely.
  host:        $HOST
  deploy user: $SSH_USER
  key:         $SSH_KEY
  env:         $ENV_FILE
  site:        $SITE_DIR
  would run:
    ssh ${BOOTSTRAP_USER}@${HOST} 'bash /tmp/rigel-bootstrap.sh'
    tar deploy server cms | ssh ${SSH_USER}@${HOST} 'tar -C /opt/rigel -xzf -'
    tar -C ${SITE_DIR} . | ssh ${SSH_USER}@${HOST} 'tar -C /opt/rigel/site -xzf -'
    ssh ${SSH_USER}@${HOST} 'bash /opt/rigel/deploy/remote.sh'
EOF
  exit 0
fi

SSH_OPTS=(-i "$SSH_KEY" -o BatchMode=yes -o StrictHostKeyChecking=accept-new -o ConnectTimeout=15)

log "checking whether ${SSH_USER}@${HOST} is already prepared"
if ssh "${SSH_OPTS[@]}" "$SSH_USER@$HOST" 'command -v docker >/dev/null 2>&1 && test -w /opt/rigel' >/dev/null 2>&1; then
  log "host is already prepared"
else
  [ "$SKIP_BOOTSTRAP" -eq 1 ] && fail "host is not prepared and --skip-bootstrap was requested"
  ssh "${SSH_OPTS[@]}" "$BOOTSTRAP_USER@$HOST" 'true' >/dev/null 2>&1 \
    || fail "key login as $BOOTSTRAP_USER@$HOST failed; install $SSH_KEY.pub for that user first"
  log "uploading bootstrap and public key"
  ssh "${SSH_OPTS[@]}" "$BOOTSTRAP_USER@$HOST" 'cat > /tmp/rigel-bootstrap.sh' < "$SCRIPT_DIR/bootstrap.sh"
  ssh "${SSH_OPTS[@]}" "$BOOTSTRAP_USER@$HOST" 'cat > /tmp/rigel-vps.pub' < "$SSH_KEY.pub"
  log "running bootstrap as $BOOTSTRAP_USER"
  ssh "${SSH_OPTS[@]}" "$BOOTSTRAP_USER@$HOST" \
    'RIGEL_SSH_PUBKEY_FILE=/tmp/rigel-vps.pub bash /tmp/rigel-bootstrap.sh'

  log "waiting for key login as $SSH_USER"
  ready=0
  for _ in $(seq 1 20); do
    if ssh "${SSH_OPTS[@]}" "$SSH_USER@$HOST" 'true' >/dev/null 2>&1; then ready=1; break; fi
    sleep 2
  done
  [ "$ready" -eq 1 ] || fail "bootstrap finished but key login as $SSH_USER failed"
fi

log "uploading application source"
ssh "${SSH_OPTS[@]}" "$SSH_USER@$HOST" 'mkdir -p /opt/rigel/deploy /opt/rigel/site'
tar -C "$ROOT_DIR" -czf - \
  --exclude='server/node_modules' \
  --exclude='server/test' \
  --exclude='cms/node_modules' \
  --exclude='cms/.next' \
  --exclude='cms/cms.db' \
  --exclude='cms/cms.db-*' \
  --exclude='cms/media' \
  --exclude='cms/.env' \
  --exclude='cms/test.env' \
  --exclude='cms/test-results' \
  --exclude='cms/playwright-report' \
  deploy server cms \
  | ssh "${SSH_OPTS[@]}" "$SSH_USER@$HOST" 'tar -C /opt/rigel -xzf -'

printf '%s' "$(cat "$ENV_FILE")" | ssh "${SSH_OPTS[@]}" "$SSH_USER@$HOST" \
  'umask 077; cat > /opt/rigel/deploy/.env.production'
ssh "${SSH_OPTS[@]}" "$SSH_USER@$HOST" \
  'chmod +x /opt/rigel/deploy/bootstrap.sh /opt/rigel/deploy/remote.sh /opt/rigel/deploy/deploy.sh 2>/dev/null || true'

log "uploading the built site"
ssh "${SSH_OPTS[@]}" "$SSH_USER@$HOST" 'mkdir -p /opt/rigel/site && find /opt/rigel/site -mindepth 1 -delete'
tar -C "$SITE_DIR" -czf - . | ssh "${SSH_OPTS[@]}" "$SSH_USER@$HOST" 'tar -C /opt/rigel/site -xzf -'

log "running the remote deployment"
ssh "${SSH_OPTS[@]}" "$SSH_USER@$HOST" 'bash /opt/rigel/deploy/remote.sh'

log "deployment finished"
printf '  site: https://askrigel.com (after DNS points at %s)\n' "$HOST"
printf '  api:  https://api.askrigel.com/health\n'
printf '  cms:  https://admin.askrigel.com/admin\n'

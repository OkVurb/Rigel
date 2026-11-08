#!/usr/bin/env bash
# Publish AAAA records only after the VPS actually has a routable IPv6 address.
# The server already requests DHCPv6; Caddy publishes [::]:80/443; bootstrap UFW
# allows 80/443. Run this from the repository root after IONOS assigns IPv6.
set -Eeuo pipefail
cd "$(dirname "$0")/.."

HOST="209.46.126.99"
SSH_KEY="${RIGEL_SSH_KEY:-$HOME/.ssh/rigel_vps}"
SSH_USER="${RIGEL_SSH_USER:-rigel}"
ZONE="askrigel.com"
NAMES=(askrigel.com www.askrigel.com api.askrigel.com admin.askrigel.com)
IPV6=""
APPLY=0

usage() {
  cat <<'EOF'
Usage: ./deploy/ipv6-cutover.sh [--host IP] [--ipv6 ADDRESS] [--apply]

Without --ipv6, the script reads the first global IPv6 address on the VPS.
Without --apply it only prints the AAAA plan. It refuses to publish AAAA
records when the VPS has no global IPv6, because that would blackhole IPv6.

Prerequisite: assign one public IPv6 address to the VPS in IONOS Cloud Panel
(Servers & Cloud > VPS > Network > Public IP > Create/Assign > IPv6) and reboot.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --host) HOST="${2:?--host needs a value}"; shift 2 ;;
    --ipv6) IPV6="${2:?--ipv6 needs a value}"; shift 2 ;;
    --apply) APPLY=1; shift ;;
    --help|-h) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [ -z "$IPV6" ]; then
  IPV6="$(ssh -i "$SSH_KEY" -o BatchMode=yes -o StrictHostKeyChecking=accept-new \
    "$SSH_USER@$HOST" 'ip -6 addr show scope global' 2>/dev/null \
    | awk '$1 == "inet6" {print $2}' | cut -d/ -f1 | grep -E '^[23]' | head -n1 || true)"
fi

if [ -z "$IPV6" ]; then
  cat >&2 <<EOF
No global IPv6 address is assigned to $HOST yet.
In IONOS Cloud Panel, assign one public IPv6 address to this VPS and reboot:
  Servers & Cloud > <contract> > VPS > Network > Public IP > Create/Assign > IPv6 > reboot
The server already requests DHCPv6, Caddy publishes [::]:80/443, and the bootstrap
firewall allows 80/443. No AAAA records were published.
After the address appears, run:
  ./deploy/ipv6-cutover.sh --host $HOST --apply
EOF
  exit 2
fi

node -e '
const net = require("node:net");
const ip = process.argv[1];
if (!net.isIPv6(ip) || !/^[23]/.test(ip)) {
  console.error(`not a global unicast IPv6 address: ${ip}`);
  process.exit(1);
}
' "$IPV6"

echo "Global IPv6: $IPV6"
echo "Checking Caddy IPv6 listeners..."
PORTS="$(ssh -i "$SSH_KEY" -o BatchMode=yes -o StrictHostKeyChecking=accept-new \
  "$SSH_USER@$HOST" 'cd /opt/rigel/deploy && docker compose --env-file .env.production ps --format "{{.Ports}}"' 2>/dev/null)"
grep -q '\[::\]:80' <<<"$PORTS" || { echo "Caddy is not publishing [::]:80" >&2; exit 1; }
grep -q '\[::\]:443' <<<"$PORTS" || { echo "Caddy is not publishing [::]:443" >&2; exit 1; }

echo "Checking Caddy over IPv6 from the VPS itself..."
check_v6() {
  local host="$1" path="$2" code
  code="$(ssh -i "$SSH_KEY" -o BatchMode=yes -o StrictHostKeyChecking=accept-new "$SSH_USER@$HOST" \
    "curl -6 -sS -o /dev/null -w '%{http_code}' --max-time 15 --resolve '$host:443:[$IPV6]' 'https://$host$path'" 2>/dev/null || true)"
  if [ "$code" != "200" ]; then
    echo "IPv6 $host$path returned '${code:-no response}'" >&2
    exit 1
  fi
  echo "PASS IPv6 $host$path $code"
}
while IFS='|' read -r v6_host v6_path; do
  check_v6 "$v6_host" "$v6_path"
done <<'EOF'
askrigel.com|/
www.askrigel.com|/
api.askrigel.com|/health
admin.askrigel.com|/admin
askrigel.com|/site.css
EOF

set -a
# shellcheck disable=SC1091
. ./.env.ionos
set +a

if [ "$APPLY" -eq 0 ]; then
  echo "Dry run: AAAA records will be created/updated. Re-run with --apply to publish."
  for name in "${NAMES[@]}"; do
    node deploy/ionos.mjs ensure --zone "$ZONE" --name "$name" --type AAAA --content "$IPV6" --ttl 300
  done
  exit 0
fi

for name in "${NAMES[@]}"; do
  node deploy/ionos.mjs ensure --zone "$ZONE" --name "$name" --type AAAA --content "$IPV6" --ttl 300 --apply
done

echo "Published AAAA records for: ${NAMES[*]}"
echo "Verifying authoritative DNS:"
for name in "${NAMES[@]}"; do
  printf '%-24s ' "$name"
  nslookup -type=AAAA "$name" 2>/dev/null | awk '/^Address: /{print $2}' | tail -n 2 | tr '\n' ' '
  echo
done
echo "Verify from an IPv6-only network:"
echo "  curl -6 -fsS https://askrigel.com/"
echo "  curl -6 -fsS https://www.askrigel.com/"
echo "  curl -6 -fsS https://api.askrigel.com/health"
echo "  curl -6 -fsS 'https://admin.askrigel.com/api/faqs?limit=1'"
echo "  curl -6 -fsSI https://admin.askrigel.com/admin"

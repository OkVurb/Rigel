#!/usr/bin/env bash
# Run on the VPS after deploy.sh has uploaded the source and site.
set -Eeuo pipefail
cd /opt/rigel/deploy

set -a
# shellcheck disable=SC1091
. ./.env.production
set +a

COMPOSE=(docker compose --env-file .env.production)
"${COMPOSE[@]}" config --quiet

log() { printf '\n[rigel-remote] %s\n' "$*"; }

log "starting Postgres"
"${COMPOSE[@]}" up -d db

for _ in $(seq 1 60); do
  if "${COMPOSE[@]}" exec -T db pg_isready -U "$POSTGRES_USER" -d "$POSTGRES_DB" >/dev/null 2>&1; then
    break
  fi
  sleep 2
done
"${COMPOSE[@]}" exec -T db pg_isready -U "$POSTGRES_USER" -d "$POSTGRES_DB" >/dev/null

if ! "${COMPOSE[@]}" exec -T db psql -U "$POSTGRES_USER" -d postgres -tAc \
  "SELECT 1 FROM pg_database WHERE datname='$POSTGRES_CMS_DB'" | grep -qx 1; then
  log "creating the CMS database"
  "${COMPOSE[@]}" exec -T db createdb -U "$POSTGRES_USER" -O "$POSTGRES_USER" "$POSTGRES_CMS_DB"
fi

log "applying Payload migrations"
"${COMPOSE[@]}" run --rm cms npm run migrate

if [ ! -f /opt/rigel/.cms-seeded ]; then
  log "seeding FAQs and the first CMS admin"
  "${COMPOSE[@]}" run --rm cms npm run seed
  install -m 600 /dev/null /opt/rigel/.cms-seeded
fi

log "building and starting api, cms, and caddy"
"${COMPOSE[@]}" up -d --build --remove-orphans

log "waiting for the API"
api_ok=0
for _ in $(seq 1 60); do
  if "${COMPOSE[@]}" exec -T api node -e "fetch('http://127.0.0.1:8080/health').then(r=>{if(!r.ok)process.exit(1)}).catch(()=>process.exit(1))" >/dev/null 2>&1; then
    api_ok=1; break
  fi
  sleep 2
done
[ "$api_ok" -eq 1 ] || { "${COMPOSE[@]}" logs --tail=80 api; exit 1; }

log "waiting for the CMS"
cms_ok=0
for _ in $(seq 1 90); do
  if "${COMPOSE[@]}" exec -T cms node -e "fetch('http://127.0.0.1:3000/api/faqs?limit=1').then(r=>{if(!r.ok)process.exit(1)}).catch(()=>process.exit(1))" >/dev/null 2>&1; then
    cms_ok=1; break
  fi
  sleep 2
done
[ "$cms_ok" -eq 1 ] || { "${COMPOSE[@]}" logs --tail=80 cms; exit 1; }

log "container status"
"${COMPOSE[@]}" ps

log "HTTP smoke checks (TLS is issued after DNS points here)"
caddy_ok=0
for _ in $(seq 1 30); do
  if curl -fsSI -H 'Host: askrigel.com' http://127.0.0.1/ >/dev/null 2>&1; then caddy_ok=1; break; fi
  sleep 1
done
[ "$caddy_ok" -eq 1 ] || { "${COMPOSE[@]}" logs --tail=80 caddy; exit 1; }
curl -fsSI -H 'Host: askrigel.com' http://127.0.0.1/ | sed -n '1,4p'
"${COMPOSE[@]}" exec -T api node -e "fetch('http://127.0.0.1:8080/health').then(r=>{if(!r.ok)process.exit(1);return r.text()}).then(t=>process.stdout.write(t)).catch(()=>process.exit(1))"
printf '\n'

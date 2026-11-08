# Deploying Rigel to askrigel.com

This is the production runbook for the VPS stack. It is deliberately separate from the
Firebase project so the migration can be staged and rolled back.

## What runs

| Service | Image/build | Network | Public URL |
| --- | --- | --- | --- |
| `caddy` | `caddy:2-alpine` | publishes 80/443 | `askrigel.com`, `www.askrigel.com`, `api.askrigel.com`, `admin.askrigel.com` |
| `api` | `server/Dockerfile` | internal 8080 | `https://api.askrigel.com`, including the scoped Rigel↔Muse relay |
| `cms` | `cms/Dockerfile` | internal 3000 | `https://admin.askrigel.com` |
| `db` | `postgres:17-alpine` | internal 5432 only | none |
| static site | `.deploy/` built by `scripts/build-web.mjs` | mounted into Caddy at `/srv/site` | `https://askrigel.com` |

The API and CMS share one Postgres container but use separate databases: `rigel` and
`rigel_cms`. The CMS still uses the existing public Firebase web config for the web reader;
those values are public and generated into `.deploy/app/config.js`.

The site's `/api/contact` and `/api/gravity-ad` calls are temporarily reverse-proxied to the
existing Firebase Functions in `functions/index.js`. Remove those two `handle` blocks from
`deploy/Caddyfile` when the Fastify public routes replace them.

## Prerequisites

- A fresh Ubuntu 24.04 or Debian 12 VPS with a public IPv4 address.
- SSH key access as `root` for the first bootstrap. The bootstrap then creates `rigel`.
- Local Node 22+, `tar`, `ssh`, and the Docker CLI (the CLI is only needed for local
  `docker compose config` validation; Docker Desktop does not need to be running).
- The `rigel-vps` key pair at `~/.ssh/rigel_vps` and `~/.ssh/rigel_vps.pub`.

If the key is not yet on the server, add it once from a machine that can log in:

```bash
mkdir -p ~/.ssh && chmod 700 ~/.ssh
cat ~/.ssh/rigel_vps.pub | ssh root@<VPS_IP> \
  'mkdir -p ~/.ssh && chmod 700 ~/.ssh && cat >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys'
```

## Generate the production env file

```bash
node deploy/init-env.mjs
```

This writes the gitignored `deploy/.env.production` with random Postgres, JWT, Payload, and
admin secrets. It reads the existing public Firebase config from `.deploy/app/config.js`.
It does **not** print any secret. The first CMS admin email is `hello@askrigel.com`; the
generated password is in that file. Change the email or password there before the first
deploy if you want a different admin login.

`node deploy/init-env.mjs` also creates distinct 256-bit `RIGEL_RELAY_TOKEN` and
`MUSE_RELAY_TOKEN` values. Keep the first on the Rigel machine and install the second in Muse's
secure store; never copy either into source, logs, URLs, or chat.

Fill in the optional external values when they are available:

- `RESEND_API_KEY` for verification/reset/contact mail.
- `LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODELS` for the gateway.
- `GRAVITY_API_KEY` and `GRAVITY_PRODUCTION` for the ad bridge.
- `TURNSTILE_SECRET`, `ENROLL_TOKEN`, `REQUIRE_DEVICE` when device enrollment is enabled.

Never commit `deploy/.env.production`. The root `.gitignore` ignores `.env.*`.

## Validate before deploying

```bash
node deploy/check.mjs
```

The checker verifies:

- every env key read by `server/src/config.js` and the CMS is present;
- `docker compose config` parses and the service names, build contexts, ports, and
  `DATABASE_URL` values match the code;
- Caddy targets `api:8080`, `cms:3000`, uses `/srv/site`, and has the four public hostnames;
- `.deploy` contains the built hashed site and a non-empty Firebase config;
- the Dockerfiles expose/start the same ports and commands the compose file expects;
- the shell scripts and `init-env.mjs` parse.

Caddy itself is optional locally:

```bash
# If caddy is not installed:
CADDY_BIN=/path/to/caddy.exe node deploy/check.mjs
```

## Deploy

The first run installs packages and Docker, creates the `rigel` user, adds the public key,
adds 4 GB swap, enables UFW for 22/80/443, and disables root/password SSH. It then builds the
static site, uploads the source, applies migrations, seeds the CMS once, and starts the stack.

```bash
./deploy/deploy.sh --host <VPS_IP>
```

Dry run (builds/validates locally, prints the remote steps, changes nothing remotely):

```bash
./deploy/deploy.sh --host <VPS_IP> --dry-run
```

If the server is already prepared and you only changed the site or application:

```bash
./deploy/deploy.sh --host <VPS_IP> --skip-bootstrap
```

The deploy script uses `/opt/rigel` on the server and `deploy/.env.production` locally. The
private key never leaves the local machine and is never uploaded. The server receives only
the public key during bootstrap.

## DNS records

Point all four names at the VPS. Use a low TTL (300 seconds) for the cutover; keep the old
records until the new stack has been verified.

| Name | Type | Value | TTL |
| --- | --- | --- | --- |
| `askrigel.com` | A | `<VPS_IP>` | 300 |
| `www.askrigel.com` | A | `<VPS_IP>` | 300 |
| `api.askrigel.com` | A | `<VPS_IP>` | 300 |
| `admin.askrigel.com` | A | `<VPS_IP>` | 300 |

Do not add an AAAA record unless the VPS has a working IPv6 address. Do not change MX, SPF,
DKIM, or DMARC: mail stays on IONOS. Before the first cutover the apex and `www` pointed at
the old shared hosting; changing the apex A record can make IONOS deactivate the old apex
AAAA, the old `www` A/AAAA, and its website mutex. Verify the zone after the cutover and
remove any remaining old AAAA record if the VPS has no IPv6.

For the IONOS API helper, plan the full cutover with:

```bash
set -a; . ./.env.ionos; set +a
node deploy/ionos.mjs cutover --zone askrigel.com --ip <VPS_IP> --names askrigel.com,www,api,admin
node deploy/ionos.mjs cutover --zone askrigel.com --ip <VPS_IP> --names askrigel.com,www,api,admin --apply
```

The helper re-reads the zone before each applied change because IONOS can replace record IDs
when an update deactivates conflicting records.

The apex and `www` change is the actual site cutover; do it only after the API/CMS health
checks pass and after you are ready to move traffic off Firebase Hosting.

## TLS

Caddy obtains and renews Let's Encrypt certificates automatically for all four hostnames.
There is no need to use the IONOS wildcard certificate quota (it is already fully used).
Caddy needs inbound TCP 80 and 443 and working DNS before it can complete HTTP-01/TLS-ALPN
validation. It retries automatically while DNS propagates.

## IPv6

The VPS currently has no global IPv6 address. IONOS assigns public IPv6 to this VPS class
in the Cloud Panel; for one IPv4 plus one IPv6, cloud-init/DHCPv6 configures it after a
reboot. The server already requests DHCPv6, Caddy publishes `[::]:80/443`, and the bootstrap
UFW rules allow 80/443. Do not publish AAAA records before the address exists; that would
blackhole IPv6 clients.

After IONOS assigns the address and the VPS reboots:

```bash
./deploy/ipv6-cutover.sh --host <VPS_IP>
./deploy/ipv6-cutover.sh --host <VPS_IP> --apply
```

The helper reads the global address from the VPS, checks Caddy's IPv6 listeners and the
five public paths over IPv6 with valid certificates, then publishes AAAA records for
`askrigel.com`, `www`, `api`, and `admin`. It refuses to publish if no global IPv6 is
present.

## Verify

On the VPS:

```bash
ssh -i ~/.ssh/rigel_vps rigel@<VPS_IP>
cd /opt/rigel/deploy
docker compose --env-file .env.production ps
docker compose --env-file .env.production logs --tail=100 caddy api cms
# The API and CMS ports are internal-only; query them inside their containers.
docker compose --env-file .env.production exec -T api node -e "fetch('http://127.0.0.1:8080/health').then(r=>{if(!r.ok)process.exit(1);return r.text()}).then(t=>process.stdout.write(t)).catch(()=>process.exit(1))"
printf '\n'
docker compose --env-file .env.production exec -T cms node -e "fetch('http://127.0.0.1:3000/api/faqs?limit=1').then(r=>{if(!r.ok)process.exit(1);return r.text()}).then(t=>process.stdout.write(t)).catch(()=>process.exit(1))"
printf '\n'
curl -fsSI -H 'Host: askrigel.com' http://127.0.0.1/
```

After DNS points at the VPS:

```bash
curl -fsS https://api.askrigel.com/health
curl -fsSI https://askrigel.com/
curl -fsSI https://www.askrigel.com/
curl -fsS 'https://admin.askrigel.com/api/faqs?limit=1'
```

The API health response is JSON with `"ok": true`. Verify each relay role separately with its
bearer credential at `GET https://api.askrigel.com/v1/relay/health`; an unauthenticated request
must return `401`. The CMS `/api/faqs` response is JSON with
a `docs` array. The apex response should be the static site over a valid certificate.

## Operations

```bash
cd /opt/rigel/deploy
docker compose --env-file .env.production restart caddy
docker compose --env-file .env.production logs -f --tail=100 api
docker compose --env-file .env.production up -d --build api
```

Do not use `docker compose down -v`: `-v` deletes the Postgres, API data, CMS media, and
Caddy certificate volumes. A normal `down` keeps them.

Back up Postgres:

```bash
docker compose --env-file .env.production exec -T db \
  pg_dump -U rigel -d rigel | gzip > /opt/rigel/rigel-$(date +%F).sql.gz
docker compose --env-file .env.production exec -T db \
  pg_dump -U rigel -d rigel_cms | gzip > /opt/rigel/rigel-cms-$(date +%F).sql.gz
```

## Rollback

1. Point the DNS records back at the previous host (`74.208.236.210` for the current apex).
2. Leave the VPS stack running or stop it with `docker compose --env-file .env.production down`
   (never `-v`).
3. Do not delete the Firebase project, Firestore data, or the existing hosting site until the
   new stack has served real traffic and the data import is complete.

## Remaining migration work

This runbook deploys the current code, not the entire Firebase migration:

- The Fastify API currently exposes health and auth/device/session routes; the cloud, files,
  published-session, LLM, contact, and ad routes are still to be implemented.
- The web reader still uses Firebase Auth/Firestore directly; the static site is simply hosted
  on the VPS now.
- Contact and Gravity ads are temporarily bridged to the existing Firebase Functions.
- Resend, LLM provider credentials, and Firebase data export/import are external inputs.

Keep the existing Firebase Hosting deployment live until the above are complete and the new
stack has passed real-user verification.

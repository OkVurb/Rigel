# IONOS migration helper

The IONOS Hosting APIs cover **domains, DNS and DV certificates**. They do **not**
provision a VPS, so the server still has to be ordered in the IONOS account before
the subdomains can be pointed at it.

## API access

1. Enable API access for the account and create a key at
   <https://developer.hosting.ionos.com/keys>.
2. Export the key locally. Never commit it or paste it into chat.

   ```bash
   export IONOS_API_KEY='publicprefix.secret'
   ```

The key is sent as `X-API-Key`. The rate limit is 1,200 requests/hour per key.

## Commands

All changes are dry runs unless `--apply` is passed.

```bash
# Inventory
node deploy/ionos.mjs zones
node deploy/ionos.mjs records --zone askrigel.com
node deploy/ionos.mjs ssl:quota

# Preview, then point api/admin at the VPS
node deploy/ionos.mjs cutover --zone askrigel.com --ip 203.0.113.10
node deploy/ionos.mjs cutover --zone askrigel.com --ip 203.0.113.10 --apply

# One record at a time (A, AAAA, CNAME, TXT, …)
node deploy/ionos.mjs ensure --zone askrigel.com \
  --name api --type A --content 203.0.113.10 --ttl 300
```

`cutover` defaults to `api,admin`. It refuses to modify a name that already has
more than one record of the same type, so a forgotten round-robin is never
half-updated. The apex and `www` records are intentionally left alone until the
web/CMS cutover.

## SSL

Caddy on the VPS obtains and renews public certificates from Let's Encrypt once
`api.askrigel.com` and `admin.askrigel.com` resolve to the server. The IONOS SSL
API is only needed if you deliberately choose an IONOS DV certificate; it
requires an unused certificate of the requested type already purchased in the
account, and `ssl:quota` shows whether one exists.

## VPS prerequisite

Order the IONOS VPS S+ with Ubuntu 24.04, add an SSH public key, and record its
IPv4 address. Do not paste a root password; the install uses the key. Once the
address exists, run the `cutover` dry run, inspect it, and apply it.

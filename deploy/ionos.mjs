#!/usr/bin/env node
// IONOS Hosting API helper for the askrigel.com migration.
//
// It deliberately does not create servers: the Hosting APIs cover domains, DNS and
// DV certificates only. The VPS is ordered in the IONOS account, then this script
// points the subdomains at it.
//
// Auth: IONOS_API_KEY=publicprefix.secret (never commit or print it).

import { pathToFileURL } from 'node:url';

const DNS_BASE = 'https://api.hosting.ionos.com/dns';
const SSL_BASE = 'https://api.hosting.ionos.com/ssl';
const DEFAULT_ZONE = 'askrigel.com';
const DEFAULT_TTL = 300;

export function stripDot(value) {
  return String(value ?? '').trim().replace(/\.$/, '').toLowerCase();
}

/** Turn "api", "api.askrigel.com" or "@" into the fully-qualified record name. */
export function recordName(value, zone) {
  const z = stripDot(zone);
  const name = stripDot(value);
  if (!name || name === '@') return z;
  if (name === z || name.endsWith(`.${z}`)) return name;
  return `${name}.${z}`;
}

export function matchesRecord(record, name, type) {
  return (
    stripDot(record.name) === stripDot(name) &&
    String(record.type ?? '').toUpperCase() === String(type ?? '').toUpperCase()
  );
}

/**
 * Decide what an upsert should do. More than one existing record at the same
 * name/type is ambiguous: changing only one of them would leave a round-robin in
 * place, so the caller must resolve it explicitly.
 */
export function planRecord(records, desired) {
  const matches = (records ?? []).filter((record) =>
    matchesRecord(record, desired.name, desired.type),
  );
  if (matches.length === 0) return { action: 'create', desired };
  if (matches.length > 1) {
    return {
      action: 'ambiguous',
      matches: matches.map(({ id, name, type, content, ttl }) => ({ id, name, type, content, ttl })),
    };
  }
  const current = matches[0];
  const changes = {};
  for (const key of ['content', 'ttl', 'prio', 'disabled']) {
    if (desired[key] !== undefined && String(current[key] ?? '') !== String(desired[key])) {
      changes[key] = desired[key];
    }
  }
  if (Object.keys(changes).length === 0) return { action: 'unchanged', id: current.id };
  return { action: 'update', id: current.id, changes, current };
}

function parseArgs(argv) {
  if (argv.length === 0 || argv[0] === '--help' || argv[0] === '-h') {
    return { command: 'help', apply: false, json: false, help: true };
  }
  const [command, ...rest] = argv;
  const options = { command, apply: false, json: false };
  for (let index = 0; index < rest.length; index += 1) {
    const token = rest[index];
    if (token === '--apply') options.apply = true;
    else if (token === '--json') options.json = true;
    else if (token === '--help') options.help = true;
    else if (token.startsWith('--')) {
      const [rawKey, inline] = token.slice(2).split('=', 2);
      const key = rawKey.replaceAll('-', '_');
      const value = inline ?? rest[index + 1];
      if (inline === undefined) index += 1;
      if (value === undefined) throw new Error(`--${rawKey} needs a value`);
      options[key] = value;
    } else {
      throw new Error(`unexpected argument: ${token}`);
    }
  }
  return options;
}

function apiKey() {
  const key = process.env.IONOS_API_KEY?.trim();
  if (!key) {
    throw new Error(
      'IONOS_API_KEY is not set. Create a key at https://developer.hosting.ionos.com/keys and export IONOS_API_KEY=publicprefix.secret',
    );
  }
  return key;
}

async function request(base, path, init = {}) {
  const headers = {
    'X-API-Key': apiKey(),
    Accept: 'application/json',
    ...(init.body ? { 'Content-Type': 'application/json' } : {}),
    ...init.headers,
  };
  const response = await fetch(`${base}${path}`, {
    ...init,
    headers,
    signal: AbortSignal.timeout(20_000),
  });
  const text = await response.text();
  let body = null;
  if (text) {
    try {
      body = JSON.parse(text);
    } catch {
      body = text;
    }
  }
  if (!response.ok) {
    const message =
      body?.message ?? body?.error ?? (typeof body === 'string' ? body.slice(0, 300) : '');
    throw new Error(`IONOS ${response.status}${message ? `: ${message}` : ''}`);
  }
  return body;
}

async function loadZone(name) {
  const zones = await request(DNS_BASE, '/v1/zones');
  const wanted = stripDot(name);
  const zone = (Array.isArray(zones) ? zones : []).find((entry) => stripDot(entry.name) === wanted);
  if (!zone) {
    const known = (Array.isArray(zones) ? zones : []).map((entry) => entry.name).join(', ') || 'none';
    throw new Error(`zone ${wanted} not found; available zones: ${known}`);
  }
  return request(DNS_BASE, `/v1/zones/${encodeURIComponent(zone.id)}`);
}

function print(value, json) {
  if (json) console.log(JSON.stringify(value, null, 2));
  else if (typeof value === 'string') console.log(value);
  else console.log(JSON.stringify(value, null, 2));
}

function recordFor(options, zone) {
  if (!options.content) throw new Error('--content is required (the IP or target value)');
  const record = {
    name: recordName(options.name ?? '@', zone.name),
    type: String(options.type ?? 'A').toUpperCase(),
    content: String(options.content).trim(),
    ttl: Number.parseInt(options.ttl ?? String(DEFAULT_TTL), 10),
  };
  if (!Number.isInteger(record.ttl) || record.ttl < 60 || record.ttl > 86_400) {
    throw new Error('--ttl must be an integer between 60 and 86400');
  }
  if (options.prio !== undefined) {
    record.prio = Number.parseInt(options.prio, 10);
    if (!Number.isInteger(record.prio) || record.prio < 0) throw new Error('--prio must be a number');
  }
  return record;
}

async function ensureRecord(zone, options) {
  const desired = recordFor(options, zone);
  const plan = planRecord(zone.records, desired);
  if (plan.action === 'ambiguous') {
    throw new Error(
      `${desired.name} already has ${plan.matches.length} ${desired.type} records; refusing to guess. ` +
        `Delete or update them manually: ${plan.matches.map((row) => row.id).join(', ')}`,
    );
  }
  if (plan.action === 'unchanged') {
    return { action: 'unchanged', name: desired.name, type: desired.type };
  }
  if (!options.apply) {
    return { action: `would_${plan.action}`, desired, plan };
  }
  if (plan.action === 'create') {
    const created = await request(DNS_BASE, `/v1/zones/${encodeURIComponent(zone.id)}/records`, {
      method: 'POST',
      body: JSON.stringify([desired]),
    });
    return { action: 'created', record: created?.[0] ?? null };
  }
  const updated = await request(
    DNS_BASE,
    `/v1/zones/${encodeURIComponent(zone.id)}/records/${encodeURIComponent(plan.id)}`,
    { method: 'PUT', body: JSON.stringify(plan.changes) },
  );
  return { action: 'updated', record: updated };
}

function usage() {
  return `IONOS Hosting API helper (DNS/SSL only)

Usage:
  IONOS_API_KEY=prefix.secret node deploy/ionos.mjs <command> [options]

Commands:
  zones                         List DNS zones in the account
  records --zone <name>         List a zone's records
  ensure --zone <name> --name <label> --type <type> --content <value>
                                Create or update one record (dry run unless --apply)
  cutover --zone <name> --ip <v4> [--names api,admin]
                                Point the API/admin names at the VPS (dry run unless --apply)
  ssl:quota                     Show purchased/unused SSL certificate quota
  ssl:list                      List certificates

Options:
  --zone <name>       DNS zone (default: askrigel.com)
  --ttl <seconds>     Record TTL (default: 300; 60..86400)
  --prio <number>     Priority for MX/SRV
  --apply             Perform the change; without it, print the plan
  --json              Machine-readable output

This script cannot order a VPS. The Hosting APIs expose domains, DNS and SSL only.
The VPS must be ordered in the IONOS account first; then use cutover with its IPv4.
Caddy obtains the public certificates from Let's Encrypt, so the IONOS SSL API is
only needed if you deliberately choose an IONOS DV certificate.`;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help || options.command === 'help') {
    console.log(usage());
    return;
  }
  if (options.command === 'zones') {
    const zones = await request(DNS_BASE, '/v1/zones');
    print(zones, options.json);
    return;
  }
  if (options.command === 'records') {
    const zone = await loadZone(options.zone ?? DEFAULT_ZONE);
    print(zone, options.json);
    return;
  }
  if (options.command === 'ensure') {
    const zone = await loadZone(options.zone ?? DEFAULT_ZONE);
    const result = await ensureRecord(zone, options);
    print(result, options.json);
    return;
  }
  if (options.command === 'cutover') {
    if (!options.ip) throw new Error('--ip is required for cutover');
    const names = String(options.names ?? 'api,admin')
      .split(',')
      .map((name) => name.trim())
      .filter(Boolean);
    const zone = await loadZone(options.zone ?? DEFAULT_ZONE);
    const results = [];
    for (const name of names) {
      // IONOS can replace record IDs when an update deactivates conflicting
      // records (for example the old website's apex/www records). Re-read the
      // zone before every applied change so the next upsert never uses a stale ID.
      const currentZone = options.apply ? await loadZone(zone.name) : zone;
      results.push(
        await ensureRecord(currentZone, {
          ...options,
          name,
          type: 'A',
          content: options.ip,
          ttl: options.ttl ?? String(DEFAULT_TTL),
        }),
      );
    }
    print({ zone: zone.name, applied: options.apply, results }, options.json);
    return;
  }
  if (options.command === 'ssl:quota') {
    print(await request(SSL_BASE, '/v1/certificates/quota'), options.json);
    return;
  }
  if (options.command === 'ssl:list') {
    print(await request(SSL_BASE, '/v1/certificates?page=1&size=100'), options.json);
    return;
  }
  throw new Error(`unknown command: ${options.command}\n\n${usage()}`);
}

const invokedDirectly =
  process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url;

if (invokedDirectly) {
  main().catch((error) => {
    console.error(`ionos: ${error.message}`);
    process.exitCode = 1;
  });
}

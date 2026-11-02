'use strict';

const crypto = require('node:crypto');

/**
 * A client line is `label:sha256-hex`, one per installation. Only the hash sits in the
 * deployment; the token itself is handed to the client out of band and kept in the OS keychain.
 */
const LABEL = /^[a-z0-9][a-z0-9-]{0,31}$/;
const HASH = /^[0-9a-f]{64}$/;
const MAX_TOKEN_BYTES = 512;

function hashToken(token) {
  return crypto.createHash('sha256').update(token, 'utf8').digest('hex');
}

/**
 * Parse `RIGEL_ATTEST_CLIENTS`. Blank lines and `#` comments are ignored, so a secret can list
 * one installation per line and still be readable. A malformed configuration is an error rather
 * than an empty allowlist: the endpoint must fail closed, not silently accept nobody (or
 * everybody).
 */
function parseClients(raw) {
  if (typeof raw !== 'string' || raw.trim() === '') {
    throw new Error('RIGEL_ATTEST_CLIENTS is empty');
  }
  const clients = [];
  const labels = new Set();
  const hashes = new Set();
  for (const line of raw.split(/\r?\n/)) {
    const text = line.trim();
    if (!text || text.startsWith('#')) continue;
    const separator = text.indexOf(':');
    if (separator <= 0) {
      throw new Error('each client line must be `label:sha256`');
    }
    const label = text.slice(0, separator).trim();
    const hash = text.slice(separator + 1).trim().toLowerCase();
    if (!LABEL.test(label)) {
      throw new Error(`invalid client label \`${label}\``);
    }
    if (!HASH.test(hash)) {
      throw new Error(`invalid SHA-256 for \`${label}\``);
    }
    if (labels.has(label)) {
      throw new Error(`duplicate client label \`${label}\``);
    }
    if (hashes.has(hash)) {
      throw new Error(`duplicate credential hash for \`${label}\``);
    }
    labels.add(label);
    hashes.add(hash);
    clients.push({ label, hash: Buffer.from(hash, 'hex') });
  }
  if (clients.length === 0) {
    throw new Error('RIGEL_ATTEST_CLIENTS has no clients');
  }
  return clients;
}

/**
 * Return the client label for a `Bearer` token, or null. Every configured digest is compared in
 * constant time and no early exit tells a caller how close a guess was. A token longer than 512
 * bytes is refused before hashing; no real credential needs that much.
 */
function verifyClient(header, clients) {
  if (typeof header !== 'string' || !Array.isArray(clients)) return null;
  const match = /^Bearer[ \t]+([^ \t]+)$/i.exec(header.trim());
  if (!match) return null;
  const token = match[1];
  if (token.length === 0 || Buffer.byteLength(token, 'utf8') > MAX_TOKEN_BYTES) return null;
  const digest = crypto.createHash('sha256').update(token, 'utf8').digest();
  let found = null;
  for (const client of clients) {
    if (crypto.timingSafeEqual(digest, client.hash)) found = client.label;
  }
  return found;
}

module.exports = { LABEL, HASH, MAX_TOKEN_BYTES, hashToken, parseClients, verifyClient };

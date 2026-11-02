'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');

const { hashToken, parseClients, verifyClient } = require('../lib/clients');

const TOKEN = 'desktop-install-0123456789abcdef';
const OTHER = 'phone-install-fedcba9876543210';

function config(entries) {
  return entries.map(([label, token]) => `${label}:${hashToken(token)}`).join('\n');
}

test('a client line maps a label to the SHA-256 of its token', () => {
  const clients = parseClients(`# installations\n\n${config([['desktop-1', TOKEN]])}\n`);
  assert.equal(clients.length, 1);
  assert.equal(clients[0].label, 'desktop-1');
  assert.equal(verifyClient(`Bearer ${TOKEN}`, clients), 'desktop-1');
  assert.equal(verifyClient(`bearer ${TOKEN}`, clients), 'desktop-1');
  assert.equal(verifyClient(`Bearer   ${TOKEN}  `, clients), 'desktop-1');
});

test('a token that is not in the allowlist is refused', () => {
  const clients = parseClients(config([['desktop-1', TOKEN]]));
  assert.equal(verifyClient(`Bearer ${OTHER}`, clients), null);
  assert.equal(verifyClient(`Bearer ${TOKEN}x`, clients), null);
  assert.equal(verifyClient(`Basic ${TOKEN}`, clients), null);
  assert.equal(verifyClient(TOKEN, clients), null);
  assert.equal(verifyClient(undefined, clients), null);
  assert.equal(verifyClient(`Bearer ${'x'.repeat(513)}`, clients), null);
});

test('two installations can hold different tokens', () => {
  const clients = parseClients(config([
    ['desktop-1', TOKEN],
    ['phone-1', OTHER],
  ]));
  assert.equal(verifyClient(`Bearer ${TOKEN}`, clients), 'desktop-1');
  assert.equal(verifyClient(`Bearer ${OTHER}`, clients), 'phone-1');
  assert.equal(verifyClient('Bearer nobody', clients), null);
});

test('a malformed allowlist is refused instead of becoming open or empty', () => {
  assert.throws(() => parseClients(''), /empty/);
  assert.throws(() => parseClients('desktop-1'), /label:sha256/);
  assert.throws(() => parseClients('desktop-1:not-a-hash'), /SHA-256/);
  assert.throws(() => parseClients(`desktop-1:${'a'.repeat(64)}\ndesktop-1:${'b'.repeat(64)}`), /duplicate client label/);
  assert.throws(() => parseClients(`desktop-1:${'a'.repeat(64)}\nphone-1:${'a'.repeat(64)}`), /duplicate credential/);
  assert.throws(() => parseClients(`Desktop:${'a'.repeat(64)}`), /invalid client label/);
  assert.throws(() => parseClients(`desktop_1:${'a'.repeat(64)}`), /invalid client label/);
});

test('hashToken is SHA-256 hex, matching the documented openssl command', () => {
  // Known vector: SHA-256("abc"). This is the same digest `printf %s abc | sha256sum` prints.
  assert.equal(
    hashToken('abc'),
    'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
  );
  assert.match(hashToken('rigel'), /^[0-9a-f]{64}$/);
});

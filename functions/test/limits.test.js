'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');

const { bucketId, createLimiter, takeToken, HOUR_MS } = require('../lib/limits');

const NOW = 1_760_961_600_000;

function memoryDb() {
  const documents = new Map();
  const db = {
    documents,
    collection: (name) => ({
      doc: (id) => ({ id, path: `${name}/${id}` }),
    }),
    runTransaction: async (body) => body({
      get: async (ref) => ({
        exists: documents.has(ref.id),
        data: () => documents.get(ref.id),
      }),
      set: (ref, value) => documents.set(ref.id, value),
    }),
  };
  return db;
}

test('a full bucket spends one token and stops at empty', () => {
  const full = takeToken(null, NOW, 2, 2);
  assert.equal(full.allowed, true);
  assert.equal(full.tokens, 1);

  const next = takeToken({ tokens: 0.1, updatedAt: NOW }, NOW, 2, 2);
  assert.equal(next.allowed, false);
  assert.ok(next.retryAfter >= 1, 'a refusal says when to come back');
});

test('a bucket refills at the configured rate and never above its burst', () => {
  const refilled = takeToken({ tokens: 0, updatedAt: NOW - HOUR_MS }, NOW, 3, 5);
  assert.equal(refilled.allowed, true);
  assert.equal(refilled.tokens, 2, 'one hour adds three, one is spent');

  const capped = takeToken({ tokens: 1, updatedAt: NOW - 24 * HOUR_MS }, NOW, 3, 5);
  assert.equal(capped.tokens, 4, 'the burst is the ceiling');
});

test('the limiter refuses a client that has spent its burst', async () => {
  const db = memoryDb();
  const limiter = createLimiter({
    db,
    client: { perHour: 1, burst: 1 },
    ip: { perHour: 100, burst: 100 },
  });

  const first = await limiter.consume({ client: 'desktop-1', ip: '203.0.113.7', now: NOW });
  assert.equal(first.allowed, true);
  const second = await limiter.consume({ client: 'desktop-1', ip: '203.0.113.7', now: NOW });
  assert.equal(second.allowed, false);
  assert.ok(second.retryAfter > 0);
});

test('a denial writes nothing, so a refused request does not consume another bucket', async () => {
  const db = memoryDb();
  const limiter = createLimiter({
    db,
    client: { perHour: 100, burst: 100 },
    ip: { perHour: 1, burst: 1 },
  });

  assert.equal(
    (await limiter.consume({ client: 'desktop-1', ip: '203.0.113.7', now: NOW })).allowed,
    true,
  );
  const before = new Map(db.documents);
  assert.equal(
    (await limiter.consume({ client: 'desktop-2', ip: '203.0.113.7', now: NOW })).allowed,
    false,
  );
  assert.deepEqual(db.documents, before, 'the denied transaction leaves both buckets untouched');
});

test('a missing address falls back to the client bucket only', async () => {
  const db = memoryDb();
  const limiter = createLimiter({
    db,
    client: { perHour: 1, burst: 1 },
    ip: { perHour: 1, burst: 1 },
  });
  assert.equal((await limiter.consume({ client: 'desktop-1', ip: null, now: NOW })).allowed, true);
  assert.equal(db.documents.size, 1);
});

test('rate-limit documents are keyed by hashes and carry an expiry', async () => {
  const db = memoryDb();
  const limiter = createLimiter({
    db,
    client: { perHour: 10, burst: 2 },
    ip: { perHour: 10, burst: 2 },
  });
  await limiter.consume({ client: 'desktop-1', ip: '203.0.113.7', now: NOW });
  assert.ok(!db.documents.has('desktop-1'));
  assert.ok(![...db.documents.keys()].some((key) => key.includes('203.0.113.7')));
  assert.ok(db.documents.has(bucketId('client', 'desktop-1')));
  for (const value of db.documents.values()) {
    assert.ok(value.expires_at instanceof Date);
    assert.ok(value.expires_at.getTime() > NOW);
  }
});

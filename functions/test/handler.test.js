'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');

const { clientIp, createAttestationHandler } = require('../lib/handler');

const NOW = 1_760_961_600_000;
const EXPIRES = Math.floor(NOW / 1000) + 3_600;

function request({ method = 'GET', authorization, ip = '203.0.113.7', forwarded } = {}) {
  const headers = {};
  if (authorization !== undefined) headers.authorization = authorization;
  if (forwarded !== undefined) headers['x-forwarded-for'] = forwarded;
  return {
    method,
    ip,
    headers,
    get(name) {
      return headers[String(name).toLowerCase()];
    },
  };
}

function response() {
  const state = { status: 0, headers: {}, body: null };
  return {
    state,
    status(code) {
      state.status = code;
      return this;
    },
    set(headers) {
      Object.assign(state.headers, headers);
      return this;
    },
    send(text) {
      state.body = JSON.parse(text);
      return this;
    },
  };
}

function harness(overrides = {}) {
  const calls = { consume: [], mint: 0 };
  const logs = [];
  const handler = createAttestationHandler({
    configured: true,
    verifyClient: (header) => (header === 'Bearer good' ? 'desktop-1' : null),
    consume: async (input) => {
      calls.consume.push(input);
      return { allowed: true, retryAfter: 0 };
    },
    mint: async () => {
      calls.mint += 1;
      return { token: 'minted-token', expiresAt: EXPIRES };
    },
    now: () => NOW,
    log: (message) => logs.push(message),
    ...overrides,
  });
  return { handler, calls, logs };
}

test('an accepted installation gets a token and a Unix-second expiry', async () => {
  const { handler, calls } = harness();
  const res = response();
  await handler(request({ authorization: 'Bearer good' }), res);

  assert.equal(res.state.status, 200);
  assert.deepEqual(res.state.body, { token: 'minted-token', expires_at: EXPIRES });
  assert.equal(res.state.headers['cache-control'], 'no-store, max-age=0');
  assert.equal(calls.mint, 1);
  assert.deepEqual(calls.consume, [{ client: 'desktop-1', ip: '203.0.113.7', now: NOW }]);
});

test('a missing or wrong credential is refused before the limiter and the mint', async () => {
  const { handler, calls } = harness();
  for (const authorization of [undefined, 'Bearer wrong', 'Basic good', 'Bearer good extra']) {
    const res = response();
    await handler(request({ authorization }), res);
    assert.equal(res.state.status, 401, String(authorization));
    assert.deepEqual(res.state.body, { error: 'not attested' });
    assert.match(res.state.headers['www-authenticate'], /Bearer/);
    assert.ok(!JSON.stringify(res.state.body).includes('good'));
  }
  assert.equal(calls.consume.length, 0);
  assert.equal(calls.mint, 0);
});

test('an exhausted bucket answers 429 with Retry-After and does not mint', async () => {
  const { handler, calls } = harness({
    consume: async () => ({ allowed: false, retryAfter: 17 }),
  });
  const res = response();
  await handler(request({ authorization: 'Bearer good' }), res);

  assert.equal(res.state.status, 429);
  assert.equal(res.state.headers['retry-after'], '17');
  assert.deepEqual(res.state.body, { error: 'rate limited', retry_after: 17 });
  assert.equal(calls.mint, 0);
});

test('only GET is served', async () => {
  for (const method of ['POST', 'PUT', 'HEAD']) {
    const { handler } = harness();
    const res = response();
    await handler(request({ method, authorization: 'Bearer good' }), res);
    assert.equal(res.state.status, 405);
    assert.equal(res.state.headers.allow, 'GET');
  }
});

test('the last forwarded hop is the address Google saw, not a caller-supplied one', () => {
  assert.equal(
    clientIp(request({ forwarded: '198.51.100.9, 10.0.0.2' })),
    '10.0.0.2',
    'the caller can prepend a fake hop, but not the one Google appends',
  );
  assert.equal(
    clientIp(request({ forwarded: '203.0.113.99, 198.51.100.9, 10.0.0.2' })),
    '10.0.0.2',
  );
  assert.equal(clientIp(request({ ip: 'not-an-ip' })), null);
  assert.equal(clientIp(request({ forwarded: 'garbage', ip: '2001:db8::1' })), '2001:db8::1');
});

test('a mint failure is a generic 500 and never logs the caller credential', async () => {
  const { handler, logs } = harness({
    mint: async () => {
      throw Object.assign(new Error('permission denied for token secret'), { code: 'PERMISSION_DENIED' });
    },
  });
  const res = response();
  await handler(request({ authorization: 'Bearer good' }), res);

  assert.equal(res.state.status, 500);
  assert.deepEqual(res.state.body, { error: 'attestation failed' });
  assert.ok(logs.some((line) => line.includes('PERMISSION_DENIED')));
  assert.ok(!logs.join('\n').includes('Bearer good'));
  assert.ok(!logs.join('\n').includes('token secret'));
});

test('a rate-limit store failure fails closed', async () => {
  const { handler, calls } = harness({
    consume: async () => {
      throw Object.assign(new Error('firestore unavailable'), { code: '14' });
    },
  });
  const res = response();
  await handler(request({ authorization: 'Bearer good' }), res);
  assert.equal(res.state.status, 500);
  assert.equal(calls.mint, 0);
});

test('an unconfigured deployment answers 503 without checking a credential', async () => {
  const { handler, calls } = harness({ configured: false });
  const res = response();
  await handler(request({ authorization: 'Bearer good' }), res);
  assert.equal(res.state.status, 503);
  assert.equal(calls.consume.length, 0);
  assert.equal(calls.mint, 0);
});

test('a bad Admin SDK answer is not passed through', async () => {
  for (const mint of [
    async () => ({ token: '', expiresAt: EXPIRES }),
    async () => ({ token: 'x', expiresAt: Math.floor(NOW / 1000) - 1 }),
    async () => ({ token: 'x', expiresAt: 1.5 }),
  ]) {
    const { handler } = harness({ mint });
    const res = response();
    await handler(request({ authorization: 'Bearer good' }), res);
    assert.equal(res.state.status, 500);
  }
});

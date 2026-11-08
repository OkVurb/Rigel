'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');

const { PLACEMENT, createGravityHandler, publicAd } = require('../lib/gravity');

const ORIGIN = 'https://rigel-25128.web.app';
const BODY = {
  messages: [
    { role: 'user', content: 'How do I run several coding agents on one project?' },
    { role: 'assistant', content: 'Give each agent a worktree and keep one journal.' },
  ],
  gravity_context: {
    sessionId: 'sess-review-1',
    user: { id: 'install-7' },
    device: { ua: 'Mozilla/5.0', viewport_width: 1200, timezone: 'Europe/London' },
  },
};

const AD = {
  adText: 'Serverless Postgres that scales to zero.',
  title: 'Ship a database this afternoon',
  brandName: 'Neon',
  cta: 'Try Neon Free',
  url: 'https://neon.tech',
  clickUrl: 'https://api.trygravity.ai/track/click?p=abc',
  impUrl: 'https://api.trygravity.ai/ack?p=abc',
  placement: 'below_response',
  placement_id: 'rigel-sponsor',
  internalScore: 0.99,
};

function request({ method = 'POST', origin = ORIGIN, body = BODY, forwarded = '198.51.100.9, 10.0.0.2' } = {}) {
  const headers = { 'content-type': 'application/json' };
  if (origin) headers.origin = origin;
  if (forwarded) headers['x-forwarded-for'] = forwarded;
  return {
    method,
    headers,
    body,
    get(name) {
      return headers[String(name).toLowerCase()];
    },
  };
}

function response() {
  const state = { status: 0, headers: {}, body: null, ended: false };
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
    json(body) {
      state.body = body;
      return this;
    },
    end() {
      state.ended = true;
      return this;
    },
  };
}

function harness(overrides = {}) {
  const calls = { getAds: [] };
  const logs = [];
  const handler = createGravityHandler({
    test: true,
    getAds: async (...args) => {
      calls.getAds.push(args);
      return { ads: [AD], status: 200, elapsed: '12ms', requestBody: {} };
    },
    log: (message) => logs.push(message),
    ...overrides,
  });
  return { handler, calls, logs };
}

test('the website origin gets one sanitised ad for the shared placement', async () => {
  const { handler, calls } = harness();
  const res = response();
  await handler(request(), res);

  assert.equal(res.state.status, 200);
  assert.equal(res.state.headers['access-control-allow-origin'], ORIGIN);
  assert.equal(res.state.headers['cache-control'], 'no-store, max-age=0');
  assert.equal(res.state.headers.vary, 'Origin');
  assert.deepEqual(res.state.body, {
    ads: [
      {
        adText: AD.adText,
        title: AD.title,
        brandName: AD.brandName,
        cta: AD.cta,
        url: AD.url,
        clickUrl: AD.clickUrl,
        impUrl: AD.impUrl,
        placement: AD.placement,
        placement_id: AD.placement_id,
      },
    ],
    test: true,
  });
  assert.equal(res.state.body.ads[0].internalScore, undefined);

  const [incoming, messages, placements] = calls.getAds[0];
  assert.deepEqual(placements, [PLACEMENT]);
  assert.equal(messages.length, 2);
  assert.equal(incoming.body.gravity_context.sessionId, 'sess-review-1');
  assert.equal(incoming.body.gravity_context.user.id, 'install-7');
  assert.equal(incoming.body.gravity_context.device.viewport_width, 1200);
  assert.equal(incoming.headers['x-forwarded-for'], '10.0.0.2');
});

test('the askrigel.com production origins are accepted by the Gravity broker', async () => {
  for (const origin of ['https://askrigel.com', 'https://www.askrigel.com']) {
    const { handler, calls } = harness();
    const res = response();
    await handler(request({ origin }), res);
    assert.equal(res.state.status, 200, origin);
    assert.equal(res.state.headers['access-control-allow-origin'], origin);
    assert.equal(calls.getAds.length, 1);
  }
});

test('the desktop flat body and the website context body share one contract', async () => {
  const { handler, calls } = harness();
  const res = response();
  await handler(
    request({
      body: {
        sessionId: 'rigel-sponsor:abc',
        user: { id: 'install-9', tags: { plan: 'free' } },
        device: { ua: 'Rigel', os: 'Windows' },
        messages: [{ role: 'user', content: 'What is a good database for a side project?' }],
      },
    }),
    res,
  );

  assert.equal(res.state.status, 200);
  const [incoming] = calls.getAds[0];
  assert.equal(incoming.body.gravity_context.sessionId, 'rigel-sponsor:abc');
  assert.deepEqual(incoming.body.gravity_context.user, { id: 'install-9', tags: { plan: 'free' } });
  assert.equal(incoming.body.gravity_context.device.os, 'Windows');
});

test('a preflight is answered without touching Gravity', async () => {
  const { handler, calls } = harness();
  const res = response();
  await handler(request({ method: 'OPTIONS' }), res);

  assert.equal(res.state.status, 204);
  assert.equal(res.state.headers['access-control-allow-origin'], ORIGIN);
  assert.match(res.state.headers['access-control-allow-methods'], /POST/);
  assert.equal(calls.getAds.length, 0);
});

test('a foreign browser origin and a non-POST method are refused', async () => {
  const { handler, calls } = harness();
  const foreign = response();
  await handler(request({ origin: 'https://evil.example' }), foreign);
  assert.equal(foreign.state.status, 403);
  assert.equal(foreign.state.headers['access-control-allow-origin'], undefined);
  assert.equal(calls.getAds.length, 0);

  const get = response();
  await handler(request({ method: 'GET' }), get);
  assert.equal(get.state.status, 405);
  assert.equal(get.state.headers.allow, 'POST, OPTIONS');
});

test('missing context, no messages and an oversized body are rejected before the SDK', async () => {
  const { handler, calls } = harness();

  for (const body of [
    {},
    { messages: [], gravity_context: BODY.gravity_context },
    { messages: BODY.messages, gravity_context: { user: { id: 'x' } } },
    { messages: BODY.messages, gravity_context: { sessionId: 's', user: { id: '' } } },
  ]) {
    const res = response();
    await handler(request({ body }), res);
    assert.equal(res.state.status, 400, JSON.stringify(body));
  }

  const large = response();
  await handler(request({ body: { ...BODY, padding: 'x'.repeat(25_000) } }), large);
  assert.equal(large.state.status, 413);
  assert.equal(calls.getAds.length, 0);
});

test('an ad with no https click URL is not returned', async () => {
  assert.equal(publicAd({ ...AD, clickUrl: 'http://api.trygravity.ai/click' }), null);
  assert.equal(publicAd({ ...AD, clickUrl: 'not a url' }), null);

  const { handler } = harness({
    getAds: async () => ({
      ads: [{ ...AD, clickUrl: 'javascript:alert(1)' }],
      status: 200,
      elapsed: '1ms',
      requestBody: {},
    }),
  });
  const res = response();
  await handler(request(), res);
  assert.deepEqual(res.state.body.ads, []);
});

test('a missing secret is a generic 503 and never leaks the reason', async () => {
  const { handler, logs } = harness({
    getAds: async () => ({ ads: [], status: 0, elapsed: '0ms', requestBody: null }),
  });
  const res = response();
  await handler(request(), res);

  assert.equal(res.state.status, 503);
  assert.deepEqual(res.state.body, { error: 'ads_unavailable', test: true });
  assert.ok(!JSON.stringify(res.state.body).includes('key'));
  assert.ok(logs.length > 0);
});

test('a Gravity error is a quiet no-fill, not a broken page', async () => {
  const { handler } = harness({
    getAds: async () => ({ ads: [], status: 403, elapsed: '5ms', requestBody: {} }),
  });
  const res = response();
  await handler(request(), res);
  assert.equal(res.state.status, 200);
  assert.deepEqual(res.state.body, { ads: [], test: true });
});

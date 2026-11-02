'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');

const { assess, projectUrl } = require('../lib/recaptcha');

const PROJECT = 'rigel-25128';
const KEY = 'test-key';

function fakeFetch(handler) {
  const calls = [];
  const fn = async (url, init) => {
    calls.push({ url, init, body: init && init.body ? JSON.parse(init.body) : null });
    return handler(url, init);
  };
  fn.calls = calls;
  return fn;
}

const respond = (status, body) => async () => ({
  ok: status >= 200 && status < 300,
  status,
  json: async () => body,
});

test('posts the token to the project assessments endpoint', async () => {
  const fetchImpl = fakeFetch(
    respond(200, { tokenProperties: { valid: true, action: 'contact' }, riskAnalysis: { score: 0.9 } }),
  );
  const result = await assess({ projectId: PROJECT, apiKey: KEY, siteKey: 'site', token: 'x'.repeat(40), fetchImpl });

  assert.deepEqual(result, { ok: true, score: 0.9 });
  assert.equal(fetchImpl.calls.length, 1);
  assert.equal(
    fetchImpl.calls[0].url,
    `${projectUrl(PROJECT)}?key=${KEY}`,
  );
  assert.equal(fetchImpl.calls[0].body.event.expectedAction, 'contact');
  assert.equal(fetchImpl.calls[0].body.event.siteKey, 'site');
});

test('refuses to call Google without an API key', async () => {
  const fetchImpl = fakeFetch(respond(200, {}));
  const result = await assess({ projectId: PROJECT, apiKey: '', siteKey: 'site', token: 'x'.repeat(40), fetchImpl });

  assert.equal(result.ok, false);
  assert.equal(result.status, 503);
  assert.equal(result.reason, 'not_configured');
  assert.equal(fetchImpl.calls.length, 0);
});

test('authenticates with a service-account token when no key is configured', async () => {
  const fetchImpl = fakeFetch(respond(200, { tokenProperties: { valid: true, action: 'contact' } }));
  const result = await assess({
    projectId: PROJECT,
    accessToken: 'ya29.test',
    siteKey: 'site',
    token: 'x'.repeat(40),
    fetchImpl,
  });

  assert.equal(result.ok, true);
  assert.equal(fetchImpl.calls[0].url, projectUrl(PROJECT));
  assert.equal(fetchImpl.calls[0].init.headers.Authorization, 'Bearer ya29.test');
});

test('rejects a short or missing token before spending a request', async () => {
  const fetchImpl = fakeFetch(respond(200, {}));
  for (const token of [undefined, '', 'short']) {
    const result = await assess({ projectId: PROJECT, apiKey: KEY, siteKey: 'site', token, fetchImpl });
    assert.equal(result.status, 400);
  }
  assert.equal(fetchImpl.calls.length, 0);
});

test('an invalid token is a 403, not a server error', async () => {
  const fetchImpl = fakeFetch(respond(200, { tokenProperties: { valid: false } }));
  const result = await assess({ projectId: PROJECT, apiKey: KEY, siteKey: 'site', token: 'x'.repeat(40), fetchImpl });

  assert.deepEqual(result, { ok: false, status: 403, reason: 'invalid_token' });
});

test('a token minted for another action is refused', async () => {
  const fetchImpl = fakeFetch(
    respond(200, { tokenProperties: { valid: true, action: 'signup' } }),
  );
  const result = await assess({ projectId: PROJECT, apiKey: KEY, siteKey: 'site', token: 'x'.repeat(40), fetchImpl });

  assert.equal(result.status, 403);
  assert.equal(result.reason, 'action_mismatch');
});

test('a low score is refused, a missing score is not', async () => {
  const low = fakeFetch(
    respond(200, { tokenProperties: { valid: true, action: 'contact' }, riskAnalysis: { score: 0.1 } }),
  );
  assert.equal(
    (await assess({ projectId: PROJECT, apiKey: KEY, siteKey: 'site', token: 'x'.repeat(40), fetchImpl: low })).reason,
    'low_score',
  );

  const unscored = fakeFetch(respond(200, { tokenProperties: { valid: true, action: 'contact' } }));
  assert.equal(
    (await assess({ projectId: PROJECT, apiKey: KEY, siteKey: 'site', token: 'x'.repeat(40), fetchImpl: unscored })).ok,
    true,
  );
});

test('a Google outage is a 502 the browser can retry', async () => {
  const down = fakeFetch(async () => {
    throw new Error('ECONNRESET');
  });
  assert.equal(
    (await assess({ projectId: PROJECT, apiKey: KEY, siteKey: 'site', token: 'x'.repeat(40), fetchImpl: down })).status,
    502,
  );

  const http = fakeFetch(respond(429, {}));
  const result = await assess({ projectId: PROJECT, apiKey: KEY, siteKey: 'site', token: 'x'.repeat(40), fetchImpl: http });
  assert.equal(result.status, 502);
  assert.equal(result.reason, 'assess_http_429');
});

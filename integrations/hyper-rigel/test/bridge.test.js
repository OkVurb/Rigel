'use strict';

// Protocol tests for the Hyper bridge. They drive the same handler the HTTP server uses, plus a
// real server on an ephemeral port for the transport rules. No Hyper install is required.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { __internals } = require('../index.js');
const { createBridge } = __internals;

const TOKEN = 'a'.repeat(64);
const JSON_TYPE = 'application/json';

function configFile({ allowCommands = false, token = TOKEN } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'hyper-rigel-test-'));
  const file = path.join(dir, 'hyper-bridge.json');
  fs.writeFileSync(file, JSON.stringify({ token, allowCommands }));
  return { dir, file };
}

/** A stand-in for Hyper's main process: one window whose first session records writes. */
function fakeApp() {
  const writes = [];
  let windows = 0;
  return {
    writes,
    get windows() {
      return windows;
    },
    createWindow() {
      windows += 1;
      const session = { write: (data) => writes.push(data) };
      return { sessions: new Map([['session-1', session]]) };
    },
  };
}

function bridgeFor(app, file) {
  return createBridge({ app, env: { RIGEL_HYPER_CONFIG: file }, settleMs: 0, log: () => {} });
}

function openRequest(body, headers = {}) {
  return {
    method: 'POST',
    url: '/open',
    headers: { 'content-type': JSON_TYPE, 'x-rigel-token': TOKEN, ...headers },
    body: JSON.stringify(body),
  };
}

test('a path request opens a window and types cd', async () => {
  const { file } = configFile();
  const app = fakeApp();
  const bridge = bridgeFor(app, file);

  const result = await bridge.handle(openRequest({ path: 'C:\\work\\rigel' }));

  assert.equal(result.status, 200);
  assert.equal(result.payload.ok, true);
  assert.equal(result.payload.command, false);
  assert.equal(app.windows, 1);
  assert.deepEqual(app.writes, ['cd "C:\\work\\rigel"\r']);
});

test('the token is required and compared exactly', async () => {
  const { file } = configFile();
  const app = fakeApp();
  const bridge = bridgeFor(app, file);

  for (const token of [undefined, '', 'b'.repeat(64), TOKEN.slice(0, 32)]) {
    const headers = { 'content-type': JSON_TYPE };
    if (token !== undefined) headers['x-rigel-token'] = token;
    const result = await bridge.handle({
      method: 'POST',
      url: '/open',
      headers,
      body: JSON.stringify({ path: 'C:\\work' }),
    });
    assert.equal(result.status, 401, `token ${String(token)} must be refused`);
  }
  assert.equal(app.windows, 0);
  assert.equal(app.writes.length, 0);
});

test('a browser origin is refused before the token is even checked', async () => {
  const { file } = configFile();
  const bridge = bridgeFor(fakeApp(), file);
  const result = await bridge.handle(
    openRequest({ path: 'C:\\work' }, { origin: 'https://evil.example' }),
  );
  assert.equal(result.status, 403);
  assert.match(result.payload.error, /origin/i);
});

test('only POST /open with JSON is served', async () => {
  const { file } = configFile();
  const bridge = bridgeFor(fakeApp(), file);

  assert.equal((await bridge.handle({ method: 'GET', url: '/open', headers: {} })).status, 404);
  assert.equal(
    (await bridge.handle({ method: 'POST', url: '/other', headers: {}, body: '{}' })).status,
    404,
  );
  assert.equal(
    (
      await bridge.handle({
        method: 'POST',
        url: '/open',
        headers: { 'content-type': 'text/plain', 'x-rigel-token': TOKEN },
        body: JSON.stringify({ path: 'C:\\work' }),
      })
    ).status,
    415,
  );
});

test('without a config the bridge reports itself unconfigured', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'hyper-rigel-test-'));
  const bridge = bridgeFor(fakeApp(), path.join(dir, 'missing.json'));
  const result = await bridge.handle(openRequest({ path: 'C:\\work' }));
  assert.equal(result.status, 503);
  assert.match(result.payload.error, /rigel shell hyper/);
});

test('a relative path is refused', async () => {
  const { file } = configFile();
  const bridge = bridgeFor(fakeApp(), file);
  const result = await bridge.handle(openRequest({ path: 'relative/dir' }));
  assert.equal(result.status, 400);
  assert.match(result.payload.error, /absolute/);
});

test('a path that could escape the quoted cd is refused', async () => {
  const { file } = configFile();
  const bridge = bridgeFor(fakeApp(), file);
  for (const target of ['C:\\work" & calc', 'C:\\work`id`', 'C:\\$env:TEMP']) {
    const result = await bridge.handle(openRequest({ path: target }));
    assert.equal(result.status, 400, target);
  }
});

test('a command is refused until the user enables commands', async () => {
  const { file } = configFile({ allowCommands: false });
  const app = fakeApp();
  const bridge = bridgeFor(app, file);

  const result = await bridge.handle(openRequest({ path: 'C:\\work', command: 'npm test' }));

  assert.equal(result.status, 403);
  assert.match(result.payload.error, /allow-commands/);
  assert.equal(app.windows, 0);
});

test('an enabled command is typed into the visible shell', async () => {
  const { file } = configFile({ allowCommands: true });
  const app = fakeApp();
  const bridge = bridgeFor(app, file);

  const result = await bridge.handle(openRequest({ path: 'C:\\work', command: 'npm test' }));

  assert.equal(result.status, 200);
  assert.equal(result.payload.command, true);
  assert.deepEqual(app.writes, ['cd "C:\\work"\r', 'npm test\r']);
});

test('a multi-line command cannot smuggle a second command', async () => {
  const { file } = configFile({ allowCommands: true });
  const bridge = bridgeFor(fakeApp(), file);
  const result = await bridge.handle(
    openRequest({ path: 'C:\\work', command: 'npm test\nrm -rf /' }),
  );
  assert.equal(result.status, 400);
  assert.match(result.payload.error, /single line/);
});

test('the HTTP transport enforces size, auth and success', async () => {
  const { file } = configFile();
  const app = fakeApp();
  const bridge = bridgeFor(app, file);
  const server = bridge.start(0);
  await new Promise((resolve) => server.once('listening', resolve));
  const port = server.address().port;

  try {
    const unauthorized = await fetch(`http://127.0.0.1:${port}/open`, {
      method: 'POST',
      headers: { 'content-type': JSON_TYPE },
      body: JSON.stringify({ path: 'C:\\work' }),
    });
    assert.equal(unauthorized.status, 401);

    const tooLarge = await fetch(`http://127.0.0.1:${port}/open`, {
      method: 'POST',
      headers: { 'content-type': JSON_TYPE, 'x-rigel-token': TOKEN },
      body: JSON.stringify({ path: 'C:\\work', padding: 'x'.repeat(9000) }),
    });
    assert.equal(tooLarge.status, 413);

    const ok = await fetch(`http://127.0.0.1:${port}/open`, {
      method: 'POST',
      headers: { 'content-type': JSON_TYPE, 'x-rigel-token': TOKEN },
      body: JSON.stringify({ path: 'C:\\work' }),
    });
    assert.equal(ok.status, 200);
    assert.equal((await ok.json()).ok, true);
    assert.deepEqual(app.writes, ['cd "C:\\work"\r']);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
});

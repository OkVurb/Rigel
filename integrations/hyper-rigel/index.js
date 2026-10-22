'use strict';

// Hyper's main process loads this file. It exposes one loopback-only endpoint, POST /open,
// which opens a new Hyper window, changes into a project directory and - only when the user
// has enabled it - types one command into that shell.
//
// Why a plugin: Hyper's own entry point accepts no command line arguments (it prints
// "Hyper does not accept any command line arguments" and exits for anything but --version),
// so an external launcher cannot even choose a directory. The plugin is the only supported
// way in, and it uses the documented surface: app.createWindow(), the window's `sessions`
// map and Session#write.
//
// Threat model, stated plainly: this is not isolation from other processes running as the
// same user. It *is* isolation from web pages and other users:
//   - bind to 127.0.0.1 only, never 0.0.0.0;
//   - a bearer token from the Rigel data directory, compared in constant time;
//   - any request carrying an Origin header is refused, so a browser cannot reach it;
//   - application/json is required, and no CORS headers are ever sent;
//   - small body cap, one route, no shell on our side: the command is typed into the
//     terminal the user can see, not executed by the plugin.

const http = require('node:http');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');

const HOST = '127.0.0.1';
const DEFAULT_PORT = 20130;
const MAX_BODY_BYTES = 8 * 1024;
const MAX_COMMAND_CHARS = 2000;
const SESSION_TIMEOUT_MS = 10000;
const SETTLE_MS = 800;

/** Where the CLI writes {token, allowCommands}. Keep in step with rigel-cli's shell module. */
function bridgeConfigPath(env = process.env) {
  if (env.RIGEL_HYPER_CONFIG) return env.RIGEL_HYPER_CONFIG;
  const base = env.APPDATA || env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config');
  return path.join(base, 'Rigel', 'hyper-bridge.json');
}

function loadBridgeConfig(env = process.env) {
  let parsed;
  try {
    parsed = JSON.parse(fs.readFileSync(bridgeConfigPath(env), 'utf8'));
  } catch {
    return null;
  }
  if (typeof parsed.token !== 'string' || parsed.token.length < 32) return null;
  return { token: parsed.token, allowCommands: parsed.allowCommands === true };
}

function tokenMatches(given, expected) {
  if (typeof given !== 'string') return false;
  const a = Buffer.from(given, 'utf8');
  const b = Buffer.from(expected, 'utf8');
  return a.length === b.length && crypto.timingSafeEqual(a, b);
}

function quotePath(target) {
  return `"${target}"`;
}

/** Validate the request body. Returns {ok, path, command} or {status, error}. */
function validateOpen(body, config) {
  if (!body || typeof body !== 'object' || Array.isArray(body)) {
    return { status: 400, error: 'body must be an object' };
  }
  const target = typeof body.path === 'string' ? body.path : '';
  if (!target || !path.isAbsolute(target)) {
    return { status: 400, error: 'path must be absolute' };
  }
  if (/[\u0000-\u001f]/.test(target)) {
    return { status: 400, error: 'path must not contain control characters' };
  }
  // These are the characters that would escape the quotes in a POSIX shell or expand as a
  // PowerShell variable. Rejecting them keeps the typed `cd` a single literal argument.
  if (/["`$]/.test(target)) {
    return { status: 400, error: 'path contains an unsupported character' };
  }

  let command = null;
  if (body.command !== undefined && body.command !== null) {
    if (typeof body.command !== 'string' || !body.command.trim()) {
      return { status: 400, error: 'command must be a non-empty string' };
    }
    if (body.command.length > MAX_COMMAND_CHARS) {
      return { status: 400, error: 'command is too long' };
    }
    if (/[\u0000\r\n]/.test(body.command)) {
      return { status: 400, error: 'command must be a single line' };
    }
    if (!config.allowCommands) {
      return {
        status: 403,
        error: 'commands are disabled; run `rigel shell hyper --allow-commands` once to enable them',
      };
    }
    command = body.command;
  }

  return { ok: true, path: target, command };
}

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function sessionsOf(win) {
  if (!win || !win.sessions || typeof win.sessions.values !== 'function') return [];
  return Array.from(win.sessions.values());
}

async function waitForSession(win, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const sessions = sessionsOf(win).filter((session) => typeof session.write === 'function');
    if (sessions.length > 0) return sessions[sessions.length - 1];
    if (Date.now() >= deadline) return null;
    await delay(50);
  }
}

/** Open a window, wait for its shell, then type `cd <path>` and optionally the command. */
async function openTerminal(app, request, options = {}) {
  const settleMs = options.settleMs ?? SETTLE_MS;
  const sessionTimeoutMs = options.sessionTimeoutMs ?? SESSION_TIMEOUT_MS;
  const win = app.createWindow();
  const session = await waitForSession(win, sessionTimeoutMs);
  if (!session) throw new Error('the new window never started a shell');
  // The shell needs a moment to reach its prompt; PowerShell discards type-ahead before it.
  await delay(settleMs);
  session.write(`cd ${quotePath(request.path)}\r`);
  if (request.command) {
    await delay(120);
    session.write(`${request.command}\r`);
  }
  return { command: Boolean(request.command) };
}

function send(res, status, payload) {
  const body = JSON.stringify(payload);
  res.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'content-length': Buffer.byteLength(body),
    'cache-control': 'no-store',
    'x-content-type-options': 'nosniff',
  });
  res.end(body);
}

function createBridge({ app, env = process.env, log = () => {}, settleMs = SETTLE_MS } = {}) {
  let server = null;
  let port = null;

  async function handle(request) {
    const { method, url, headers = {}, body = '' } = request;
    const route = String(url || '').split('?')[0];
    if (method !== 'POST' || route !== '/open') {
      return { status: 404, payload: { error: 'not found' } };
    }
    if (headers.origin) {
      return { status: 403, payload: { error: 'browser origins are not allowed' } };
    }
    const contentType = String(headers['content-type'] || '')
      .split(';')[0]
      .trim()
      .toLowerCase();
    if (contentType !== 'application/json') {
      return { status: 415, payload: { error: 'content-type must be application/json' } };
    }
    const config = loadBridgeConfig(env);
    if (!config) {
      return {
        status: 503,
        payload: { error: 'no bridge config; run `rigel shell hyper` once to create it' },
      };
    }
    if (!tokenMatches(headers['x-rigel-token'], config.token)) {
      return { status: 401, payload: { error: 'invalid token' } };
    }

    let parsed;
    try {
      parsed = JSON.parse(body || '{}');
    } catch {
      return { status: 400, payload: { error: 'body must be JSON' } };
    }
    const check = validateOpen(parsed, config);
    if (!check.ok) return { status: check.status, payload: { error: check.error } };

    try {
      const opened = await openTerminal(app, check, { settleMs });
      return { status: 200, payload: { ok: true, path: check.path, command: opened.command } };
    } catch (error) {
      return { status: 500, payload: { error: String(error && error.message) } };
    }
  }

  function serve(req, res) {
    const chunks = [];
    let size = 0;
    let tooLarge = false;
    req.on('data', (chunk) => {
      size += chunk.length;
      if (size > MAX_BODY_BYTES) tooLarge = true;
      else chunks.push(chunk);
    });
    req.on('error', () => {
      if (!res.headersSent) send(res, 400, { error: 'could not read request' });
    });
    req.on('end', () => {
      if (tooLarge) {
        send(res, 413, { error: 'request body too large' });
        return;
      }
      handle({
        method: req.method,
        url: req.url,
        headers: req.headers,
        body: Buffer.concat(chunks).toString('utf8'),
      })
        .then(({ status, payload }) => {
          log(`${req.method} ${req.url} -> ${status}`);
          send(res, status, payload);
        })
        .catch((error) => {
          log(`handler error: ${error && error.message}`);
          send(res, 500, { error: 'internal error' });
        });
    });
  }

  function start(requestedPort = Number(env.RIGEL_HYPER_PORT || DEFAULT_PORT)) {
    if (server) return server;
    server = http.createServer(serve);
    server.on('error', (error) => log(`listen failed: ${error.message}`));
    server.listen(requestedPort, HOST, () => {
      const address = server.address();
      port = address && typeof address === 'object' ? address.port : requestedPort;
      log(`listening on ${HOST}:${port}`);
    });
    return server;
  }

  function close() {
    if (server) {
      server.close();
      server = null;
    }
  }

  return {
    handle,
    start,
    close,
    get port() {
      return port;
    },
  };
}

let bridge = null;

/** Called by Hyper when the app is ready. */
function onApp(app) {
  if (bridge) return;
  bridge = createBridge({ app, log: (message) => console.log('[hyper-rigel]', message) });
  bridge.start();
}

/** Called by Hyper when the plugin is removed or reloaded. */
function onUnload() {
  if (bridge) {
    bridge.close();
    bridge = null;
  }
}

module.exports = {
  onApp,
  onUnload,
  __internals: {
    bridgeConfigPath,
    loadBridgeConfig,
    tokenMatches,
    validateOpen,
    createBridge,
    openTerminal,
    MAX_COMMAND_CHARS,
  },
};

'use strict';

const net = require('node:net');

const JSON_TYPE = 'application/json; charset=utf-8';
const MAX_RETRY_SECONDS = 24 * 60 * 60;

function respond(res, status, body, headers = {}) {
  res.status(status);
  res.set({
    'cache-control': 'no-store, max-age=0',
    'content-type': JSON_TYPE,
    'x-content-type-options': 'nosniff',
    ...headers,
  });
  res.send(JSON.stringify(body));
}

/**
 * The client address. Google's front end appends the address it saw to
 * `X-Forwarded-For`, so the **last** entry is the one the caller cannot choose; earlier entries
 * may have been supplied by the caller. Fall back to the socket address if the header is absent.
 */
function clientIp(req) {
  const forwarded = req.headers ? req.headers['x-forwarded-for'] : undefined;
  const hops = Array.isArray(forwarded)
    ? forwarded
    : String(forwarded || '').split(',');
  for (let index = hops.length - 1; index >= 0; index -= 1) {
    const address = String(hops[index] || '').trim();
    if (net.isIP(address)) return address;
  }
  for (const candidate of [req.ip, req.socket && req.socket.remoteAddress]) {
    const address = String(candidate || '').trim();
    if (net.isIP(address)) return address;
  }
  return null;
}

function describe(error) {
  if (!error) return 'unknown error';
  return error.code || error.name || 'error';
}

/**
 * The endpoint minting App Check tokens. The checks are injected so the tests can exercise a
 * missing credential, an exhausted bucket and a mint failure without Firestore or Google.
 *
 * The order matters: the credential is verified before any store is touched, so an unauthenticated
 * caller cannot write rate-limit documents; only an accepted installation is rate limited.
 */
function createAttestationHandler({ configured, verifyClient, consume, mint, now = Date.now, log = () => {} }) {
  return async function attestation(req, res) {
    if (req.method !== 'GET') {
      respond(res, 405, { error: 'method not allowed' }, { allow: 'GET' });
      return;
    }
    if (!configured) {
      respond(res, 503, { error: 'attestation is not configured' });
      return;
    }

    const authorization = typeof req.get === 'function' ? req.get('authorization') : undefined;
    const client = verifyClient(authorization);
    if (!client) {
      respond(res, 401, { error: 'not attested' }, {
        'www-authenticate': 'Bearer realm="rigel-app-check"',
      });
      return;
    }

    let decision;
    try {
      decision = await consume({ client, ip: clientIp(req), now: now() });
    } catch (error) {
      log(`app-check: rate limiter failed (${describe(error)})`);
      respond(res, 500, { error: 'attestation failed' });
      return;
    }
    if (!decision || decision.allowed !== true) {
      const retryAfter = Math.min(
        MAX_RETRY_SECONDS,
        Math.max(1, Math.floor(Number(decision && decision.retryAfter) || 60)),
      );
      respond(res, 429, { error: 'rate limited', retry_after: retryAfter }, {
        'retry-after': String(retryAfter),
      });
      return;
    }

    let minted;
    try {
      minted = await mint();
    } catch (error) {
      log(`app-check: token minting failed (${describe(error)})`);
      respond(res, 500, { error: 'attestation failed' });
      return;
    }
    if (!minted || typeof minted.token !== 'string' || minted.token.trim() === '') {
      log('app-check: the Admin SDK returned an empty token');
      respond(res, 500, { error: 'attestation failed' });
      return;
    }
    if (!Number.isSafeInteger(minted.expiresAt) || minted.expiresAt <= Math.floor(now() / 1000)) {
      log('app-check: the Admin SDK returned an unusable expiry');
      respond(res, 500, { error: 'attestation failed' });
      return;
    }

    log(`app-check: issued client=${client}`);
    respond(res, 200, { token: minted.token, expires_at: minted.expiresAt });
  };
}

module.exports = { clientIp, createAttestationHandler, respond };

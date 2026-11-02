'use strict';

const { getApp, initializeApp } = require('firebase-admin/app');
const { getAppCheck } = require('firebase-admin/app-check');
const { getFirestore, FieldValue } = require('firebase-admin/firestore');
const { onRequest } = require('firebase-functions/v2/https');
const { defineSecret } = require('firebase-functions/params');

const { parseClients, verifyClient } = require('./lib/clients');
const { createAttestationHandler } = require('./lib/handler');
const { bucketId, createLimiter, takeToken } = require('./lib/limits');
const { assess } = require('./lib/recaptcha');

initializeApp();

const clientsSecret = defineSecret('RIGEL_ATTEST_CLIENTS');
const appId = (process.env.RIGEL_FIREBASE_APP_ID || '').trim();

const DEFAULT_LIMITS = {
  client: { perHour: 12, burst: 4 },
  ip: { perHour: 60, burst: 12 },
};

function positiveInteger(name, fallback) {
  const value = Number.parseInt(process.env[name] || '', 10);
  return Number.isFinite(value) && value > 0 ? value : fallback;
}

const limits = {
  client: {
    perHour: positiveInteger('RIGEL_ATTEST_CLIENT_PER_HOUR', DEFAULT_LIMITS.client.perHour),
    burst: positiveInteger('RIGEL_ATTEST_CLIENT_BURST', DEFAULT_LIMITS.client.burst),
  },
  ip: {
    perHour: positiveInteger('RIGEL_ATTEST_IP_PER_HOUR', DEFAULT_LIMITS.ip.perHour),
    burst: positiveInteger('RIGEL_ATTEST_IP_BURST', DEFAULT_LIMITS.ip.burst),
  },
};

let cachedRaw = null;
let cachedClients = null;

/** The secret is read per request; parsing it once per value keeps the check cheap. */
function configuredClients(raw) {
  if (raw === cachedRaw) return cachedClients;
  cachedClients = parseClients(raw);
  cachedRaw = raw;
  return cachedClients;
}

exports.appCheck = onRequest(
  {
    secrets: [clientsSecret],
    invoker: 'public',
    maxInstances: 20,
    timeoutSeconds: 15,
  },
  async (request, response) => {
    let clients;
    try {
      clients = configuredClients(clientsSecret.value());
    } catch (error) {
      // The configuration, not the caller, is wrong. Say so in the log, never in the body.
      console.error(`app-check: ${error.message}`);
      response.status(503).json({ error: 'attestation is not configured' });
      return;
    }

    const handler = createAttestationHandler({
      configured: appId.length > 0,
      verifyClient: (header) => verifyClient(header, clients),
      consume: createLimiter({ db: getFirestore(), client: limits.client, ip: limits.ip }).consume,
      mint: async () => {
        const minted = await getAppCheck().createToken(appId);
        const ttlMillis = Number.isFinite(minted.ttlMillis) ? minted.ttlMillis : 60 * 60 * 1000;
        return {
          token: minted.token,
          expiresAt: Math.floor(Date.now() / 1000) + Math.floor(ttlMillis / 1000),
        };
      },
      log: (message) => console.log(message),
    });

    await handler(request, response);
  },
);

// ---------------------------------------------------------------- contact form

// Public by design: the same key is in the page's script tag.
const RECAPTCHA_SITE_KEY = process.env.RECAPTCHA_SITE_KEY || '6Lc7CAwuAAAAABFNlJiGKGyMDPaKOoAeTpwaVKkr';
const CONTACT_PER_HOUR = 5;
const CONTACT_BURST = 2;
const MAX_MESSAGE = 2000;
const MAX_EMAIL = 200;

/**
 * One message per visitor, gated by a reCAPTCHA Enterprise token.
 *
 * The limits are deliberately tight: the form exists so someone can report a wrong number or ask
 * for a skill, not so a script can fill a collection. Google is asked first, because a token that
 * was never assessed proves nothing.
 */
async function consumeContact(ip) {
  const db = getFirestore();
  const ref = db.collection('rigelContactLimits').doc(bucketId('contact-ip', ip));
  return db.runTransaction(async (transaction) => {
    const snapshot = await transaction.get(ref);
    const now = Date.now();
    const state = snapshot.exists
      ? { tokens: snapshot.data().tokens, updatedAt: snapshot.data().updated_at_ms }
      : { tokens: CONTACT_BURST, updatedAt: now };
    const step = takeToken(state, now, CONTACT_PER_HOUR, CONTACT_BURST);
    transaction.set(ref, { tokens: step.tokens, updated_at_ms: step.updatedAt }, { merge: true });
    return step;
  });
}

/**
 * The runtime service account's own access token. reCAPTCHA Enterprise accepts it instead of an
 * API key, which keeps a rotatable credential out of the deployment entirely.
 */
async function serviceAccountToken() {
  try {
    const credential = getApp().options.credential;
    if (credential && typeof credential.getAccessToken === 'function') {
      const token = await credential.getAccessToken();
      return (token && token.access_token) || '';
    }
  } catch (error) {
    console.error(`contact: no service-account token: ${error.message}`);
  }
  return '';
}

exports.contact = onRequest(
  { invoker: 'public', maxInstances: 10, timeoutSeconds: 20 },
  async (request, response) => {
    if (request.method !== 'POST') {
      response.set('Allow', 'POST').status(405).json({ ok: false, error: 'method_not_allowed' });
      return;
    }

    const body = request.body && typeof request.body === 'object' ? request.body : {};
    const message = typeof body.message === 'string' ? body.message.trim() : '';
    const email = typeof body.email === 'string' ? body.email.trim() : '';
    const token = typeof body.token === 'string' ? body.token : '';

    if (message.length < 4 || message.length > MAX_MESSAGE) {
      response.status(400).json({ ok: false, error: 'message_length' });
      return;
    }
    if (email.length > MAX_EMAIL || (email.length > 0 && !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email))) {
      response.status(400).json({ ok: false, error: 'email_invalid' });
      return;
    }

    const ip = request.ip || request.headers['x-forwarded-for'] || 'unknown';
    let budget;
    try {
      budget = await consumeContact(String(ip).split(',')[0].trim());
    } catch (error) {
      console.error(`contact: limit check failed: ${error.message}`);
      response.status(503).json({ ok: false, error: 'temporarily_unavailable' });
      return;
    }
    if (!budget.allowed) {
      response.set('Retry-After', String(budget.retryAfter)).status(429).json({
        ok: false,
        error: 'rate_limited',
        retryAfter: budget.retryAfter,
      });
      return;
    }

    const apiKey = (process.env.RECAPTCHA_API_KEY || '').trim();
    const verdict = await assess({
      projectId: process.env.GCLOUD_PROJECT,
      apiKey,
      accessToken: apiKey ? '' : await serviceAccountToken(),
      siteKey: RECAPTCHA_SITE_KEY,
      token,
      expectedAction: 'contact',
    });

    if (!verdict.ok) {
      // 503/502 mean "we are misconfigured or Google is down" — the browser may retry those.
      // 403/400 are about the token, and the visitor gets told to try again.
      console.error(`contact: ${verdict.reason}`);
      if (verdict.status >= 500) {
        response.status(verdict.status).json({ ok: false, error: 'verification_unavailable' });
      } else {
        response.status(verdict.status).json({ ok: false, error: 'verification_failed' });
      }
      return;
    }

    try {
      await getFirestore().collection('messages').add({
        message,
        email,
        score: verdict.score === undefined ? null : verdict.score,
        created_at: FieldValue.serverTimestamp(),
      });
    } catch (error) {
      console.error(`contact: write failed: ${error.message}`);
      response.status(503).json({ ok: false, error: 'temporarily_unavailable' });
      return;
    }

    response.status(200).json({ ok: true });
  },
);

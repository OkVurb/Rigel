'use strict';

const { clientIp } = require('./handler');

/**
 * The one sponsored slot Rigel shows. The desktop app and the website both ask this broker for
 * `rigel-sponsor`; neither holds the Gravity publisher key or talks to Gravity directly.
 */
const PLACEMENT = Object.freeze({ placement: 'below_response', placement_id: 'rigel-sponsor' });

const SITE_ORIGINS = new Set([
  'https://askrigel.com',
  'https://www.askrigel.com',
  'https://rigel-25128.web.app',
  'https://rigel-25128.firebaseapp.com',
]);
const DEV_ORIGINS = new Set([
  'http://localhost:5000',
  'http://localhost:5173',
  'http://localhost:4173',
  'http://127.0.0.1:5000',
  'http://127.0.0.1:5173',
  'http://127.0.0.1:4173',
]);

const MAX_BODY_CHARS = 24_000;
const MAX_MESSAGES = 4;
const MAX_MESSAGE_CHARS = 2_000;
const MAX_ID = 128;
const MAX_URL = 2_048;

const DEVICE_FIELDS = [
  'ua',
  'os',
  'locale',
  'language',
  'timezone',
  'browser',
  'device_type',
  'screen_width',
  'screen_height',
  'viewport_width',
  'viewport_height',
  'country',
];

function header(request, name) {
  if (typeof request.get === 'function') {
    const value = request.get(name);
    if (value !== undefined) return value;
  }
  const raw = request.headers ? request.headers[name] : undefined;
  return Array.isArray(raw) ? raw[0] : raw;
}

function originAllowed(origin) {
  // Rust clients and server-side previews send no Origin; they are not browsers evading CORS.
  return !origin || SITE_ORIGINS.has(origin) || DEV_ORIGINS.has(origin);
}

function cors(origin) {
  const headers = {
    'cache-control': 'no-store, max-age=0',
    'content-type': 'application/json; charset=utf-8',
    'x-content-type-options': 'nosniff',
    vary: 'Origin',
    'access-control-allow-headers': 'content-type',
    'access-control-allow-methods': 'POST, OPTIONS',
    'access-control-max-age': '600',
  };
  if (origin) headers['access-control-allow-origin'] = origin;
  return headers;
}

function respond(response, status, body, headers) {
  response.status(status).set(headers).json(body);
}

function text(value, max) {
  return typeof value === 'string' ? value.trim().slice(0, max) : '';
}

function id(value) {
  const cleaned = text(value, MAX_ID);
  return /^[A-Za-z0-9:_-]+$/.test(cleaned) ? cleaned : '';
}

function https(value) {
  if (typeof value !== 'string' || value.length > MAX_URL) return '';
  try {
    const url = new URL(value);
    return url.protocol === 'https:' ? value.trim() : '';
  } catch {
    return '';
  }
}

function messages(input) {
  if (!Array.isArray(input)) return [];
  return input
    .slice(-MAX_MESSAGES)
    .map((message) => {
      if (!message || typeof message !== 'object') return null;
      const content = text(message.content, MAX_MESSAGE_CHARS);
      if (!content) return null;
      return {
        role: message.role === 'assistant' ? 'assistant' : 'user',
        content,
      };
    })
    .filter(Boolean);
}

/**
 * The website sends the SDK's `gravity_context`; the desktop sends the same fields flat. Both
 * become one context object, and the server supplies the placement and the end-user IP.
 */
function context(body) {
  const source =
    body && typeof body.gravity_context === 'object' && body.gravity_context
      ? body.gravity_context
      : body || {};
  const sessionId = id(source.sessionId || source.session_id || source.session);
  const rawUser = source.user && typeof source.user === 'object' ? source.user : {};
  const userId = id(rawUser.id || rawUser.userId || source.userId || source.user_id);
  if (!sessionId || !userId) return null;

  const user = { id: userId };
  if (rawUser.tags && typeof rawUser.tags === 'object' && !Array.isArray(rawUser.tags)) {
    const tags = {};
    for (const [key, value] of Object.entries(rawUser.tags).slice(0, 8)) {
      if (typeof value === 'string' && key.length <= 64 && value.length <= 64) {
        tags[key] = value;
      }
    }
    if (Object.keys(tags).length > 0) user.tags = tags;
  }

  const rawDevice = source.device && typeof source.device === 'object' ? source.device : {};
  const device = {};
  for (const field of DEVICE_FIELDS) {
    const value = rawDevice[field];
    if (typeof value === 'string') {
      const cleaned = text(value, 512);
      if (cleaned) device[field] = cleaned;
    } else if (typeof value === 'number' && Number.isFinite(value)) {
      device[field] = value;
    }
  }

  const result = { sessionId, user, device };
  const rawConsent = source.consent && typeof source.consent === 'object' ? source.consent : null;
  if (rawConsent) {
    const consent = {};
    if (typeof rawConsent.gdprApplies === 'boolean') consent.gdprApplies = rawConsent.gdprApplies;
    if (typeof rawConsent.tcfString === 'string' && rawConsent.tcfString.length <= 8_192) {
      consent.tcfString = rawConsent.tcfString;
    }
    if (typeof rawConsent.usPrivacy === 'string' && rawConsent.usPrivacy.length <= 64) {
      consent.usPrivacy = rawConsent.usPrivacy;
    }
    if (Object.keys(consent).length > 0) result.consent = consent;
  }
  return result;
}

/** Drop fields the card cannot use, and never forward an insecure tracking URL. */
function publicAd(ad) {
  if (!ad || typeof ad !== 'object') return null;
  const clickUrl = https(ad.clickUrl);
  if (!clickUrl) return null;
  const result = {
    adText: text(ad.adText, 600),
    title: text(ad.title, 200),
    brandName: text(ad.brandName, 120),
    cta: text(ad.cta, 80) || 'Learn more',
    url: https(ad.url),
    clickUrl,
    impUrl: https(ad.impUrl),
    placement: text(ad.placement, 64),
    placement_id: text(ad.placement_id, 64),
  };
  if (!result.adText && !result.title) return null;
  return result;
}

/**
 * The endpoint both Rigel surfaces use. `getAds` is injected so the tests can exercise an empty
 * fill, a missing configuration and a bad ad without a publisher key or a network call.
 */
function createGravityHandler({ getAds, test, log = () => {} }) {
  return async function gravityAd(request, response) {
    const origin = header(request, 'origin');

    if (!originAllowed(origin)) {
      // No Access-Control-Allow-Origin here: the browser must not be able to read this answer.
      respond(response, 403, { error: 'origin_not_allowed' }, cors(undefined));
      return;
    }

    const headers = cors(origin);

    if (request.method === 'OPTIONS') {
      response.status(204).set(headers).end();
      return;
    }
    if (request.method !== 'POST') {
      respond(response, 405, { error: 'method_not_allowed' }, { ...headers, allow: 'POST, OPTIONS' });
      return;
    }

    let body = request.body;
    if (typeof body === 'string') {
      try {
        body = JSON.parse(body);
      } catch {
        respond(response, 400, { error: 'invalid_json' }, headers);
        return;
      }
    }
    if (!body || typeof body !== 'object' || Array.isArray(body)) {
      respond(response, 400, { error: 'invalid_body' }, headers);
      return;
    }
    if (JSON.stringify(body).length > MAX_BODY_CHARS) {
      respond(response, 413, { error: 'body_too_large' }, headers);
      return;
    }

    const conversation = messages(body.messages);
    const gravityContext = context(body);
    if (conversation.length === 0 || !gravityContext) {
      respond(response, 400, { error: 'invalid_context' }, headers);
      return;
    }

    const ip = clientIp(request);
    const incoming = {
      body: { gravity_context: gravityContext },
      headers: {
        ...(request.headers || {}),
        ...(ip ? { 'x-forwarded-for': ip } : {}),
      },
      ...(ip ? { ip } : {}),
    };

    let result;
    try {
      result = await getAds(incoming, conversation, [PLACEMENT]);
    } catch (error) {
      log(`gravity-ad: the SDK threw (${error && error.code ? error.code : 'error'})`);
      respond(response, 503, { error: 'ads_unavailable', test: Boolean(test) }, headers);
      return;
    }

    if (!result || result.status === 0) {
      // A missing secret or a blocked network. The caller sees a generic unavailable, not why.
      log(`gravity-ad: no answer (${result && result.status ? result.status : 'no status'})`);
      respond(response, 503, { error: 'ads_unavailable', test: Boolean(test) }, headers);
      return;
    }

    if (result.status >= 400) {
      log(`gravity-ad: Gravity answered ${result.status}`);
      respond(response, 200, { ads: [], test: Boolean(test) }, headers);
      return;
    }

    const ads = (Array.isArray(result.ads) ? result.ads : []).map(publicAd).filter(Boolean).slice(0, 1);
    respond(response, 200, { ads, test: Boolean(test) }, headers);
  };
}

module.exports = {
  DEV_ORIGINS,
  PLACEMENT,
  SITE_ORIGINS,
  createGravityHandler,
  publicAd,
};

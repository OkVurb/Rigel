import { bearer, safeEqual } from '../security.js';

const ROLES = new Set(['rigel', 'muse']);
const KINDS = new Set(['event', 'request', 'response', 'status']);
const MAX_PAYLOAD_BYTES = 256 * 1024;
const MAX_WAIT_SECONDS = 25;
const POLL_INTERVAL_MS = 250;

function other(role) {
  return role === 'rigel' ? 'muse' : 'rigel';
}

function integer(value, fallback, minimum, maximum) {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(maximum, Math.max(minimum, parsed));
}

function validIdempotencyKey(value) {
  return typeof value === 'string' && /^[A-Za-z0-9._:-]{1,128}$/.test(value);
}

function validKind(value) {
  return typeof value === 'string' && KINDS.has(value);
}

function canonicalJson(value) {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
  if (value && typeof value === 'object') {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`)
      .join(',')}}`;
  }
  return JSON.stringify(value);
}

function validPayload(value) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return false;
  return Buffer.byteLength(JSON.stringify(value), 'utf8') <= MAX_PAYLOAD_BYTES;
}

function sleep(milliseconds, signal) {
  return new Promise((resolve) => {
    const timer = setTimeout(done, milliseconds);
    function done() {
      signal?.removeEventListener('abort', done);
      clearTimeout(timer);
      resolve();
    }
    signal?.addEventListener('abort', done, { once: true });
  });
}

function authenticateRelay(config, request) {
  const token = bearer(request.headers.authorization);
  if (!token) return null;
  for (const role of ROLES) {
    const expected = config.relayTokens?.[role];
    if (expected && safeEqual(token, expected)) return role;
  }
  return null;
}

function publicMessage(row) {
  return {
    seq: Number(row.seq),
    id: row.id,
    sender: row.sender,
    recipient: row.recipient,
    kind: row.kind,
    payload: row.payload,
    created_at: row.created_at,
    expires_at: row.expires_at,
    acknowledged_at: row.acknowledged_at,
  };
}

async function inbox(app, role, after, limit) {
  const { rows } = await app.db.query(
    `SELECT seq, id, sender, recipient, kind, payload, created_at, expires_at, acknowledged_at
       FROM relay_messages
      WHERE recipient = $1
        AND seq > $2
        AND acknowledged_at IS NULL
        AND expires_at > now()
      ORDER BY seq ASC
      LIMIT $3`,
    [role, after, limit],
  );
  return rows.map(publicMessage);
}

export async function registerRelayRoutes(app) {
  async function relayAuth(request, reply) {
    const role = authenticateRelay(app.config, request);
    if (!role) return reply.code(401).send({ error: 'unauthorized' });
    request.relayRole = role;
    return undefined;
  }

  app.get('/v1/relay/health', { preHandler: relayAuth }, async (request) => ({
    ok: true,
    role: request.relayRole,
    peer: other(request.relayRole),
  }));

  app.post(
    '/v1/relay/messages',
    { preHandler: relayAuth, config: { rateLimit: { max: 120, timeWindow: '1 minute' } } },
    async (request, reply) => {
      const body = request.body ?? {};
      if (!validKind(body.kind)) {
        return reply.code(400).send({ error: 'invalid_kind', allowed: [...KINDS] });
      }
      if (!validIdempotencyKey(body.idempotency_key)) {
        return reply.code(400).send({ error: 'invalid_idempotency_key' });
      }
      if (!validPayload(body.payload)) {
        return reply.code(413).send({ error: 'invalid_payload', max_bytes: MAX_PAYLOAD_BYTES });
      }

      const sender = request.relayRole;
      const recipient = other(sender);
      const { rows } = await app.db.query(
        `INSERT INTO relay_messages (sender, recipient, kind, payload, idempotency_key)
         VALUES ($1, $2, $3, $4::jsonb, $5)
         ON CONFLICT (sender, idempotency_key) DO UPDATE
           SET idempotency_key = relay_messages.idempotency_key
         RETURNING seq, id, sender, recipient, kind, payload, created_at, expires_at,
                   acknowledged_at`,
        [sender, recipient, body.kind, JSON.stringify(body.payload), body.idempotency_key],
      );
      if (rows[0].kind !== body.kind || canonicalJson(rows[0].payload) !== canonicalJson(body.payload)) {
        return reply.code(409).send({ error: 'idempotency_conflict' });
      }
      return reply.code(202).send({ message: publicMessage(rows[0]) });
    },
  );

  app.get('/v1/relay/messages', { preHandler: relayAuth }, async (request) => {
    const after = integer(request.query?.after, 0, 0, Number.MAX_SAFE_INTEGER);
    const limit = integer(request.query?.limit, 50, 1, 200);
    const wait = integer(request.query?.wait, 0, 0, MAX_WAIT_SECONDS);
    const deadline = Date.now() + wait * 1000;
    let messages = await inbox(app, request.relayRole, after, limit);
    while (messages.length === 0 && Date.now() < deadline && !request.raw.aborted) {
      await sleep(Math.min(POLL_INTERVAL_MS, deadline - Date.now()), request.raw.signal);
      messages = await inbox(app, request.relayRole, after, limit);
    }
    return {
      role: request.relayRole,
      messages,
      next_after: messages.at(-1)?.seq ?? after,
    };
  });

  app.post('/v1/relay/messages/:id/ack', { preHandler: relayAuth }, async (request, reply) => {
    const id = String(request.params?.id ?? '');
    if (!/^[0-9a-f-]{36}$/i.test(id)) return reply.code(404).send({ error: 'not_found' });
    const { rows } = await app.db.query(
      `UPDATE relay_messages
          SET acknowledged_at = COALESCE(acknowledged_at, now())
        WHERE id = $1 AND recipient = $2 AND expires_at > now()
      RETURNING seq, id, sender, recipient, kind, payload, created_at, expires_at, acknowledged_at`,
      [id, request.relayRole],
    );
    if (!rows[0]) return reply.code(404).send({ error: 'not_found' });
    return { message: publicMessage(rows[0]) };
  });
}

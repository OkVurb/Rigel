import assert from 'node:assert/strict';
import test from 'node:test';
import Fastify from 'fastify';
import { registerRelayRoutes } from '../src/routes/relay.js';

const tokens = {
  rigel: 'rigel-token-that-is-long-enough-for-production-1234567890',
  muse: 'muse-token-that-is-long-enough-for-production-12345678901',
};

function auth(role) {
  return { authorization: `Bearer ${tokens[role]}` };
}

async function makeApp() {
  const messages = [];
  let nextSeq = 1;
  const db = {
    async query(sql, values) {
      if (sql.includes('INSERT INTO relay_messages')) {
        const [sender, recipient, kind, payloadText, key] = values;
        let row = messages.find((message) => message.sender === sender && message.key === key);
        if (!row) {
          row = {
            seq: nextSeq++,
            id: `00000000-0000-4000-8000-${String(nextSeq).padStart(12, '0')}`,
            sender,
            recipient,
            kind,
            payload: JSON.parse(payloadText),
            key,
            created_at: new Date().toISOString(),
            expires_at: new Date(Date.now() + 60_000).toISOString(),
            acknowledged_at: null,
          };
          messages.push(row);
        }
        return { rows: [row] };
      }
      if (sql.includes('FROM relay_messages')) {
        const [recipient, after, limit] = values;
        return {
          rows: messages
            .filter((message) => message.recipient === recipient && message.seq > after && !message.acknowledged_at)
            .slice(0, limit),
        };
      }
      if (sql.includes('UPDATE relay_messages')) {
        const [id, recipient] = values;
        const row = messages.find((message) => message.id === id && message.recipient === recipient);
        if (!row) return { rows: [] };
        row.acknowledged_at ||= new Date().toISOString();
        return { rows: [row] };
      }
      throw new Error(`unexpected query: ${sql}`);
    },
  };
  const app = Fastify({ logger: false });
  app.decorate('config', { relayTokens: tokens });
  app.decorate('db', db);
  await registerRelayRoutes(app);
  await app.ready();
  return { app, messages };
}

async function send(app, role, body) {
  return app.inject({
    method: 'POST',
    url: '/v1/relay/messages',
    headers: { ...auth(role), 'content-type': 'application/json' },
    payload: body,
  });
}

test('relay requires one of the two machine credentials', async () => {
  const { app } = await makeApp();
  try {
    const missing = await app.inject({ method: 'GET', url: '/v1/relay/health' });
    const invalid = await app.inject({
      method: 'GET',
      url: '/v1/relay/health',
      headers: { authorization: 'Bearer wrong' },
    });
    assert.equal(missing.statusCode, 401);
    assert.equal(invalid.statusCode, 401);
  } finally {
    await app.close();
  }
});

test('Rigel can send only to Muse and inboxes are role-isolated', async () => {
  const { app } = await makeApp();
  try {
    const created = await send(app, 'rigel', {
      kind: 'event',
      idempotency_key: 'swarm-1',
      payload: { type: 'swarm.ready', job_id: 'swarm-1' },
    });
    assert.equal(created.statusCode, 202);
    assert.equal(created.json().message.sender, 'rigel');
    assert.equal(created.json().message.recipient, 'muse');

    const rigelInbox = await app.inject({ method: 'GET', url: '/v1/relay/messages', headers: auth('rigel') });
    const museInbox = await app.inject({ method: 'GET', url: '/v1/relay/messages', headers: auth('muse') });
    assert.deepEqual(rigelInbox.json().messages, []);
    assert.equal(museInbox.json().messages.length, 1);
    assert.equal(museInbox.json().messages[0].payload.job_id, 'swarm-1');
  } finally {
    await app.close();
  }
});

test('repeating an idempotency key is safe but changing its message conflicts', async () => {
  const { app, messages } = await makeApp();
  const body = { kind: 'status', idempotency_key: 'status-1', payload: { online: true } };
  try {
    assert.equal((await send(app, 'muse', body)).statusCode, 202);
    assert.equal((await send(app, 'muse', body)).statusCode, 202);
    assert.equal(messages.length, 1);
    const conflict = await send(app, 'muse', { ...body, payload: { online: false } });
    assert.equal(conflict.statusCode, 409);
    assert.deepEqual(conflict.json(), { error: 'idempotency_conflict' });
  } finally {
    await app.close();
  }
});

test('only the recipient can acknowledge a message', async () => {
  const { app } = await makeApp();
  try {
    const sent = await send(app, 'rigel', {
      kind: 'request',
      idempotency_key: 'request-1',
      payload: { action: 'sync' },
    });
    const id = sent.json().message.id;
    const senderAck = await app.inject({ method: 'POST', url: `/v1/relay/messages/${id}/ack`, headers: auth('rigel') });
    const recipientAck = await app.inject({ method: 'POST', url: `/v1/relay/messages/${id}/ack`, headers: auth('muse') });
    assert.equal(senderAck.statusCode, 404);
    assert.equal(recipientAck.statusCode, 200);

    const inbox = await app.inject({ method: 'GET', url: '/v1/relay/messages', headers: auth('muse') });
    assert.deepEqual(inbox.json().messages, []);
  } finally {
    await app.close();
  }
});

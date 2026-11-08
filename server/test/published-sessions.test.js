import assert from 'node:assert/strict';
import test from 'node:test';
import Fastify from 'fastify';
import { registerAuthRoutes } from '../src/routes/auth.js';

const ownerId = '00000000-0000-4000-8000-000000000001';
const otherId = '00000000-0000-4000-8000-000000000002';

async function makeApp({ authenticated = true } = {}) {
  const calls = [];
  const db = {
    async query(sql, values) {
      calls.push({ sql, values });
      if (sql.includes('FROM published_sessions') && sql.includes('session_id = $2')) {
        return { rows: values[0] === ownerId && values[1] === 'owned' ? [{ id: 'owned' }] : [] };
      }
      if (sql.includes('FROM published_sessions')) {
        return {
          rows: values[0] === ownerId ? [{ id: 'owned', title: 'A published session' }] : [],
        };
      }
      if (sql.includes('FROM published_turns')) {
        return { rows: [{ id: 'turn-1', prompt: 'hello', reply: 'world' }] };
      }
      return { rows: [] };
    },
  };
  const app = Fastify({ logger: false });
  app.decorate('config', {});
  app.decorate('db', db);
  app.decorate('mailer', {});
  app.decorate('authenticate', async (request, reply) => {
    if (!authenticated) return reply.code(401).send({ error: 'unauthorized' });
    request.user = { id: ownerId };
  });
  await registerAuthRoutes(app);
  await app.ready();
  return { app, calls };
}

test('published sessions are authenticated and scoped to the current user', async () => {
  const { app, calls } = await makeApp();
  try {
    const response = await app.inject({ method: 'GET', url: '/v1/sessions?limit=12' });
    assert.equal(response.statusCode, 200);
    assert.deepEqual(response.json().sessions, [{ id: 'owned', title: 'A published session' }]);
    assert.deepEqual(calls[0].values, [ownerId, 12]);
  } finally {
    await app.close();
  }
});

test('session detail does not reveal another user’s session', async () => {
  const { app, calls } = await makeApp();
  try {
    const response = await app.inject({ method: 'GET', url: '/v1/sessions/not-owned' });
    assert.equal(response.statusCode, 404);
    assert.deepEqual(response.json(), { error: 'not_found' });
    assert.deepEqual(calls[0].values, [ownerId, 'not-owned']);
  } finally {
    await app.close();
  }
});

test('turns verify session ownership before returning scoped turn data', async () => {
  const { app, calls } = await makeApp();
  try {
    const response = await app.inject({ method: 'GET', url: '/v1/sessions/owned/turns?limit=2' });
    assert.equal(response.statusCode, 200);
    assert.deepEqual(response.json().turns, [{ id: 'turn-1', prompt: 'hello', reply: 'world' }]);
    assert.deepEqual(calls.map((call) => call.values), [
      [ownerId, 'owned'],
      [ownerId, 'owned', 2],
    ]);
  } finally {
    await app.close();
  }
});

test('unauthenticated clients cannot read published sessions', async () => {
  const { app, calls } = await makeApp({ authenticated: false });
  try {
    const response = await app.inject({ method: 'GET', url: '/v1/sessions' });
    assert.equal(response.statusCode, 401);
    assert.deepEqual(calls, []);
  } finally {
    await app.close();
  }
});

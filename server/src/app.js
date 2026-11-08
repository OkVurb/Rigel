import Fastify from 'fastify';
import cors from '@fastify/cors';
import rateLimit from '@fastify/rate-limit';
import { publicConfig } from './config.js';
import { bearer, verifyAccessToken } from './security.js';
import { registerAuthRoutes } from './routes/auth.js';
import { registerRelayRoutes } from './routes/relay.js';

export async function buildApp({ config, pool, mailer, logger = true, fetchImpl = fetch }) {
  const app = Fastify({
    logger,
    bodyLimit: 6 * 1024 * 1024,
    trustProxy: true,
    disableRequestLogging: config.production,
  });

  const allowedOrigins = new Set(config.allowedOrigins);
  await app.register(cors, {
    origin(origin, callback) {
      if (!origin || allowedOrigins.has(origin)) return callback(null, true);
      return callback(null, false);
    },
    credentials: true,
    methods: ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS'],
    allowedHeaders: ['content-type', 'authorization', 'x-rigel-native', 'x-rigel-client'],
    maxAge: 86_400,
  });
  await app.register(rateLimit, {
    max: 240,
    timeWindow: '1 minute',
    allowList: [],
  });

  app.decorate('config', config);
  app.decorate('db', pool);
  app.decorate('mailer', mailer);
  app.decorate('fetchImpl', fetchImpl);
  app.decorate('startedAt', Date.now());

  app.decorate('authenticate', async function authenticate(request, reply) {
    const token = bearer(request.headers.authorization);
    if (!token) return reply.code(401).send({ error: 'unauthorized' });
    let payload;
    try {
      payload = await verifyAccessToken(config.jwtSecret, token);
    } catch {
      return reply.code(401).send({ error: 'invalid_token' });
    }
    const { rows } = await app.db.query(
      `SELECT id, email, email_verified, reset_required, role, daily_llm_micros,
              created_at, last_login_at
         FROM users WHERE id = $1`,
      [payload.sub],
    );
    if (!rows[0]) return reply.code(401).send({ error: 'invalid_token' });
    if (!payload.sid) return reply.code(401).send({ error: 'invalid_token' });
    request.user = rows[0];
    request.sessionId = payload.sid;
    return undefined;
  });

  app.addHook('onSend', async (request, reply, payload) => {
    reply.header('x-content-type-options', 'nosniff');
    reply.header('referrer-policy', 'no-referrer');
    reply.header('x-frame-options', 'DENY');
    if (request.url.startsWith('/v1/')) reply.header('cache-control', 'no-store');
    return payload;
  });

  app.setErrorHandler((error, request, reply) => {
    const status = Number(error.statusCode) || 500;
    if (status >= 500) request.log.error({ err: error }, 'request failed');
    else request.log.info({ err: error.message, url: request.url }, 'request rejected');
    if (reply.sent) return undefined;
    return reply.code(status).send({
      error: status >= 500 ? 'internal_error' : (error.code || 'bad_request'),
      ...(status >= 500 || config.production ? {} : { message: error.message }),
    });
  });

  app.get('/health', async () => {
    await app.db.query('SELECT 1');
    return {
      ok: true,
      service: 'rigel-api',
      version: 1,
      uptime_seconds: Math.floor((Date.now() - app.startedAt) / 1000),
    };
  });

  app.get('/v1/meta', async () => publicConfig(config));

  await registerAuthRoutes(app);
  if (config.relayTokens?.rigel && config.relayTokens?.muse) {
    await registerRelayRoutes(app);
  }

  return app;
}

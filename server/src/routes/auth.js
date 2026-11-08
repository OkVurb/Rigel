import { randomUUID } from 'node:crypto';
import {
  bearer,
  clientIp,
  hashPassword,
  normalizeEmail,
  randomToken,
  sha256,
  signAccessToken,
  validEmail,
  validPassword,
  verifyPassword,
} from '../security.js';

const REFRESH_COOKIE = 'rigel_refresh';
const VERIFY_HOURS = 24;
const RESET_MINUTES = 60;

function parseCookies(header) {
  const result = {};
  for (const part of String(header ?? '').split(';')) {
    const index = part.indexOf('=');
    if (index < 1) continue;
    result[part.slice(0, index).trim()] = decodeURIComponent(part.slice(index + 1).trim());
  }
  return result;
}

function wantsNative(request) {
  return request.headers['x-rigel-native'] === '1';
}

function setRefreshCookie(reply, config, token) {
  const maxAge = config.refreshDays * 86_400;
  const secure = config.production ? '; Secure' : '';
  reply.header(
    'set-cookie',
    `${REFRESH_COOKIE}=${encodeURIComponent(token)}; HttpOnly; Path=/v1/auth; SameSite=Lax; Max-Age=${maxAge}${secure}`,
  );
}

function clearRefreshCookie(reply, config) {
  const secure = config.production ? '; Secure' : '';
  reply.header(
    'set-cookie',
    `${REFRESH_COOKIE}=; HttpOnly; Path=/v1/auth; SameSite=Lax; Max-Age=0${secure}`,
  );
}

function publicUser(user) {
  return {
    id: user.id,
    email: user.email,
    email_verified: user.email_verified,
    role: user.role,
    reset_required: user.reset_required,
    daily_llm_micros: Number(user.daily_llm_micros ?? 0),
    created_at: user.created_at,
  };
}

async function createSession(app, request, user) {
  const { config } = app;
  const id = randomUUID();
  const refresh = randomToken(48);
  const expiresAt = new Date(Date.now() + config.refreshDays * 86_400_000);
  await app.db.query(
    `INSERT INTO auth_sessions (id, user_id, refresh_hash, user_agent, ip_hash, expires_at)
     VALUES ($1, $2, $3, $4, $5, $6)`,
    [
      id,
      user.id,
      sha256(refresh),
      String(request.headers['user-agent'] ?? '').slice(0, 300),
      sha256(`${config.jwtSecret}:${clientIp(request)}`),
      expiresAt,
    ],
  );
  const access = await signAccessToken({
    secret: config.jwtSecret,
    userId: user.id,
    sessionId: id,
    email: user.email,
    minutes: config.accessMinutes,
  });
  return {
    accessToken: access,
    refreshToken: refresh,
    expiresIn: config.accessMinutes * 60,
  };
}

function sessionResponse(user, session, native) {
  return {
    access_token: session.accessToken,
    token_type: 'Bearer',
    expires_in: session.expiresIn,
    user: publicUser(user),
    ...(native ? { refresh_token: session.refreshToken } : {}),
  };
}

async function createToken(app, userId, kind, lifetimeMs) {
  const raw = randomToken(32);
  await app.db.query(
    `INSERT INTO auth_tokens (id, user_id, kind, token_hash, expires_at)
     VALUES ($1, $2, $3, $4, $5)`,
    [randomUUID(), userId, kind, sha256(raw), new Date(Date.now() + lifetimeMs)],
  );
  return raw;
}

async function audit(app, userId, action, detail = {}) {
  await app.db
    .query('INSERT INTO audit_log (id, user_id, action, detail) VALUES ($1, $2, $3, $4::jsonb)', [
      randomUUID(),
      userId,
      action,
      JSON.stringify(detail),
    ])
    .catch(() => undefined);
}

export async function registerAuthRoutes(app) {
  const { config } = app;

  app.post(
    '/v1/auth/signup',
    { config: { rateLimit: { max: 5, timeWindow: '1 minute' } } },
    async (request, reply) => {
      const email = normalizeEmail(request.body?.email);
      const password = request.body?.password;
      if (!validEmail(email)) return reply.code(400).send({ error: 'invalid_email' });
      if (!validPassword(password)) {
        return reply.code(400).send({ error: 'weak_password', message: 'Use at least 10 characters.' });
      }
      const passwordHash = await hashPassword(password);
      let user;
      try {
        const { rows } = await app.db.query(
          `INSERT INTO users (email, password_hash, daily_llm_micros)
           VALUES ($1, $2, $3)
           RETURNING id, email, email_verified, reset_required, role, daily_llm_micros, created_at`,
          [email, passwordHash, config.llmDefaultDailyMicros],
        );
        user = rows[0];
      } catch (error) {
        if (error.code === '23505') return reply.code(409).send({ error: 'email_taken' });
        throw error;
      }

      const verifyToken = await createToken(app, user.id, 'verify', VERIFY_HOURS * 3_600_000);
      let verificationSent = true;
      try {
        await app.mailer.verification(email, verifyToken);
      } catch (error) {
        verificationSent = false;
        request.log.error({ err: error }, 'verification mail failed');
      }
      const session = await createSession(app, request, user);
      const native = wantsNative(request);
      if (!native) setRefreshCookie(reply, config, session.refreshToken);
      await audit(app, user.id, 'signup', { verificationSent });
      return reply.code(201).send({
        ...sessionResponse(user, session, native),
        verification_sent: verificationSent,
      });
    },
  );

  app.post(
    '/v1/auth/login',
    { config: { rateLimit: { max: 10, timeWindow: '1 minute' } } },
    async (request, reply) => {
      const email = normalizeEmail(request.body?.email);
      const password = String(request.body?.password ?? '');
      const { rows } = await app.db.query(
        `SELECT id, email, password_hash, email_verified, reset_required, role,
                daily_llm_micros, created_at
           FROM users WHERE lower(email) = lower($1)`,
        [email],
      );
      const user = rows[0];
      if (!user) {
        await verifyPassword(password, 'scrypt$32768$8$1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA');
        return reply.code(401).send({ error: 'invalid_credentials' });
      }
      if (!user.password_hash || !(await verifyPassword(password, user.password_hash))) {
        return reply.code(401).send({ error: 'invalid_credentials' });
      }
      if (user.reset_required) {
        const token = await createToken(app, user.id, 'reset', RESET_MINUTES * 60_000);
        await app.mailer.reset(user.email, token).catch((error) => {
          request.log.error({ err: error }, 'reset mail failed');
        });
        return reply.code(403).send({ error: 'password_reset_required' });
      }
      await app.db.query('UPDATE users SET last_login_at = now(), updated_at = now() WHERE id = $1', [
        user.id,
      ]);
      const session = await createSession(app, request, user);
      const native = wantsNative(request);
      if (!native) setRefreshCookie(reply, config, session.refreshToken);
      await audit(app, user.id, 'login', { native });
      return sessionResponse(user, session, native);
    },
  );

  app.post(
    '/v1/auth/refresh',
    { config: { rateLimit: { max: 30, timeWindow: '1 minute' } } },
    async (request, reply) => {
      const cookies = parseCookies(request.headers.cookie);
      const supplied = String(request.body?.refresh_token ?? cookies[REFRESH_COOKIE] ?? '');
      if (!supplied) return reply.code(401).send({ error: 'invalid_refresh_token' });
      const { rows } = await app.db.query(
        `SELECT s.id AS session_id, s.user_id, s.expires_at, s.revoked_at,
                u.id, u.email, u.email_verified, u.reset_required, u.role,
                u.daily_llm_micros, u.created_at
           FROM auth_sessions s
           JOIN users u ON u.id = s.user_id
          WHERE s.refresh_hash = $1`,
        [sha256(supplied)],
      );
      const row = rows[0];
      if (!row || row.revoked_at || new Date(row.expires_at).getTime() <= Date.now()) {
        clearRefreshCookie(reply, config);
        return reply.code(401).send({ error: 'invalid_refresh_token' });
      }
      const refresh = randomToken(48);
      await app.db.query(
        `UPDATE auth_sessions
            SET refresh_hash = $2, last_seen_at = now(),
                expires_at = now() + ($3 || ' days')::interval
          WHERE id = $1`,
        [row.session_id, sha256(refresh), String(config.refreshDays)],
      );
      const user = {
        id: row.user_id,
        email: row.email,
        email_verified: row.email_verified,
        reset_required: row.reset_required,
        role: row.role,
        daily_llm_micros: row.daily_llm_micros,
        created_at: row.created_at,
      };
      const access = await signAccessToken({
        secret: config.jwtSecret,
        userId: user.id,
        sessionId: row.session_id,
        email: user.email,
        minutes: config.accessMinutes,
      });
      const native = wantsNative(request) || Boolean(request.body?.refresh_token);
      if (!native) setRefreshCookie(reply, config, refresh);
      return {
        access_token: access,
        token_type: 'Bearer',
        expires_in: config.accessMinutes * 60,
        user: publicUser(user),
        ...(native ? { refresh_token: refresh } : {}),
      };
    },
  );

  app.post('/v1/auth/logout', async (request, reply) => {
    const cookies = parseCookies(request.headers.cookie);
    const supplied = String(request.body?.refresh_token ?? cookies[REFRESH_COOKIE] ?? '');
    if (supplied) {
      await app.db.query(
        'UPDATE auth_sessions SET revoked_at = now() WHERE refresh_hash = $1 AND revoked_at IS NULL',
        [sha256(supplied)],
      );
    }
    clearRefreshCookie(reply, config);
    return reply.code(204).send();
  });

  app.post('/v1/auth/logout-all', { preHandler: app.authenticate }, async (request, reply) => {
    await app.db.query(
      'UPDATE auth_sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL',
      [request.user.id],
    );
    clearRefreshCookie(reply, config);
    return reply.code(204).send();
  });

  app.get('/v1/auth/me', { preHandler: app.authenticate }, async (request) => ({
    user: publicUser(request.user),
  }));

  app.get('/v1/sessions', { preHandler: app.authenticate }, async (request) => {
    const limit = Math.min(100, Math.max(1, Number.parseInt(request.query?.limit, 10) || 50));
    const { rows } = await app.db.query(
      `SELECT session_id AS id, title, project, status, created_at_ms, ended_at_ms,
              spend_micros, budget_cents, turn_count, agent_count, published_at
         FROM published_sessions
        WHERE user_id = $1
        ORDER BY created_at_ms DESC NULLS LAST, published_at DESC
        LIMIT $2`,
      [request.user.id, limit],
    );
    return { sessions: rows };
  });

  app.get('/v1/sessions/:id', { preHandler: app.authenticate }, async (request, reply) => {
    const { rows } = await app.db.query(
      `SELECT session_id AS id, title, project, status, created_at_ms, ended_at_ms,
              spend_micros, budget_cents, turn_count, agent_count, published_at
         FROM published_sessions
        WHERE user_id = $1 AND session_id = $2`,
      [request.user.id, request.params.id],
    );
    if (!rows[0]) return reply.code(404).send({ error: 'not_found' });
    return { session: rows[0] };
  });

  app.get('/v1/sessions/:id/turns', { preHandler: app.authenticate }, async (request, reply) => {
    const session = await app.db.query(
      'SELECT 1 FROM published_sessions WHERE user_id = $1 AND session_id = $2',
      [request.user.id, request.params.id],
    );
    if (!session.rows[0]) return reply.code(404).send({ error: 'not_found' });

    const limit = Math.min(300, Math.max(1, Number.parseInt(request.query?.limit, 10) || 100));
    const { rows } = await app.db.query(
      `SELECT turn_id AS id, order_index AS "order", seq, prompt, reply, status, stop_reason,
              started_at, completed_at, harness, role, model, cost_micros, input_tokens,
              output_tokens, subagent, truncated, published_at
         FROM published_turns
        WHERE user_id = $1 AND session_id = $2
        ORDER BY order_index ASC, seq ASC NULLS LAST
        LIMIT $3`,
      [request.user.id, request.params.id, limit],
    );
    return { turns: rows };
  });

  app.post(
    '/v1/auth/verify/request',
    { preHandler: app.authenticate, config: { rateLimit: { max: 3, timeWindow: '15 minutes' } } },
    async (request, reply) => {
      if (request.user.email_verified) return { ok: true, already_verified: true };
      const token = await createToken(app, request.user.id, 'verify', VERIFY_HOURS * 3_600_000);
      try {
        await app.mailer.verification(request.user.email, token);
      } catch (error) {
        request.log.error({ err: error }, 'verification mail failed');
        return reply.code(503).send({ error: 'mail_unavailable' });
      }
      return { ok: true, sent: true };
    },
  );

  app.post('/v1/auth/verify/confirm', async (request, reply) => {
    const token = String(request.body?.token ?? '');
    if (!token) return reply.code(400).send({ error: 'token_required' });
    const client = await app.db.connect();
    try {
      await client.query('BEGIN');
      const { rows } = await client.query(
        `SELECT id, user_id FROM auth_tokens
          WHERE token_hash = $1 AND kind = 'verify' AND used_at IS NULL AND expires_at > now()
          FOR UPDATE`,
        [sha256(token)],
      );
      const row = rows[0];
      if (!row) {
        await client.query('ROLLBACK');
        return reply.code(400).send({ error: 'invalid_or_expired_token' });
      }
      await client.query('UPDATE auth_tokens SET used_at = now() WHERE id = $1', [row.id]);
      await client.query(
        'UPDATE users SET email_verified = TRUE, updated_at = now() WHERE id = $1',
        [row.user_id],
      );
      await client.query('COMMIT');
      await audit(app, row.user_id, 'email_verified');
      return { ok: true, email_verified: true };
    } catch (error) {
      await client.query('ROLLBACK');
      throw error;
    } finally {
      client.release();
    }
  });

  app.post(
    '/v1/auth/reset/request',
    { config: { rateLimit: { max: 5, timeWindow: '15 minutes' } } },
    async (request) => {
      const email = normalizeEmail(request.body?.email);
      if (!validEmail(email)) return { ok: true };
      const { rows } = await app.db.query('SELECT id, email FROM users WHERE lower(email) = lower($1)', [
        email,
      ]);
      if (rows[0]) {
        const token = await createToken(app, rows[0].id, 'reset', RESET_MINUTES * 60_000);
        await app.mailer.reset(rows[0].email, token).catch((error) => {
          request.log.error({ err: error }, 'reset mail failed');
        });
        await audit(app, rows[0].id, 'password_reset_requested');
      }
      return { ok: true };
    },
  );

  app.post(
    '/v1/auth/reset/confirm',
    { config: { rateLimit: { max: 10, timeWindow: '15 minutes' } } },
    async (request, reply) => {
      const token = String(request.body?.token ?? '');
      const password = request.body?.password;
      if (!token) return reply.code(400).send({ error: 'token_required' });
      if (!validPassword(password)) {
        return reply.code(400).send({ error: 'weak_password', message: 'Use at least 10 characters.' });
      }
      const passwordHash = await hashPassword(password);
      const client = await app.db.connect();
      try {
        await client.query('BEGIN');
        const { rows } = await client.query(
          `SELECT id, user_id FROM auth_tokens
            WHERE token_hash = $1 AND kind = 'reset' AND used_at IS NULL AND expires_at > now()
            FOR UPDATE`,
          [sha256(token)],
        );
        const row = rows[0];
        if (!row) {
          await client.query('ROLLBACK');
          return reply.code(400).send({ error: 'invalid_or_expired_token' });
        }
        await client.query('UPDATE auth_tokens SET used_at = now() WHERE id = $1', [row.id]);
        await client.query(
          `UPDATE users
              SET password_hash = $2, reset_required = FALSE, email_verified = TRUE, updated_at = now()
            WHERE id = $1`,
          [row.user_id, passwordHash],
        );
        await client.query(
          'UPDATE auth_sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL',
          [row.user_id],
        );
        await client.query('COMMIT');
        const { rows: users } = await app.db.query('SELECT email FROM users WHERE id = $1', [
          row.user_id,
        ]);
        if (users[0]) {
          await app.mailer.passwordChanged(users[0].email).catch((error) => {
            request.log.error({ err: error }, 'password changed mail failed');
          });
        }
        await audit(app, row.user_id, 'password_reset_completed');
        clearRefreshCookie(reply, config);
        return { ok: true };
      } catch (error) {
        await client.query('ROLLBACK');
        throw error;
      } finally {
        client.release();
      }
    },
  );

  app.post('/v1/auth/password', { preHandler: app.authenticate }, async (request, reply) => {
    const current = String(request.body?.current_password ?? '');
    const next = request.body?.new_password;
    if (!validPassword(next)) {
      return reply.code(400).send({ error: 'weak_password', message: 'Use at least 10 characters.' });
    }
    const { rows } = await app.db.query('SELECT password_hash FROM users WHERE id = $1', [
      request.user.id,
    ]);
    if (!rows[0]?.password_hash || !(await verifyPassword(current, rows[0].password_hash))) {
      return reply.code(403).send({ error: 'invalid_current_password' });
    }
    const hash = await hashPassword(next);
    await app.db.query(
      'UPDATE users SET password_hash = $2, reset_required = FALSE, updated_at = now() WHERE id = $1',
      [request.user.id, hash],
    );
    await app.db.query(
      'UPDATE auth_sessions SET revoked_at = now() WHERE user_id = $1 AND id <> $2 AND revoked_at IS NULL',
      [request.user.id, request.sessionId],
    );
    await app.mailer.passwordChanged(request.user.email).catch((error) => {
      request.log.error({ err: error }, 'password changed mail failed');
    });
    await audit(app, request.user.id, 'password_changed');
    return { ok: true };
  });

  app.delete('/v1/auth/account', { preHandler: app.authenticate }, async (request, reply) => {
    if (request.body?.confirm !== 'DELETE') {
      return reply.code(400).send({ error: 'confirmation_required' });
    }
    await app.db.query('DELETE FROM users WHERE id = $1', [request.user.id]);
    clearRefreshCookie(reply, config);
    return reply.code(204).send();
  });

  app.post('/v1/devices', { preHandler: app.authenticate }, async (request, reply) => {
    const name = String(request.body?.name ?? '').trim().slice(0, 120);
    if (name.length < 2) return reply.code(400).send({ error: 'name_required' });
    const token = randomToken(32);
    const { rows } = await app.db.query(
      `INSERT INTO devices (id, user_id, name, token_hash)
       VALUES ($1, $2, $3, $4)
       RETURNING id, name, created_at, last_seen_at`,
      [randomUUID(), request.user.id, name, sha256(token)],
    );
    await audit(app, request.user.id, 'device_created', { deviceId: rows[0].id });
    return reply.code(201).send({ device: rows[0], token });
  });

  app.get('/v1/devices', { preHandler: app.authenticate }, async (request) => {
    const { rows } = await app.db.query(
      `SELECT id, name, created_at, last_seen_at, revoked_at
         FROM devices WHERE user_id = $1 ORDER BY created_at DESC`,
      [request.user.id],
    );
    return { devices: rows };
  });

  app.delete('/v1/devices/:id', { preHandler: app.authenticate }, async (request, reply) => {
    const { rowCount } = await app.db.query(
      'UPDATE devices SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL',
      [request.params.id, request.user.id],
    );
    if (!rowCount) return reply.code(404).send({ error: 'not_found' });
    return reply.code(204).send();
  });

  app.get('/v1/auth/sessions', { preHandler: app.authenticate }, async (request) => {
    const { rows } = await app.db.query(
      `SELECT id, user_agent, created_at, last_seen_at, expires_at, revoked_at,
              (id = $2) AS current
         FROM auth_sessions WHERE user_id = $1 ORDER BY last_seen_at DESC LIMIT 50`,
      [request.user.id, request.sessionId],
    );
    return { sessions: rows };
  });

  app.delete('/v1/auth/sessions/:id', { preHandler: app.authenticate }, async (request, reply) => {
    const { rowCount } = await app.db.query(
      'UPDATE auth_sessions SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL',
      [request.params.id, request.user.id],
    );
    if (!rowCount) return reply.code(404).send({ error: 'not_found' });
    return reply.code(204).send();
  });
}

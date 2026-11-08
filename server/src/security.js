import {
  createHash,
  randomBytes,
  scrypt as scryptCallback,
  timingSafeEqual,
} from 'node:crypto';
import { promisify } from 'node:util';
import path from 'node:path';
import { SignJWT, jwtVerify } from 'jose';

const scrypt = promisify(scryptCallback);
const SCRYPT = { N: 32768, r: 8, p: 1, maxmem: 96 * 1024 * 1024 };

export function normalizeEmail(value) {
  return String(value ?? '').trim().toLowerCase();
}

export function validEmail(value) {
  return /^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(value) && value.length <= 320;
}

export function validPassword(value) {
  return typeof value === 'string' && value.length >= 10 && value.length <= 256;
}

export async function hashPassword(password) {
  const salt = randomBytes(16);
  const key = await scrypt(password, salt, 64, SCRYPT);
  return `scrypt$${SCRYPT.N}$${SCRYPT.r}$${SCRYPT.p}$${salt.toString('base64url')}$${key.toString('base64url')}`;
}

export async function verifyPassword(password, encoded) {
  if (typeof encoded !== 'string') return false;
  const parts = encoded.split('$');
  if (parts.length !== 6 || parts[0] !== 'scrypt') return false;
  const [, n, r, p, saltText, keyText] = parts;
  const salt = Buffer.from(saltText, 'base64url');
  const expected = Buffer.from(keyText, 'base64url');
  if (salt.length < 8 || expected.length < 16) return false;
  try {
    const actual = await scrypt(password, salt, expected.length, {
      N: Number(n),
      r: Number(r),
      p: Number(p),
      maxmem: SCRYPT.maxmem,
    });
    return actual.length === expected.length && timingSafeEqual(actual, expected);
  } catch {
    return false;
  }
}

export function randomToken(bytes = 32) {
  return randomBytes(bytes).toString('base64url');
}

export function sha256(value) {
  return createHash('sha256').update(value, 'utf8').digest('hex');
}

export function safeEqual(left, right) {
  const a = Buffer.from(String(left ?? ''));
  const b = Buffer.from(String(right ?? ''));
  return a.length === b.length && timingSafeEqual(a, b);
}

function secretKey(secret) {
  return new TextEncoder().encode(secret);
}

export async function signAccessToken({ secret, userId, sessionId, email, minutes = 15 }) {
  return new SignJWT({ sid: sessionId, email })
    .setProtectedHeader({ alg: 'HS256', typ: 'JWT' })
    .setSubject(userId)
    .setIssuer('rigel-cloud')
    .setAudience('rigel-api')
    .setIssuedAt()
    .setExpirationTime(`${minutes}m`)
    .sign(secretKey(secret));
}

export async function verifyAccessToken(secret, token) {
  const { payload } = await jwtVerify(token, secretKey(secret), {
    issuer: 'rigel-cloud',
    audience: 'rigel-api',
  });
  return payload;
}

export function bearer(header) {
  const match = /^Bearer\s+([^\s]+)$/i.exec(String(header ?? '').trim());
  return match ? match[1] : null;
}

/** A user-owned relative path, normalized under root. Absolute paths and traversal are refused. */
export function safeJoin(root, relative) {
  const cleaned = String(relative ?? '').replace(/\\/g, '/').trim();
  if (!cleaned || cleaned.startsWith('/') || cleaned.includes('\0')) {
    throw new Error('invalid path');
  }
  const parts = cleaned.split('/');
  if (parts.some((part) => part === '..' || part === '.' || part === '')) {
    throw new Error('invalid path');
  }
  const target = path.resolve(root, ...parts);
  const base = path.resolve(root);
  if (target !== base && !target.startsWith(base + path.sep)) throw new Error('invalid path');
  return target;
}

export function usageCost({ input, output, inputMicros, outputMicros }) {
  const inTokens = Math.max(0, Number(input) || 0);
  const outTokens = Math.max(0, Number(output) || 0);
  return Math.round((inTokens * inputMicros + outTokens * outputMicros) / 1_000_000);
}

export function clientIp(request) {
  const forwarded = request.headers?.['x-forwarded-for'];
  const rows = String(Array.isArray(forwarded) ? forwarded.join(',') : forwarded ?? '').split(',');
  for (let index = rows.length - 1; index >= 0; index -= 1) {
    const row = rows[index].trim();
    if (/^[0-9a-fA-F:.]+$/.test(row)) return row;
  }
  return String(request.ip || request.socket?.remoteAddress || 'unknown').replace(/^::ffff:/, '');
}

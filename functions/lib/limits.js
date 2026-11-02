'use strict';

const crypto = require('node:crypto');

const HOUR_MS = 60 * 60 * 1000;
const RETENTION_MS = 7 * 24 * HOUR_MS;
const MAX_RETRY_SECONDS = 24 * 60 * 60;

/**
 * One token-bucket step. A bucket refills at `perHour` and never holds more than `burst`.
 * The state is plain numbers so it can live in one Firestore document; the caller decides
 * whether to persist the answer.
 */
function takeToken(bucket, nowMs, perHour, burst) {
  const capacity = Math.max(1, Math.floor(burst));
  const rate = Math.max(Number.EPSILON, Number(perHour));
  const stored = bucket && Number.isFinite(bucket.tokens) ? Math.max(0, bucket.tokens) : capacity;
  const updatedAt = bucket && Number.isFinite(bucket.updatedAt) ? bucket.updatedAt : nowMs;
  const elapsed = Math.max(0, nowMs - updatedAt);
  const available = Math.min(capacity, stored + (elapsed * rate) / HOUR_MS);

  if (available < 1) {
    const waitMs = ((1 - available) * HOUR_MS) / rate;
    return {
      allowed: false,
      retryAfter: Math.min(MAX_RETRY_SECONDS, Math.max(1, Math.ceil(waitMs / 1000))),
      tokens: available,
      updatedAt: nowMs,
    };
  }
  return {
    allowed: true,
    retryAfter: 0,
    tokens: available - 1,
    updatedAt: nowMs,
  };
}

/** A stable, non-secret document id. Neither the credential nor the raw IP is stored. */
function bucketId(kind, subject) {
  return crypto.createHash('sha256').update(`${kind}:${subject}`, 'utf8').digest('hex');
}

function stateOf(snapshot, nowMs, burst) {
  if (!snapshot || snapshot.exists !== true) {
    return { tokens: burst, updatedAt: nowMs };
  }
  const data = snapshot.data() || {};
  return { tokens: data.tokens, updatedAt: data.updated_at_ms };
}

/**
 * Two buckets per authenticated request: one for the installation (a credential may be copied,
 * but it cannot mint tokens without limit) and one for the source address (one host cannot
 * drain a credential's budget for everyone else). Both are read and written in one Firestore
 * transaction, so a denial consumes nothing.
 */
function createLimiter({ db, collection = 'rigelAppCheckLimits', client, ip }) {
  async function consume({ client: label, ip: address, now: nowMs }) {
    const refs = [db.collection(collection).doc(bucketId('client', label))];
    if (address) refs.push(db.collection(collection).doc(bucketId('ip', address)));

    return db.runTransaction(async (transaction) => {
      const snapshots = await Promise.all(refs.map((ref) => transaction.get(ref)));
      let answered = null;

      const decisions = [
        takeToken(stateOf(snapshots[0], nowMs, client.burst), nowMs, client.perHour, client.burst),
      ];
      if (refs.length > 1) {
        decisions.push(takeToken(stateOf(snapshots[1], nowMs, ip.burst), nowMs, ip.perHour, ip.burst));
      }

      const denied = decisions.filter((decision) => !decision.allowed);
      if (denied.length > 0) {
        answered = {
          allowed: false,
          retryAfter: Math.max(...denied.map((decision) => decision.retryAfter)),
        };
        return answered;
      }

      const expiresAt = new Date(nowMs + RETENTION_MS);
      refs.forEach((ref, index) => {
        transaction.set(ref, {
          tokens: decisions[index].tokens,
          updated_at_ms: decisions[index].updatedAt,
          expires_at: expiresAt,
        });
      });
      answered = { allowed: true, retryAfter: 0 };
      return answered;
    });
  }

  return { consume };
}

module.exports = { bucketId, createLimiter, takeToken, HOUR_MS, RETENTION_MS };

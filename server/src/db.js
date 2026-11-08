import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import pg from 'pg';

const { Pool } = pg;

export function createPool(connectionString, options = {}) {
  return new Pool({
    connectionString,
    max: Number(options.max ?? 10),
    idleTimeoutMillis: 30_000,
    connectionTimeoutMillis: 10_000,
    ssl: options.ssl ? { rejectUnauthorized: true } : undefined,
  });
}

const MIGRATIONS = [
  [1, '001_init'],
  [2, '002_relay'],
];

export async function migrate(pool) {
  const client = await pool.connect();
  try {
    await client.query('BEGIN');
    for (const [version, name] of MIGRATIONS) {
      const url = new URL(`../sql/${name}.sql`, import.meta.url);
      const sql = await readFile(fileURLToPath(url), 'utf8');
      await client.query(sql);
      await client.query(
        `INSERT INTO schema_migrations (version, name) VALUES ($1, $2)
         ON CONFLICT (version) DO NOTHING`,
        [version, name],
      );
    }
    await client.query('COMMIT');
  } catch (error) {
    await client.query('ROLLBACK');
    throw error;
  } finally {
    client.release();
  }
}

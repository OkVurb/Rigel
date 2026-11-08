import { mkdir } from 'node:fs/promises';
import { loadConfig } from './config.js';
import { createPool, migrate } from './db.js';
import { buildApp } from './app.js';
import { createMailer } from './mailer.js';

const config = loadConfig();
if (!config.databaseUrl) throw new Error('DATABASE_URL is required');

await mkdir(config.dataDir, { recursive: true });
const pool = createPool(config.databaseUrl, { ssl: process.env.DATABASE_SSL === 'true' });
await migrate(pool);

const mailer = createMailer({ config });
const app = await buildApp({ config, pool, mailer });

async function shutdown(signal) {
  app.log.info({ signal }, 'shutting down');
  await app.close().catch(() => undefined);
  await pool.end().catch(() => undefined);
  process.exit(0);
}

process.on('SIGINT', () => void shutdown('SIGINT'));
process.on('SIGTERM', () => void shutdown('SIGTERM'));

await app.listen({ port: config.port, host: config.host });
app.log.info({ port: config.port, host: config.host, production: config.production }, 'rigel api ready');

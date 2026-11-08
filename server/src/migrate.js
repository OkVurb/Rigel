import { loadConfig } from './config.js';
import { createPool, migrate } from './db.js';

const config = loadConfig();
if (!config.databaseUrl) throw new Error('DATABASE_URL is required');
const pool = createPool(config.databaseUrl, { ssl: process.env.DATABASE_SSL === 'true' });
try {
  await migrate(pool);
  console.log('database migrated');
} finally {
  await pool.end();
}

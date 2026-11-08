const REQUIRED_IN_PRODUCTION = ['DATABASE_URL', 'JWT_SECRET', 'APP_URL', 'API_URL'];

function integer(value, fallback) {
  const parsed = Number.parseInt(value ?? '', 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallback;
}

function list(value, fallback = '') {
  return String(value ?? fallback)
    .split(',')
    .map((row) => row.trim())
    .filter(Boolean);
}

export function loadConfig(env = process.env) {
  const production = env.NODE_ENV === 'production';
  const config = {
    production,
    port: integer(env.PORT, 8080),
    host: env.HOST || '0.0.0.0',
    databaseUrl: env.DATABASE_URL || '',
    jwtSecret: env.JWT_SECRET || 'dev-only-change-me-0123456789abcdef',
    accessMinutes: integer(env.ACCESS_TOKEN_MINUTES, 15),
    refreshDays: integer(env.REFRESH_TOKEN_DAYS, 30),
    appUrl: (env.APP_URL || 'http://localhost:5173').replace(/\/+$/, ''),
    apiUrl: (env.API_URL || 'http://localhost:8080').replace(/\/+$/, ''),
    allowedOrigins: list(
      env.ALLOWED_ORIGINS,
      'https://askrigel.com,https://www.askrigel.com,http://localhost:5173,http://localhost:4173',
    ),
    resendApiKey: env.RESEND_API_KEY || '',
    mailFrom: env.MAIL_FROM || 'Rigel <hello@askrigel.com>',
    contactTo: env.CONTACT_TO || 'hello@askrigel.com',
    dataDir: env.DATA_DIR || '/data',
    llmBaseUrl: (env.LLM_BASE_URL || '').replace(/\/+$/, ''),
    llmApiKey: env.LLM_API_KEY || '',
    llmProvider: env.LLM_PROVIDER || 'openrouter',
    llmModels: list(env.LLM_MODELS, ''),
    llmInputMicrosPerMillion: integer(env.LLM_INPUT_MICROS_PER_MILLION, 500000),
    llmOutputMicrosPerMillion: integer(env.LLM_OUTPUT_MICROS_PER_MILLION, 1500000),
    llmDefaultDailyMicros: integer(env.LLM_DAILY_MICROS, 2000000),
    llmMaxOutputTokens: integer(env.LLM_MAX_OUTPUT_TOKENS, 8192),
    gravityApiKey: env.GRAVITY_API_KEY || '',
    gravityProduction: env.GRAVITY_PRODUCTION === 'true',
    turnstileSecret: env.TURNSTILE_SECRET || '',
    enrollToken: env.ENROLL_TOKEN || '',
    requireDevice: env.REQUIRE_DEVICE === 'true',
    relayTokens: {
      rigel: env.RIGEL_RELAY_TOKEN || '',
      muse: env.MUSE_RELAY_TOKEN || '',
    },
  };

  if (production) {
    const missing = REQUIRED_IN_PRODUCTION.filter((key) => !env[key]);
    if (missing.length) throw new Error(`missing required environment: ${missing.join(', ')}`);
    if (config.jwtSecret.length < 32) throw new Error('JWT_SECRET must be at least 32 characters');
    if (!config.databaseUrl.startsWith('postgres')) throw new Error('DATABASE_URL must be a postgres URL');
    const relayConfigured = Object.values(config.relayTokens).filter(Boolean);
    if (relayConfigured.length !== 0 && relayConfigured.length !== 2) {
      throw new Error('RIGEL_RELAY_TOKEN and MUSE_RELAY_TOKEN must be configured together');
    }
    if (relayConfigured.some((token) => token.length < 43)) {
      throw new Error('relay tokens must contain at least 256 bits of random data');
    }
    if (relayConfigured.length === 2 && config.relayTokens.rigel === config.relayTokens.muse) {
      throw new Error('Rigel and Muse relay tokens must be different');
    }
  }
  return config;
}

export function publicConfig(config) {
  return {
    name: 'Rigel Cloud',
    api_version: 1,
    app_url: config.appUrl,
    llm: {
      enabled: Boolean(config.llmBaseUrl && config.llmApiKey),
      provider: config.llmProvider,
      models: config.llmModels,
    },
    auth: { email_verification: true, password_reset: true },
  };
}

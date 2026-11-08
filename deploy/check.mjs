#!/usr/bin/env node
// Validate the deployment contract without touching the server.
import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(here, '..')

function arg(name, fallback) {
  const index = process.argv.indexOf(name)
  if (index === -1) return fallback
  const value = process.argv[index + 1]
  if (!value || value.startsWith('--')) throw new Error(`${name} requires a value`)
  return value
}

const envPath = path.resolve(arg('--env', path.join(here, '.env.production')))
const sitePath = path.resolve(arg('--site', path.join(root, '.deploy')))
const composePath = path.resolve(arg('--compose', path.join(here, 'docker-compose.yml')))
const caddyPath = path.resolve(arg('--caddy', path.join(here, 'Caddyfile')))

const passed = []
const notes = []
const failed = []

function must(condition, message) {
  if (!condition) throw new Error(message)
}

function check(name, fn) {
  try {
    fn()
    passed.push(name)
  } catch (error) {
    failed.push(`${name}: ${error.message}`)
  }
}

function read(file) {
  return readFileSync(file, 'utf8').replace(/^\uFEFF/, '')
}

function parseEnv(file) {
  const env = {}
  for (const raw of read(file).split(/\r?\n/)) {
    const line = raw.trim()
    if (!line || line.startsWith('#')) continue
    const match = /^(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)=(.*)$/.exec(line)
    if (!match) continue
    let value = match[2].trim()
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1)
    }
    env[match[1]] = value
  }
  return env
}

function envMap(service) {
  const raw = service?.environment ?? {}
  if (Array.isArray(raw)) {
    return Object.fromEntries(
      raw.map((entry) => {
        const index = entry.indexOf('=')
        return index === -1 ? [entry, ''] : [entry.slice(0, index), entry.slice(index + 1)]
      }),
    )
  }
  return raw
}

function run(command, args, options = {}) {
  return execFileSync(command, args, {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    ...options,
  })
}

const env = parseEnv(envPath)

check('env file exists and parses', () => {
  must(existsSync(envPath), `missing ${envPath}`)
  must(Object.keys(env).length > 10, 'env file has too few values')
})

check('required production values are real', () => {
  const required = [
    'ACME_EMAIL',
    'POSTGRES_USER',
    'POSTGRES_PASSWORD',
    'POSTGRES_DB',
    'POSTGRES_CMS_DB',
    'JWT_SECRET',
    'APP_URL',
    'API_URL',
    'ALLOWED_ORIGINS',
    'PAYLOAD_SECRET',
    'RIGEL_RELAY_TOKEN',
    'MUSE_RELAY_TOKEN',
    'PAYLOAD_PUBLIC_SERVER_URL',
    'PAYLOAD_ADMIN_EMAIL',
    'PAYLOAD_ADMIN_PASSWORD',
    'RIGEL_FIREBASE_PROJECT_ID',
    'RIGEL_FIREBASE_API_KEY',
    'RIGEL_FIREBASE_AUTH_DOMAIN',
  ]
  for (const key of required) must(env[key], `missing ${key}`)
  must(env.POSTGRES_USER === 'rigel', 'POSTGRES_USER must be rigel for postgres-init.sql')
  must(env.POSTGRES_DB === 'rigel', 'POSTGRES_DB must be rigel')
  must(env.POSTGRES_CMS_DB === 'rigel_cms', 'POSTGRES_CMS_DB must be rigel_cms')
  must(env.APP_URL === 'https://askrigel.com', 'APP_URL must be https://askrigel.com')
  must(env.API_URL === 'https://api.askrigel.com', 'API_URL must be https://api.askrigel.com')
  must(
    env.PAYLOAD_PUBLIC_SERVER_URL === 'https://admin.askrigel.com',
    'PAYLOAD_PUBLIC_SERVER_URL must be https://admin.askrigel.com',
  )
  must(env.ACME_EMAIL.includes('@'), 'ACME_EMAIL must be an email address')
  must(env.JWT_SECRET.length >= 32, 'JWT_SECRET is shorter than 32 characters')
  must(env.PAYLOAD_SECRET.length >= 32, 'PAYLOAD_SECRET is shorter than 32 characters')
  must(env.RIGEL_RELAY_TOKEN.length >= 43, 'RIGEL_RELAY_TOKEN is shorter than 256 bits')
  must(env.MUSE_RELAY_TOKEN.length >= 43, 'MUSE_RELAY_TOKEN is shorter than 256 bits')
  must(env.RIGEL_RELAY_TOKEN !== env.MUSE_RELAY_TOKEN, 'relay tokens must be different')
  must(env.POSTGRES_PASSWORD.length >= 24, 'POSTGRES_PASSWORD is too short')
  must(env.PAYLOAD_ADMIN_PASSWORD.length >= 16, 'PAYLOAD_ADMIN_PASSWORD is too short')
  for (const origin of ['https://askrigel.com', 'https://www.askrigel.com', 'https://admin.askrigel.com']) {
    must(env.ALLOWED_ORIGINS.split(',').includes(origin), `ALLOWED_ORIGINS is missing ${origin}`)
  }
  for (const key of [
    'POSTGRES_PASSWORD',
    'JWT_SECRET',
    'PAYLOAD_SECRET',
    'PAYLOAD_ADMIN_PASSWORD',
    'RIGEL_RELAY_TOKEN',
    'MUSE_RELAY_TOKEN',
  ]) {
    must(!/CHANGE_ME|replace-with|example/i.test(env[key]), `${key} still contains a placeholder`)
  }
})

check('every server config env key is supplied', () => {
  const keys = new Set(
    [...read(path.join(root, 'server/src/config.js')).matchAll(/env\.([A-Z0-9_]+)/g)].map(
      (match) => match[1],
    ),
  )
  keys.add('DATABASE_SSL')
  const composeProvided = new Set(['NODE_ENV', 'PORT', 'HOST', 'DATABASE_URL'])
  for (const key of keys) {
    if (composeProvided.has(key)) continue
    must(Object.hasOwn(env, key), `server reads ${key}, but it is missing from the env file`)
  }
})

check('every CMS env key is supplied', () => {
  const keys = new Set()
  for (const file of ['cms/src/payload.config.ts', 'cms/src/seed.ts', 'cms/src/lib/rigel.ts']) {
    for (const match of read(path.join(root, file)).matchAll(/process\.env\.([A-Z0-9_]+)/g)) {
      keys.add(match[1])
    }
  }
  const composeProvided = new Set(['DATABASE_URL', 'PAYLOAD_PUBLIC_SERVER_URL'])
  for (const key of keys) {
    if (composeProvided.has(key)) continue
    must(Object.hasOwn(env, key), `CMS reads ${key}, but it is missing from the env file`)
  }
})

let compose = null
check('docker compose config parses', () => {
  must(existsSync(composePath), `missing ${composePath}`)
  try {
    const raw = run('docker', [
      'compose',
      '-f',
      composePath,
      '--env-file',
      envPath,
      'config',
      '--format',
      'json',
    ])
    compose = JSON.parse(raw)
  } catch (error) {
    const detail = String(error.stderr || error.message || error)
    throw new Error(detail.trim().split(/\r?\n/).slice(-3).join(' '))
  }
  must(compose?.services, 'compose output has no services')
})

check('compose has the four expected services', () => {
  must(compose, 'compose config did not parse')
  for (const name of ['db', 'api', 'cms', 'caddy']) {
    must(compose.services[name], `missing service ${name}`)
  }
  must(compose.services.api.build.context.replaceAll('\\', '/').endsWith('/server'), 'api build context is not server')
  must(compose.services.cms.build.context.replaceAll('\\', '/').endsWith('/cms'), 'cms build context is not cms')
  must(!compose.services.db.ports || compose.services.db.ports.length === 0, 'db must not publish a port')
  must(!compose.services.api.ports || compose.services.api.ports.length === 0, 'api must not publish a port')
  must(!compose.services.cms.ports || compose.services.cms.ports.length === 0, 'cms must not publish a port')
})

check('compose ports and database URLs match the code', () => {
  must(compose, 'compose config did not parse')
  const api = envMap(compose.services.api)
  const cms = envMap(compose.services.cms)
  must(String(api.PORT) === '8080', 'api PORT must be 8080')
  must(String(api.DATABASE_URL).endsWith('/rigel'), 'api DATABASE_URL must point at the rigel database')
  must(String(cms.DATABASE_URL).endsWith('/rigel_cms'), 'cms DATABASE_URL must point at rigel_cms')
  must(
    (compose.services.api.expose ?? []).map(String).includes('8080'),
    'api must expose 8080',
  )
  must(
    (compose.services.cms.expose ?? []).map(String).includes('3000'),
    'cms must expose 3000',
  )
  const published = (compose.services.caddy.ports ?? []).map((port) => String(port.published))
  must(published.includes('80') && published.includes('443'), 'caddy must publish 80 and 443')
})

check('Caddyfile targets the real services and hostnames', () => {
  const text = read(caddyPath)
  for (const host of ['askrigel.com, www.askrigel.com', 'api.askrigel.com', 'admin.askrigel.com']) {
    must(text.includes(host), `missing host ${host}`)
  }
  must(/email\s+\{\$ACME_EMAIL\}/.test(text), 'Caddyfile must use {$ACME_EMAIL}')
  must(/root\s+\*\s+\/srv\/site/.test(text), 'Caddyfile root must be /srv/site')
  must(/reverse_proxy\s+api:8080/.test(text), 'API upstream must be api:8080')
  must(/reverse_proxy\s+cms:3000/.test(text), 'CMS upstream must be cms:3000')
  must(/@cms_routes\s+path\s+\/admin\s+\/admin\/\*\s+\/_next\/\*\s+\/api\/\*/.test(text), 'website must route admin, Next assets, and API requests to the CMS')
  must(/handle\s+\/api\/contact\s+\{[\s\S]*?reverse_proxy\s+https:\/\/us-central1-rigel-25128\.cloudfunctions\.net/.test(text), 'contact bridge must remain configured')
  must(/handle\s+\/api\/gravity-ad\s+\{[\s\S]*?reverse_proxy\s+https:\/\/us-central1-rigel-25128\.cloudfunctions\.net/.test(text), 'Gravity bridge must remain configured')
  must(/handle\s+\/api\/catalog-meta\s+\{[\s\S]*?reverse_proxy\s+https:\/\/rigel-25128-default-rtdb\.firebaseio\.com/.test(text), 'catalogue metadata must use the first-party route')
  must(!/reverse_proxy\s+(localhost|127\.0\.0\.1)/.test(text), 'upstreams must not be localhost')
  must(!/example\.com/.test(text), 'Caddyfile contains an example host')
})

check('static site artifact is complete and generated', () => {
  must(existsSync(path.join(sitePath, 'index.html')), 'site index.html is missing')
  must(existsSync(path.join(sitePath, 'faq.html')), 'site faq.html is missing')
  must(existsSync(path.join(sitePath, 'app/index.html')), 'site app/index.html is missing')
  must(existsSync(path.join(sitePath, 'app/config.js')), 'site app/config.js is missing')
  const index = read(path.join(sitePath, 'index.html'))
  const app = read(path.join(sitePath, 'app/index.html'))
  must(/site\.css\?v=[a-f0-9]+/.test(index), 'index.html has no hashed site.css')
  must(/app\.css\?v=[a-f0-9]+/.test(app), 'app/index.html has no hashed app.css')
  must(/app\.js\?v=[a-f0-9]+/.test(app), 'app/index.html has no hashed app.js')
  must(!/http:\/\/localhost/.test(index), 'site index.html points at localhost')
  const configText = read(path.join(sitePath, 'app/config.js'))
  const match = configText.match(/window\.RIGEL_CONFIG\s*=\s*(\{[\s\S]*\});/)
  must(match, 'app/config.js has no RIGEL_CONFIG')
  const config = JSON.parse(match[1])
  must(config.projectId, 'site Firebase projectId is empty')
  must(config.apiKey, 'site Firebase apiKey is empty')
  must(config.authDomain, 'site Firebase authDomain is empty')
  must(!/firebaseio\.com/.test(read(path.join(sitePath, 'site.js'))), 'browser site script calls Firebase Realtime Database directly')
  must(/fetch\(["']\/api\/catalog-meta["']/.test(read(path.join(sitePath, 'site.js'))), 'browser catalogue metadata must use the first-party site API')
})

check('Dockerfiles match the runtime contract', () => {
  const api = read(path.join(root, 'server/Dockerfile'))
  const cms = read(path.join(root, 'cms/Dockerfile'))
  must(/EXPOSE\s+8080/.test(api), 'server Dockerfile must EXPOSE 8080')
  must(/USER\s+node/.test(api), 'server Dockerfile must run as node')
  must(/CMD\s+\["node",\s*"src\/index\.js"\]/.test(api), 'server CMD must start src/index.js')
  must(/EXPOSE\s+3000/.test(cms), 'CMS Dockerfile must EXPOSE 3000')
  must(/npm run migrate/.test(cms), 'CMS Dockerfile must run migrations at startup')
  must(/\/app\/media/.test(cms), 'CMS Dockerfile must create the writable media directory')
})

check('shell and helper scripts parse', () => {
  for (const script of ['bootstrap.sh', 'deploy.sh', 'remote.sh']) {
    run('bash', ['-n', path.join(here, script)])
  }
  run(process.execPath, ['--check', path.join(here, 'init-env.mjs')])
})

const caddyBin = process.env.CADDY_BIN || 'caddy'
try {
  run(caddyBin, ['validate', '--config', caddyPath, '--adapter', 'caddyfile'], {
    env: { ...process.env, ACME_EMAIL: env.ACME_EMAIL },
  })
  passed.push('Caddyfile validates with caddy')
} catch (error) {
  if (error.code === 'ENOENT') {
    notes.push('Caddy binary not found; install caddy or set CADDY_BIN for Caddyfile validation')
  } else {
    failed.push(`Caddyfile validate: ${String(error.stderr || error.message).trim().split(/\r?\n/).slice(-4).join(' ')}`)
  }
}

const sshKey = process.env.VPS_SSH_KEY || path.join(os.homedir(), '.ssh', 'rigel_vps')
if (!existsSync(sshKey) || !existsSync(`${sshKey}.pub`)) {
  notes.push(`SSH key pair not found at ${sshKey}; deploy.sh will reject it`)
}
if (!env.RESEND_API_KEY) notes.push('RESEND_API_KEY is blank: signup verification and password-reset email are disabled')
if (!env.LLM_BASE_URL || !env.LLM_API_KEY) notes.push('LLM gateway credentials are blank: LLM calls stay disabled')
if (!env.GRAVITY_API_KEY) notes.push('GRAVITY_API_KEY is blank: the public ad bridge will return no fill')

for (const name of passed) console.log(`PASS ${name}`)
for (const note of notes) console.log(`NOTE ${note}`)
for (const failure of failed) console.error(`FAIL ${failure}`)
console.log(`\n${passed.length} passed, ${notes.length} notes, ${failed.length} failed`)
if (failed.length) process.exit(1)

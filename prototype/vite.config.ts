import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'
import type { Plugin } from 'vite'

/**
 * Dev-only hub proxy.
 *
 * Neither marketplace sends CORS headers, and SkillHub only serves HTML, so the
 * browser cannot read them directly. This middleware fetches server-side and
 * normalises both into JSON for the Discover view. Phase 1 moves this into the
 * Rust indexer; the shape of the responses is the contract.
 *
 * Only allow-listed hosts are reachable — it is not an open proxy.
 */
const UA = 'Mozilla/5.0 (compatible; SkillIndexPrototype/0.1)'

const decode = (s: string) =>
  s
    .replace(/<!--\s*-->/g, '')
    .replace(/<[^>]+>/g, ' ')
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#x27;|&#39;/g, "'")
    .replace(/&nbsp;/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()

const toText = (html: string) =>
  decode(
    html
      .replace(/<\/(p|div|li|h[1-6]|tr)>/gi, '\n')
      .replace(/<br\s*\/?>/gi, '\n')
      .replace(/<li[^>]*>/gi, '- ')
      .replace(/<h[1-6][^>]*>/gi, '\n## ')
      .replace(/<code[^>]*>([\s\S]*?)<\/code>/gi, '`$1`'),
  )
    .replace(/\n{3,}/g, '\n\n')
    .trim()

interface Card {
  slug: string
  name: string
  description: string
  author: string
  source: string
  installs: string
  stars: string
  shield: string
}

function parseCards(html: string): Card[] {
  const cards: Card[] = []
  const re = /<a class="skill-card[^"]*" href="\/skills\/([^"?]+)[^"]*">([\s\S]*?)<\/a>/g
  let m: RegExpExecArray | null
  while ((m = re.exec(html))) {
    const slug = m[1]
    const body = m[2]
    const pick = (re2: RegExp) => body.match(re2)?.[1]?.trim() ?? ''
    const name = pick(/class="sc-name">([^<]+)</)
    if (!name) continue
    const stat = (label: string) => {
      const re3 = new RegExp(`title="${label}"[\\s\\S]*?</svg>\\s*([^<]+)`, 'i')
      return body.match(re3)?.[1]?.trim() ?? ''
    }
    cards.push({
      slug,
      name,
      description: decode(pick(/class="skill-desc sc-desc">([\s\S]*?)<\/p>/)),
      author: decode(pick(/class="sc-author">([^<]*)</)),
      source: decode(pick(/class="source-badge">([^<]*)</)),
      installs: stat('Installs'),
      stars: stat('GitHub stars'),
      shield: pick(/class="sc-shield-num">([\d/]+)/),
    })
  }
  return cards
}

function json(res: import('node:http').ServerResponse, status: number, body: unknown) {
  res.statusCode = status
  res.setHeader('content-type', 'application/json; charset=utf-8')
  res.setHeader('cache-control', 'no-store')
  res.end(JSON.stringify(body))
}

async function get(url: string) {
  const r = await fetch(url, { headers: { 'user-agent': UA, accept: '*/*' } })
  const text = await r.text()
  return { status: r.status, text }
}

function hubProxy(): Plugin {
  return {
    name: 'hub-proxy',
    configureServer(server) {
      server.middlewares.use('/__hub', async (req, res) => {
        const url = new URL(req.url ?? '/', 'http://localhost')
        try {
          if (url.pathname === '/skillsmp') {
            const q = url.searchParams.get('q') ?? 'agent'
            const limit = url.searchParams.get('limit') ?? '30'
            const target = `https://skillsmp.com/api/v1/skills/search?q=${encodeURIComponent(q)}&limit=${limit}&sortBy=stars`
            const r = await get(target)
            return json(res, r.status, JSON.parse(r.text))
          }

          if (url.pathname === '/skillhub') {
            const page = url.searchParams.get('page') ?? '1'
            const limit = url.searchParams.get('limit') ?? '36'
            const r = await get(
              `https://skills.browseract.com/skills?page=${page}&limit=${limit}&sort=safest`,
            )
            const total = r.text.match(/Total\s+([\d,]+)\s+skills/)
            return json(res, r.status, {
              total: total ? Number(total[1].replace(/,/g, '')) : null,
              cards: parseCards(r.text),
            })
          }

          if (url.pathname === '/skillhub/skill') {
            const slug = url.searchParams.get('slug') ?? ''
            if (!/^[a-z0-9-]+$/i.test(slug)) return json(res, 400, { error: 'bad slug' })
            const r = await get(`https://skills.browseract.com/skills/${slug}`)
            if (r.status !== 200) return json(res, r.status, { error: 'not found' })
            const article = r.text.match(/<article class="sd-ext-skillmd">([\s\S]*?)<\/article>/)
            const repo = r.text.match(/https:\/\/github\.com\/([\w.-]+\/[\w.-]+)/)
            const shield = r.text.match(/Security audits:\s*(\d+\/\d+)/)
            const title = r.text.match(/<h1[^>]*>([\s\S]*?)<\/h1>/)
            return json(res, 200, {
              name: title ? decode(title[1]) : slug,
              repo: repo ? repo[1] : null,
              shield: shield ? shield[1] : null,
              body: article ? toText(article[1]) : '',
            })
          }

          return json(res, 404, { error: 'unknown hub route' })
        } catch (e) {
          return json(res, 502, { error: e instanceof Error ? e.message : 'proxy failed' })
        }
      })
    },
  }
}

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), hubProxy()],
})

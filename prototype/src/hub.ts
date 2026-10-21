import type { ImportedSkill, RepoMeta } from "./github";
import { fetchRepoIndex, loadSkill, scanSkill, gradeFor, parseFrontmatter, toDocBlocks } from "./github";

/**
 * Two marketplaces the browser cannot call directly (no CORS, and one only
 * serves HTML). Both go through the dev-only `/__hub` proxy in vite.config.ts.
 *
 *   SkillsMP   JSON API, githubUrl points at the skill directory
 *   SkillHub   HTML, but the detail page renders the SKILL.md body
 */

export type HubSource = "github" | "skillsmp" | "skillhub";

export interface HubEntry {
  key: string;
  source: Exclude<HubSource, "github">;
  name: string;
  author: string;
  description: string;
  url: string;
  stars?: number;
  installs?: string;
  shield?: string;
  githubUrl?: string;
  slug?: string;
  repo?: string | null;
}

export interface HubLoaded {
  skill: ImportedSkill;
  repo: RepoMeta;
}

async function proxy<T>(path: string): Promise<T> {
  const res = await fetch(`/__hub${path}`);
  if (!res.ok) {
    const body = await res.json().catch(() => ({}) as { error?: string });
    throw new Error(
      res.status === 404
        ? "Marketplace proxy is only available on the dev server."
        : (body.error ?? `Proxy responded ${res.status}`),
    );
  }
  return (await res.json()) as T;
}

interface MpSkill {
  id: string;
  name: string;
  author: string;
  description: string;
  githubUrl?: string;
  skillUrl: string;
  stars?: number;
}

export async function searchSkillsMP(query: string): Promise<HubEntry[]> {
  const data = await proxy<{ data?: { skills?: MpSkill[] } }>(
    `/skillsmp?q=${encodeURIComponent(query)}&limit=30`,
  );
  const skills = data.data?.skills ?? [];
  return skills.map((s) => ({
    key: `mp:${s.id}`,
    source: "skillsmp",
    name: s.name,
    author: s.author,
    description: s.description,
    url: s.skillUrl,
    stars: s.stars,
    githubUrl: s.githubUrl,
  }));
}

interface HubCard {
  slug: string;
  name: string;
  description: string;
  author: string;
  source: string;
  installs: string;
  stars: string;
  shield: string;
}

export async function browseSkillHub(): Promise<HubEntry[]> {
  const data = await proxy<{ cards?: HubCard[] }>(`/skillhub?page=1&limit=36`);
  const cards = data.cards ?? [];
  return cards.map((c) => ({
    key: `hub:${c.slug}`,
    source: "skillhub",
    name: c.name,
    author: c.author,
    description: c.description,
    url: `https://skills.browseract.com/skills/${c.slug}`,
    slug: c.slug,
    installs: c.installs,
    shield: c.shield,
  }));
}

const emptyRepo = (fullName: string, url: string, stars = 0): RepoMeta => ({
  fullName,
  branch: "main",
  stars,
  license: null,
  pushedAt: null,
  htmlUrl: url,
});

/** Resolve a marketplace row into a scanned SKILL.md. */
export async function loadHubSkill(entry: HubEntry): Promise<HubLoaded> {
  if (entry.source === "skillsmp") {
    if (!entry.githubUrl) throw new Error("SkillsMP entry has no GitHub source.");
    const index = await fetchRepoIndex(entry.githubUrl, 5);
    const candidate =
      index.entries.find((e) => e.path.toLowerCase().includes(`/${entry.name.toLowerCase()}/`)) ??
      index.entries[0];
    if (!candidate) throw new Error("That directory has no SKILL.md.");
    return { skill: await loadSkill(candidate), repo: index.repo };
  }

  if (!entry.slug) throw new Error("Missing SkillHub slug.");
  const detail = await proxy<{ name: string; repo: string | null; shield: string | null; body: string }>(
    `/skillhub/skill?slug=${encodeURIComponent(entry.slug)}`,
  );
  if (!detail.body) throw new Error("SkillHub page had no SKILL.md body.");

  const { data, body } = parseFrontmatter(detail.body);
  const capabilities = scanSkill(body || detail.body, `${entry.slug}/SKILL.md`);
  const repo = detail.repo ? emptyRepo(detail.repo, `https://github.com/${detail.repo}`) : emptyRepo(entry.author, entry.url);

  return {
    skill: {
      key: entry.key,
      name: (data.name || detail.name || entry.name).trim(),
      description: (data.description || entry.description).slice(0, 240),
      path: `${entry.slug}/SKILL.md`,
      body: body || detail.body,
      allowedTools: data["allowed-tools"] ? data["allowed-tools"].split(/[,\s]+/).filter(Boolean) : null,
      siblings: [{ path: "SKILL.md", size: detail.body.length }],
      url: entry.url,
      grade: gradeFor(capabilities),
      capabilities,
      tokens: Math.max(120, Math.round((body || detail.body).length / 4)),
      doc: toDocBlocks(body || detail.body),
    },
    repo,
  };
}

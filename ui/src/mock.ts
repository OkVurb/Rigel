/**
 * Browser preview data.
 *
 * There is no Tauri host in `npm run dev`, so the shelf would be empty. Rather than fake a
 * database, this source says it is a sample in its own label and the shell prints
 * "browser preview" — the same honesty the Phase 1 shell had, now with the Phase 2 shape:
 * variants, evidence with an explicit `unmeasured` label, and a static review with lines.
 *
 * The interface is `SkillSource` from `api.ts`, which the real `TauriSkillSource` also
 * implements, so nothing in the UI knows which one it has.
 */

import type { SkillDetail, SkillRow, SkillSource, SourceStatus } from "./api";

const seen = Date.UTC(2026, 9, 19, 14, 5);

const ROWS: SkillRow[] = [
  {
    id: "github:anthropics/skills:skills/pdf/SKILL.md",
    name: "pdf",
    description:
      "Read, split, merge and fill PDF files: text and table extraction, watermarking, form fields.",
    source: "github",
    repo: "anthropics/skills",
    path: "skills/pdf/SKILL.md",
    url: "https://github.com/anthropics/skills/blob/main/skills/pdf/SKILL.md",
    grade: "B",
    grade_scope: "file-only",
    stars: 182_547,
    installs: null,
    tokens: 1_888,
    cluster_id: "cl-4f2a91c0d3ee",
    variants: 1,
    last_seen_at: seen,
    unmeasured: false,
    has_body: true,
  },
  {
    id: "github:affaan-m/ECC:skills/browser-qa/SKILL.md",
    name: "browser-qa",
    description: "Drive a real browser to check a flow end to end and report what broke.",
    source: "github",
    repo: "affaan-m/ECC",
    path: "skills/browser-qa/SKILL.md",
    url: "https://github.com/affaan-m/ECC/blob/main/skills/browser-qa/SKILL.md",
    grade: "C",
    grade_scope: "file-only",
    stars: 7_772,
    installs: 1_200_000,
    tokens: 940,
    cluster_id: "cl-9b1d77a4c2a1",
    variants: 1,
    last_seen_at: seen,
    unmeasured: false,
    has_body: true,
  },
  {
    id: "github:example/rtk:SKILL.md",
    name: "rtk",
    description: "Claims 60–90% token reduction by rewriting prompts before they are sent.",
    source: "github",
    repo: "example/rtk",
    path: "SKILL.md",
    url: "https://github.com/example/rtk/blob/main/SKILL.md",
    grade: "C",
    grade_scope: "file-only",
    stars: 412,
    installs: 19_000,
    tokens: 610,
    cluster_id: null,
    variants: 1,
    last_seen_at: seen,
    unmeasured: true,
    has_body: true,
  },
  {
    id: "github:acme/review:review/SKILL.md",
    name: "review",
    description: "Read the diff before the description and name the one risk that matters.",
    source: "github",
    repo: "acme/review",
    path: "review/SKILL.md",
    url: "https://github.com/acme/review/blob/main/review/SKILL.md",
    grade: "B",
    grade_scope: "file-only",
    stars: 64,
    installs: null,
    tokens: 420,
    cluster_id: "cl-71c0a9b3f10d",
    variants: 3,
    last_seen_at: seen,
    unmeasured: false,
    has_body: true,
  },
  {
    id: "github:example/deploy-helper:SKILL.md",
    name: "deploy-helper",
    description: "One-command deploys. Reads your cloud credentials at runtime.",
    source: "github",
    repo: "example/deploy-helper",
    path: "SKILL.md",
    url: "https://github.com/example/deploy-helper/blob/main/SKILL.md",
    grade: "F",
    grade_scope: "file-only",
    stars: 12,
    installs: 312,
    tokens: 380,
    cluster_id: null,
    variants: 1,
    last_seen_at: seen,
    unmeasured: false,
    has_body: true,
  },
  {
    id: "skillsmp:6f1c",
    name: "figma-to-code",
    description: "Turn a Figma frame into components. Listed on SkillsMP; no body indexed yet.",
    source: "skillsmp",
    repo: "acme/figma-skills",
    path: null,
    url: "https://skillsmp.com/skills/figma-to-code",
    grade: null,
    grade_scope: null,
    stars: 240,
    installs: null,
    tokens: null,
    cluster_id: null,
    variants: 1,
    last_seen_at: seen,
    unmeasured: false,
    has_body: false,
  },
];

function detailFor(row: SkillRow): SkillDetail {
  const capabilities =
    row.grade === "F"
      ? [
          {
            label: "Reads secrets and env files",
            severity: "critical" as const,
            evidence: [
              { file: row.path ?? "SKILL.md", line: 12, code: "export $(cat .env | xargs)" },
            ],
          },
        ]
      : row.grade === "C"
        ? [
            {
              label: "Network access",
              severity: "high" as const,
              evidence: [
                { file: row.path ?? "SKILL.md", line: 24, code: "curl -s https://api.example.dev" },
              ],
            },
          ]
        : [
            {
              label: "No dangerous capabilities detected",
              severity: "low" as const,
              evidence: [
                {
                  file: row.path ?? "SKILL.md",
                  line: 1,
                  code: "single markdown file — no scripts bundled",
                },
              ],
            },
          ];

  const evidence =
    row.unmeasured && row.name === "rtk"
      ? [
          {
            metric: "token-reduction",
            measured: null,
            advertised: 90,
            source: "description",
            source_url: row.url,
            observed_at: seen,
            label: "unmeasured" as const,
          },
        ]
      : [];

  const variants =
    row.variants > 1
      ? [
          {
            id: `${row.id}@fork`,
            name: row.name,
            repo: "someone/fork",
            grade: "B" as const,
            license: null,
            last_seen_at: seen - 86_400_000 * 240,
            delta: "3 bits from this one · no license",
          },
          {
            id: `${row.id}@copy`,
            name: row.name,
            repo: "another/copy",
            grade: "C" as const,
            license: "MIT",
            last_seen_at: seen - 86_400_000 * 900,
            delta: "identical content",
          },
        ]
      : [];

  return {
    skill: row,
    capabilities,
    evidence,
    variants,
    first_seen_at: seen - 86_400_000 * 30,
  };
}

export class PreviewSkillSource implements SkillSource {
  readonly label = "sample rows (browser preview)";

  private matches(query: string): SkillRow[] {
    const q = query.trim().toLowerCase();
    if (!q) return ROWS;
    return ROWS.filter(
      (row) =>
        row.name.toLowerCase().includes(q) ||
        row.description.toLowerCase().includes(q) ||
        (row.repo ?? "").toLowerCase().includes(q),
    );
  }

  async search(query: string, limit = 100, offset = 0): Promise<SkillRow[]> {
    return this.matches(query).slice(offset, offset + limit);
  }

  async count(query: string): Promise<number> {
    return this.matches(query).length;
  }

  async load(id: string): Promise<SkillDetail> {
    const row = ROWS.find((candidate) => candidate.id === id);
    if (!row) throw new Error("not in the preview");
    return detailFor(row);
  }

  private sources: SourceStatus[] = [
    {
      id: "github",
      base_url: "https://api.github.com",
      state: "ok",
      last_sync_at: seen,
      error: null,
    },
    {
      id: "skillsmp",
      base_url: "https://skillsmp.com",
      state: "stale",
      last_sync_at: seen,
      error: null,
    },
    {
      id: "skillhub",
      base_url: "https://skills.browseract.com",
      state: "broken",
      last_sync_at: seen,
      error: "connection reset",
    },
  ];

  async status(): Promise<SourceStatus[]> {
    return this.sources;
  }

  /**
   * Browser preview only: the same pass the window runs, described honestly, with the stale rows
   * becoming fresh — which is exactly what `rigel skills refresh` does to the real shelf.
   */
  async refresh() {
    const before = this.sources;
    this.sources = before.map((row) => ({ ...row, state: "ok", error: null }));
    return {
      checked: before.length,
      refreshed: before.filter((row) => row.state !== "ok").length,
      sources: before.map((row) => ({
        id: row.id,
        state: "ok",
        detail:
          row.state === "ok"
            ? "fresh"
            : row.id === "github"
              ? "3 repo(s), 7 row(s) updated"
              : "30 row(s) from the default query",
      })),
    };
  }
}

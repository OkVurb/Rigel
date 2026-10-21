/**
 * The shelf's data seam.
 *
 * Phase 1 renders `MockSkillSource`. Phase 2 replaces it with the SQLite-backed source and
 * deletes this file's mock rows — the interface is the contract, so the UI does not change
 * when that happens.
 */

export type Grade = "A" | "B" | "C" | "D" | "F";

export interface SkillRow {
  id: string;
  name: string;
  description: string;
  source: string;
  grade: Grade;
  installs: number;
  installed: boolean;
}

export interface SkillSource {
  search(query: string): Promise<SkillRow[]>;
}

const ROWS: SkillRow[] = [
  {
    id: "github:anthropics/frontend-design",
    name: "frontend-design",
    description: "Distinctive, production-grade frontend interfaces that avoid template default.",
    source: "github",
    grade: "A",
    installs: 212_000,
    installed: true,
  },
  {
    id: "github:dietrichgebert/ponytail",
    name: "ponytail",
    description: "Makes agents write less code by refusing to build what the task does not need.",
    source: "github",
    grade: "A",
    installs: 212_000,
    installed: false,
  },
  {
    id: "github:affaan-m/ECC",
    name: "ECC",
    description: "Operating layer for agent harnesses: 311 skills plus hooks and routing.",
    source: "github",
    grade: "B",
    installs: 1_200_000,
    installed: true,
  },
  {
    id: "github:example/caveman",
    name: "caveman",
    description: "Compresses agent prose to reduce output tokens without losing the instruction.",
    source: "github",
    grade: "B",
    installs: 44_000,
    installed: false,
  },
  {
    id: "github:example/rtk",
    name: "rtk",
    description: "Claims ~60–90% token reduction. Measured result was 18%.",
    source: "github",
    grade: "C",
    installs: 19_000,
    installed: false,
  },
  {
    id: "github:example/chrome-devtools",
    name: "chrome-devtools",
    description: "Lets the agent inspect and drive a live Chrome tab through MCP.",
    source: "github",
    grade: "B",
    installs: 96_000,
    installed: true,
  },
  {
    id: "github:example/deploy-helper",
    name: "deploy-helper",
    description: "One-command deploys. Reads your cloud credentials at runtime.",
    source: "github",
    grade: "F",
    installs: 312,
    installed: false,
  },
];

export class MockSkillSource implements SkillSource {
  async search(query: string): Promise<SkillRow[]> {
    const q = query.trim().toLowerCase();
    if (!q) return ROWS;
    return ROWS.filter(
      (row) =>
        row.name.toLowerCase().includes(q) ||
        row.description.toLowerCase().includes(q),
    );
  }
}

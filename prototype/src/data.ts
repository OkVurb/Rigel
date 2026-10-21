export type Kind = "skill" | "bundle" | "mcp" | "prompt_pack";
export type Grade = "A" | "B" | "C" | "D" | "F";
export type Severity = "critical" | "high" | "low";
export type Verdict = "confirmed" | "overstated" | "worse";

export interface Capability {
  label: string;
  severity: Severity;
  evidence: { file: string; line: number; code: string }[];
}

export interface Variant {
  name: string;
  note: string;
  canonical?: boolean;
  stars: number;
  lastCommit: string;
  grade: Grade;
}

export interface Evidence {
  metric: string;
  measured: string;
  advertised: string;
  verdict: Verdict;
}

export type DocBlock =
  | { h: string }
  | { p: string }
  | { ul: string[] };

export interface Package {
  id: string;
  name: string;
  kind: Kind;
  author: string;
  description: string;
  grade: Grade;
  installs: number;
  stars: number;
  lastCommit: string;
  license: string | null;
  curated?: boolean;
  variants?: Variant[];
  capabilities: Capability[];
  files: { path: string; size: string }[];
  evidence?: Evidence[];
  evidenceSource?: string;
  evidenceDate?: string;
  evidenceNote?: string;
  tokens: number;
  doc: DocBlock[];
  /** set when the package was pulled from GitHub in this session */
  source?: { repo: string; branch: string; path: string; url: string };
  unverified?: boolean;
}

const fmt = (n: number) =>
  n >= 1_000_000
    ? `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`
    : n >= 1000
      ? `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k`
      : String(n);

export const packages: Package[] = [
  {
    id: "affaan-m/ECC",
    name: "ECC",
    kind: "bundle",
    author: "affaan-m",
    description:
      "Operating layer for agent harnesses: 311 skills plus agents, hooks, memory and security tooling.",
    grade: "B",
    installs: 1_200_000,
    stars: 8_412,
    lastCommit: "3 days ago",
    license: "MIT",
    curated: true,
    tokens: 12_400,
    capabilities: [
      {
        label: "Spawns processes and installs packages",
        severity: "high",
        evidence: [
          { file: "install/guided.ts", line: 88, code: "execSync(`npm i ${pkg}`, { stdio: 'inherit' })" },
        ],
      },
      {
        label: "Network access",
        severity: "high",
        evidence: [
          { file: "agent-shield/registry.ts", line: 41, code: "await fetch('https://api.ecc.tools/v1/advisories')" },
        ],
      },
      {
        label: "Writes inside project only",
        severity: "low",
        evidence: [
          { file: "skills/_shared/write.ts", line: 12, code: "join(repoRoot, relPath)" },
        ],
      },
    ],
    files: [
      { path: "skills/", size: "311 dirs" },
      { path: "agents/", size: "18 files" },
      { path: "hooks/hooks.json", size: "2.1 KB" },
      { path: "agent-shield/", size: "9 files" },
      { path: ".claude-plugin/plugin.json", size: "640 B" },
      { path: "LICENSE", size: "1.1 KB" },
    ],
    doc: [
      { p: "ECC is a bundle, not a single skill. Installing it adds a shared working method — planning, memory, review and security checks — that the same agent can follow across Claude Code, Codex, Cursor, OpenCode and Kimi." },
      { h: "What you get" },
      { ul: ["311 skills across TDD, research, security, docs, frontend, data and ops", "18 agent definitions and a hooks manifest", "AgentShield: static review of third-party skills before install", "One canonical store, materialised into each harness's expected layout"] },
      { h: "Why it is a bundle" },
      { p: "Each skill is independently installable, but the value comes from the shared contract between them. This is the reason the package model in this app is generic rather than skill-shaped." },
    ],
  },
  {
    id: "dietrichgebert/ponytail",
    name: "ponytail",
    kind: "skill",
    author: "dietrichgebert",
    description: "Makes agents write less code by refusing to build what the task doesn't need.",
    grade: "A",
    installs: 212_000,
    stars: 3_180,
    lastCommit: "5 days ago",
    license: "MIT",
    curated: true,
    tokens: 1_150,
    variants: [
      { name: "dietrichgebert/ponytail", note: "best maintained · 3.2k stars", canonical: true, stars: 3180, lastCommit: "5 days ago", grade: "A" },
      { name: "mattpocock/ponytail", note: "identical body, older name · 14 months stale", stars: 940, lastCommit: "14 months ago", grade: "A" },
      { name: "ponytail-skill", note: "fork, reformatted · no license", stars: 212, lastCommit: "8 months ago", grade: "C" },
      { name: "ponytail-min", note: "trimmed to 400 tokens · loses upgrade-path rules", stars: 88, lastCommit: "2 months ago", grade: "B" },
    ],
    evidence: [
      { metric: "code written", measured: "−15%", advertised: "−54%", verdict: "overstated" },
      { metric: "cost", measured: "−10.3%", advertised: "−20%", verdict: "overstated" },
      { metric: "wall time", measured: "−11%", advertised: "−27%", verdict: "overstated" },
    ],
    evidenceSource: "JetBrains AI, 80 paired tasks",
    evidenceDate: "28 Jul 2026",
    evidenceNote: "No detectable quality difference. The code reduction only appears on tasks with room to over-build — on tightly-specified work the effect disappears.",
    capabilities: [
      {
        label: "Read-only — no scripts bundled",
        severity: "low",
        evidence: [{ file: "SKILL.md", line: 1, code: "allowed-tools: Read, Grep, Glob" }],
      },
    ],
    files: [
      { path: "SKILL.md", size: "4.2 KB" },
      { path: "references/over-build-patterns.md", size: "2.8 KB" },
      { path: "LICENSE", size: "1.1 KB" },
    ],
    doc: [
      { p: "Ponytail is a behavioural constraint: before writing code, the agent must justify each file it intends to create against the requirement, and must prefer editing an existing file over adding a new one." },
      { h: "When it helps" },
      { ul: ["Greenfield features where the model tends to scaffold aggressively", "Tasks phrased loosely, where the agent invents scope", "Situations where review cost dominates the work"] },
      { h: "When it does nothing" },
      { p: "On precisely specified work — a bug fix with a failing test — there is no over-building to remove, and measured savings fall to noise." },
      { h: "Debt marking" },
      { p: "Every deliberate shortcut gets a `shortcut:` comment naming its ceiling and upgrade path. The companion `ponytail-debt` skill audits those markers later." },
    ],
  },
  {
    id: "anthropics/frontend-design",
    name: "frontend-design",
    kind: "skill",
    author: "anthropics",
    description: "Distinctive, production-grade frontend interfaces that reject generic AI aesthetics.",
    grade: "A",
    installs: 1_000_000,
    stars: 24_800,
    lastCommit: "6 weeks ago",
    license: "Apache-2.0",
    curated: true,
    tokens: 2_400,
    capabilities: [
      { label: "Read-only — no scripts bundled", severity: "low", evidence: [{ file: "SKILL.md", line: 1, code: "allowed-tools: Read, Write" }] },
    ],
    files: [
      { path: "SKILL.md", size: "7.9 KB" },
      { path: "references/typography.md", size: "3.4 KB" },
      { path: "references/motion.md", size: "2.2 KB" },
    ],
    doc: [
      { p: "Forces the agent to choose an aesthetic direction before generating interface code, then holds it to that direction through typography, colour, spatial composition and motion." },
      { h: "Explicitly avoids" },
      { ul: ["Overused fonts and clichéd colour schemes", "Predictable three-column layouts", "Decorative gradients that carry no meaning"] },
    ],
  },
  {
    id: "sickn33/caveman",
    name: "caveman",
    kind: "skill",
    author: "sickn33",
    description: "Compresses agent prose to reduce output tokens. Advertised −65%, measured −8.5%.",
    grade: "B",
    installs: 44_300,
    stars: 1_910,
    lastCommit: "2 months ago",
    license: "MIT",
    tokens: 620,
    variants: [
      { name: "sickn33/caveman", note: "best maintained", canonical: true, stars: 1910, lastCommit: "2 months ago", grade: "B" },
      { name: "caveman-pro", note: "same body + prompt-injection footer", stars: 301, lastCommit: "7 months ago", grade: "D" },
    ],
    evidence: [
      { metric: "output tokens", measured: "−8.5%", advertised: "−65%", verdict: "overstated" },
    ],
    evidenceSource: "JetBrains AI, paired A/B",
    evidenceDate: "Jul 2026",
    evidenceNote: "Compression mostly removes text the model would not have written anyway at this prompt length.",
    capabilities: [
      { label: "Read-only — no scripts bundled", severity: "low", evidence: [{ file: "SKILL.md", line: 1, code: "allowed-tools: Read" }] },
    ],
    files: [{ path: "SKILL.md", size: "2.1 KB" }],
    doc: [
      { p: "Rewrites the agent's system prompt into telegraphic style to cut output tokens." },
      { h: "Caveat" },
      { p: "The measured saving is roughly an eighth of the advertised one. Two of the three variants in this cluster are safe; one adds a footer instructing the agent to ignore prior instructions." },
    ],
  },
  {
    id: "rtk/token-saver",
    name: "rtk",
    kind: "skill",
    author: "rtk",
    description: "Claims −60–90% token reduction. Measured result was worse than baseline.",
    grade: "C",
    installs: 18_700,
    stars: 780,
    lastCommit: "4 months ago",
    license: null,
    tokens: 980,
    evidence: [
      { metric: "tokens", measured: "+7.6%", advertised: "−60–90%", verdict: "worse" },
    ],
    evidenceSource: "JetBrains AI, paired A/B",
    evidenceDate: "Jul 2026",
    evidenceNote: "Added retry loops around compaction that increased total tokens. Do not install on the basis of the claim.",
    capabilities: [
      { label: "Writes inside project only", severity: "low", evidence: [{ file: "SKILL.md", line: 1, code: "allowed-tools: Read, Write, Edit" }] },
    ],
    files: [{ path: "SKILL.md", size: "3.3 KB" }],
    doc: [
      { p: "A token-reduction skill that re-summarises context on a fixed cadence." },
      { h: "Measured outcome" },
      { p: "The summarisation triggers extra round-trips that cost more than they save. The claim is unsubstantiated and the measured direction is negative." },
    ],
  },
  {
    id: "chrome-devtools/mcp",
    name: "chrome-devtools",
    kind: "mcp",
    author: "ChromeDevTools",
    description: "Lets the agent inspect and drive a live Chrome tab: DOM, network, console, screenshots.",
    grade: "B",
    installs: 96_400,
    stars: 6_120,
    lastCommit: "1 week ago",
    license: "Apache-2.0",
    tokens: 3_100,
    capabilities: [
      { label: "Network access", severity: "high", evidence: [{ file: "src/browser.ts", line: 33, code: "ws://127.0.0.1:9222/devtools/browser" }] },
      { label: "Spawns processes", severity: "high", evidence: [{ file: "src/launch.ts", line: 19, code: "spawn(chromePath, ['--remote-debugging-port=9222'])" }] },
    ],
    files: [
      { path: "src/index.ts", size: "8.2 KB" },
      { path: "src/browser.ts", size: "12.4 KB" },
      { path: "package.json", size: "1.4 KB" },
    ],
    doc: [
      { p: "An MCP server that attaches to a running Chrome instance over the DevTools protocol, exposing tools for navigation, DOM inspection, network capture and screenshots." },
      { h: "Install" },
      { p: "Runs locally over stdio; launches Chrome with a debugging port if none is attached." },
    ],
  },
  {
    id: "someone/deploy-helper",
    name: "deploy-helper",
    kind: "skill",
    author: "someone",
    description: "One-command deploys. Reads your cloud credentials and rewrites shell profiles.",
    grade: "F",
    installs: 312,
    stars: 14,
    lastCommit: "2 years ago",
    license: null,
    tokens: 2_050,
    capabilities: [
      {
        label: "Reads secrets and env files",
        severity: "critical",
        evidence: [
          { file: "scripts/deploy.sh", line: 7, code: 'source "$HOME/.aws/credentials"' },
          { file: "scripts/deploy.sh", line: 11, code: "cat .env >> /tmp/deploy.log" },
        ],
      },
      {
        label: "Writes outside the project",
        severity: "critical",
        evidence: [
          { file: "scripts/install.sh", line: 4, code: 'echo "export DEPLOY=1" >> ~/.bashrc' },
        ],
      },
      {
        label: "Destructive command",
        severity: "critical",
        evidence: [
          { file: "scripts/clean.sh", line: 3, code: "rm -rf $PROJECT_DIR/dist/*" },
        ],
      },
    ],
    files: [
      { path: "SKILL.md", size: "1.2 KB" },
      { path: "scripts/deploy.sh", size: "880 B" },
      { path: "scripts/install.sh", size: "410 B" },
      { path: "scripts/clean.sh", size: "190 B" },
    ],
    doc: [
      { p: "Deploys a project to a cloud host by sourcing credentials and calling a CLI." },
      { h: "Why this is graded F" },
      { p: "Three critical capabilities were detected in static review, the manifest declares none of them, and the package has no license and no maintenance in two years. The declared/actual mismatch is the disqualifying signal." },
    ],
  },
];

export const totalInstalled = 3;
export const sessionCost = 0.42;
export const sessionTokens = 18_240;
export const modelName = "sonnet-4.6";
export const branch = "feat/package-browser";

export { fmt };

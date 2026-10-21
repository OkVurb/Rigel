import type { Kind } from "./data";

/**
 * Measured on this machine, 20 Oct 2026, after `ecc-universal` 2.2.3 was installed
 * into every harness that has an install target:
 *
 *   node scripts/verify-ecc.mjs          -> structure, frontmatter, hook targets
 *   the ecc install-state.json per root -> managed files, skipped modules
 *
 * Nothing here is estimated. The counts come from reading the stores and the
 * install state, which is why Codex and Cursor disagree so loudly.
 */

export type CheckStatus = "pass" | "warn" | "fail";
export type TargetStatus = "ready" | "degraded" | "inherited";

export interface Check {
  label: string;
  status: CheckStatus;
  detail: string;
}

export interface Hosts {
  skills: boolean;
  agents: boolean;
  hooks: boolean;
  mcp: boolean;
}

export interface Target {
  id: string;
  name: string;
  adapter: string;
  root: string;
  skillsPath: string;
  /** skill dirs containing SKILL.md */
  skills: number;
  /** skill dirs written by the ECC installer */
  eccSkills: number | null;
  /** files recorded in install-state.json */
  eccFiles: number | null;
  agents: number | null;
  hookEvents: number | null;
  skipped: string[];
  status: TargetStatus;
  canonical?: boolean;
  summary: string;
  hosts: Hosts;
  checks: Check[];
  doc: string[];
}

export const targets: Target[] = [
  {
    id: "claude",
    name: "Claude Code",
    adapter: "native plugin",
    root: "~/.claude",
    skillsPath: "~/.claude/skills",
    skills: 212,
    eccSkills: 126,
    eccFiles: 767,
    agents: 68,
    hookEvents: 7,
    skipped: [],
    status: "ready",
    summary:
      "The reference target. Every ECC module lands, hooks are wired, and the installer's own doctor reports no drift.",
    hosts: { skills: true, agents: true, hooks: true, mcp: true },
    checks: [
      { label: "All modules selected", status: "pass", detail: "10/10 modules, 767 managed files" },
      { label: "Skills resolve", status: "pass", detail: "212 dirs, 212 valid frontmatter — symlinked cloud skills included" },
      { label: "Hooks wired", status: "pass", detail: "7 events, 25 targets; 24 resolve at runtime from a base64 root" },
      { label: "Existing config respected", status: "pass", detail: "settings.json merged, not replaced — backup kept" },
    ],
    doc: [
      "ECC installs as a native plugin here, which is the only target where the full module set applies. That makes Claude Code the reference: anything that fails elsewhere is an adapter gap, not a packaging problem.",
      "The hooks are the interesting part. They are not literal file paths but `node -e` one-liners that decode their own root at runtime, so a static path check has to understand that form or it reports false failures — mine did until I taught it.",
    ],
  },
  {
    id: "codex",
    name: "Codex",
    adapter: "home adapter",
    root: "~/.codex",
    skillsPath: "~/.codex/skills",
    skills: 2555,
    eccSkills: 49,
    eccFiles: 181,
    agents: 71,
    hookEvents: null,
    skipped: ["rules-core", "commands-core", "hooks-runtime", "framework-language", "orchestration"],
    status: "degraded",
    summary:
      "Has the largest skill store on the machine and the thinnest ECC install: five modules were skipped by the adapter.",
    hosts: { skills: true, agents: true, hooks: false, mcp: true },
    checks: [
      { label: "ECC files present", status: "pass", detail: "181 files, 49 skills, 71 agents" },
      { label: "Five modules skipped", status: "warn", detail: "rules-core · commands-core · hooks-runtime · framework-language · orchestration" },
      { label: "Hooks impossible", status: "warn", detail: "Adapter skips hooks-runtime — consent alone does not wire it" },
      { label: "2,477 skills live only here", status: "warn", detail: "Visible to no other harness on this machine" },
    ],
    doc: [
      "Codex is the clearest argument for a canonical store. It holds 2,555 skills — ten times the shared store — because they were bulk-imported straight into `~/.codex/skills` rather than placed anywhere portable.",
      "ECC itself only reached 49 skills here. When you asked for hooks everywhere, the Codex adapter still skipped `hooks-runtime`, along with the rules and commands modules. Nothing is broken; the adapter simply does not implement them, and the installer records that in `resolution.skippedModules` instead of failing.",
    ],
  },
  {
    id: "agents",
    name: "Cross-agent store",
    adapter: "shared .agents",
    root: "~/.agents",
    skillsPath: "~/.agents/skills",
    skills: 253,
    eccSkills: 125,
    eccFiles: 511,
    agents: 68,
    hookEvents: null,
    skipped: ["hooks-runtime", "orchestration"],
    status: "ready",
    canonical: true,
    summary:
      "The portability layer. Codex, Cursor, Cline and Copilot all read this path, so one copy here serves five harnesses.",
    hosts: { skills: true, agents: true, hooks: false, mcp: false },
    checks: [
      { label: "Canonical source", status: "pass", detail: "Read by Codex, Cursor, Cline and Copilot" },
      { label: "Skills resolve", status: "pass", detail: "253 dirs, 252 valid — one thin description, reported not failed" },
      { label: "Hooks impossible", status: "warn", detail: "hooks-runtime skipped by the adapter" },
      { label: "Most skills are not here", status: "warn", detail: "Only 59 of 2,737 exist in all four stores" },
    ],
    doc: [
      "This is the directory that answers \"does it work on whatever agent I use\". Skills written to `~/.agents/skills` are read by Codex, Cursor, Cline and Copilot without per-harness installs.",
      "It is also the narrowest pipe: 253 of your 2,737 distinct skills are addressed here, and only 59 are visible from every store at once. Everything else is stranded in whichever harness it was installed into.",
    ],
  },
  {
    id: "cursor",
    name: "Cursor",
    adapter: "home adapter",
    root: "~/.cursor",
    skillsPath: "~/.cursor/skills",
    skills: 131,
    eccSkills: 147,
    eccFiles: 866,
    agents: 68,
    hookEvents: 15,
    skipped: ["orchestration"],
    status: "ready",
    summary:
      "Largest ECC footprint after repair: 147 skills and 15 hook events, once 16 relative paths were rewritten to absolute.",
    hosts: { skills: true, agents: true, hooks: true, mcp: true },
    checks: [
      { label: "ECC files present", status: "pass", detail: "866 files, 147 skills, 68 agents" },
      { label: "Hooks wired", status: "pass", detail: "15 events, 16 targets — all present" },
      { label: "Paths repaired", status: "warn", detail: "Installer wrote project-relative paths into a user-level config; now absolute" },
      { label: "MCP merged", status: "pass", detail: "chrome-devtools added to mcp.json" },
    ],
    doc: [
      "Cursor got the biggest install and shipped with the worst defect. `~/.cursor/hooks.json` received commands like `node .cursor/hooks/session-start.js` — project-relative paths in a user-level file, run from whatever workspace is open. Every hook would have failed silently in any real project.",
      "All 16 were rewritten to absolute paths. This will regress on the next `ecc install --target cursor`, which is why the repair is a script rather than a commit.",
    ],
  },
  {
    id: "cline",
    name: "Cline",
    adapter: "inherits",
    root: "~/.cline",
    skillsPath: "~/.claude/skills",
    skills: 212,
    eccSkills: 0,
    eccFiles: 0,
    agents: null,
    hookEvents: null,
    skipped: [],
    status: "inherited",
    summary:
      "No store of its own. Cline reads the Claude Code path, so ECC arrived without a single file being written for it.",
    hosts: { skills: true, agents: false, hooks: false, mcp: false },
    checks: [
      { label: "Zero files written", status: "pass", detail: "No ~/.cline/skills — inherited by design" },
      { label: "Inherits 212 skills", status: "pass", detail: "Via ~/.claude/skills" },
      { label: "No hooks or agents", status: "warn", detail: "Inheritance carries skills only" },
    ],
    doc: [
      "Cline is why the install matrix is not one column per harness. There is no `~/.cline/skills` to write to; Cline reads the Claude Code skills directory, so the 212 skills there are already visible to it.",
      "That inheritance is also fragile: it couples two harnesses through a path owned by one of them. The `.agents` store is the more durable seam.",
    ],
  },
];

/** Store sizes measured across the four real stores. */
export const divergence = {
  union: 2737,
  sharedByAll: 59,
  singleStore: 2510,
  stores: [
    { id: "codex", label: "Codex", count: 2555 },
    { id: "agents", label: ".agents", count: 253 },
    { id: "claude", label: "Claude Code", count: 212 },
    { id: "cursor", label: "Cursor", count: 131 },
  ],
  examples: ["00-andruia-consultant", "2slides-ppt-generator", "007", "10-andruia-skill-smith"],
};

export const measuredAt = "20 Oct 2026";

export function runsOn(kind: Kind, t: Target): "full" | "partial" | "no" {
  if (kind === "mcp") return t.hosts.mcp ? "full" : "no";
  if (kind === "skill" || kind === "prompt_pack") return t.hosts.skills ? "full" : "no";
  // bundles need skills + agents + hooks to behave as designed
  if (!t.hosts.skills) return "no";
  return t.hosts.agents && t.hosts.hooks ? "full" : "partial";
}

export const skippedTotal = targets.reduce((n, t) => n + t.skipped.length, 0);

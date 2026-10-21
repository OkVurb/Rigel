export type SourceTab = "all" | "github" | "skillsmp" | "skillhub" | "deps";

export const SOURCES: { id: SourceTab; label: string; hint: string }[] = [
  { id: "all", label: "All", hint: "everything discovered so far" },
  { id: "github", label: "GitHub", hint: "repos read directly" },
  { id: "skillsmp", label: "SkillsMP", hint: "3M+ skill marketplace" },
  { id: "skillhub", label: "SkillHub", hint: "skills.sh mirror, 16,494 skills" },
  { id: "deps", label: "Dependencies", hint: "tools an agent may need" },
];

import { useState } from "react";
import type { ReactNode } from "react";
import type { Grade } from "../data";
import { agentBrief, downloadText } from "../db";

export function GradeBadge({ grade }: { grade: Grade }) {
  return (
    <span className={`grade grade-${grade.toLowerCase()}`} title={`Safety grade ${grade}`}>
      {grade}
    </span>
  );
}

export function Chip({
  children,
  tone = "default",
  title,
}: {
  children: ReactNode;
  tone?: "default" | "curated" | "clone" | "warn";
  title?: string;
}) {
  return (
    <span className={`chip ${tone === "default" ? "" : `chip-${tone}`}`} title={title}>
      {children}
    </span>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <span className="kbd">{children}</span>;
}

export function CopyButton({ text, label = "Copy brief" }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      className="ghost-btn"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1500);
        } catch {
          setCopied(false);
        }
      }}
    >
      {copied ? "✓ Copied" : label}
    </button>
  );
}

/** Download the record as Markdown, or hand the agent a text brief. */
export function AgentActions({ pkg }: { pkg: Parameters<typeof agentBrief>[0] }) {
  return (
    <div className="agent-actions">
      <button className="ghost-btn" onClick={() => downloadText(`${pkg.name}.md`, agentBrief(pkg), "text/markdown;charset=utf-8")}>
        ⤓ .md
      </button>
      <CopyButton text={agentBrief(pkg)} />
    </div>
  );
}

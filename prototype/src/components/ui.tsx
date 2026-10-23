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

/** Rigel's mark: a single bright star, the anchor the name points to. */
export function RigelMark({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <circle cx="12" cy="12" r="5.5" fill="currentColor" fillOpacity="0.14" />
      <path
        d="M12 4 Q12.9 11.1 20 12 Q12.9 12.9 12 20 Q11.1 12.9 4 12 Q11.1 11.1 12 4 Z"
        fill="currentColor"
      />
    </svg>
  );
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

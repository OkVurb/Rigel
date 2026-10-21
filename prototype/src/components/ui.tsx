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

/** Minimal Orion: the belt as a chain of stars, two bright anchors, one shoulder. */
export function OrionMark({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <path
        d="M7.2 5.6 10.6 12.4 12.6 11.9 14.6 11.4 17.4 18.6"
        stroke="currentColor"
        strokeWidth="1.7"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <g fill="currentColor">
        <circle cx="10.6" cy="12.4" r="1.7" />
        <circle cx="12.6" cy="11.9" r="1.7" />
        <circle cx="14.6" cy="11.4" r="1.7" />
        <circle cx="15.4" cy="6.2" r="1.1" />
        <circle cx="7.2" cy="5.6" r="2.4" />
        <circle cx="17.4" cy="18.6" r="2.2" />
      </g>
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

/**
 * Approval card v2 (roadmap 135).
 *
 * Reading order is fixed: the command, then what it would touch, then the scope the grant will
 * last for, then the snapshot state. The old card led with policy pills and put the command in a
 * header, which made "blast radius" the first thing a person read after the countdown.
 *
 * One primary action exists — Allow once. The three wider scopes are secondary chips, and Reject
 * is a real button at the end of the tab order with Escape as a shortcut, so a keyboard user can
 * always refuse without reaching for the mouse. A `cannot_determine` row is never wrapped in a
 * disclosure: an unknown blast radius is the reason to look, not a detail to open.
 */
import { useEffect, useState } from "react";

import type { ApprovalAnswer, PendingApproval } from "../api";

/** The asker's own deadline; the card never starts a second clock. */
function deadlineOf(pending: PendingApproval, mountedAt: number): number {
  const detail = pending.detail ?? {};
  for (const key of ["expires_at", "deadline", "deadline_ms"]) {
    const value = detail[key];
    if (typeof value === "number" && Number.isFinite(value)) return value;
  }
  return mountedAt + 10 * 60_000;
}

function countdownOf(remainingMs: number): string {
  const seconds = Math.max(0, Math.floor(remainingMs / 1_000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

type BlastTarget = {
  path?: string;
  kind?: string;
  files?: string[];
  file_count?: number | null;
  byte_count?: number | null;
  note?: string | null;
};

export function ApprovalCard({
  pending,
  refused,
  retryDisabled,
  onAnswer,
  onRetry,
  onDismiss,
}: {
  pending: PendingApproval;
  /** Q246: the question is over; only Retry and Dismiss remain. */
  refused?: boolean;
  retryDisabled?: boolean;
  onAnswer: (
    pending: PendingApproval,
    scope: ApprovalAnswer,
    options?: { skip?: boolean; note?: string },
  ) => void;
  onRetry?: (pending: PendingApproval) => void;
  onDismiss?: (pending: PendingApproval) => void;
}) {
  const detail = pending.detail ?? {};
  const [mountedAt] = useState(() => Date.now());
  const deadline = deadlineOf(pending, mountedAt);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (refused) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [refused, pending.id]);
  const lapsed = refused || now >= deadline;
  const countdown = countdownOf(deadline - now);
  const [skippingPlan, setSkippingPlan] = useState(false);
  const [skipReason, setSkipReason] = useState("");

  const cwd = typeof detail.cwd === "string" ? detail.cwd : null;
  const timeout = typeof detail.timeout_secs === "number" ? detail.timeout_secs : null;
  const destructive = Array.isArray(detail.destructive)
    ? (detail.destructive as { id?: string; summary?: string }[])
    : [];
  const blast =
    detail.blast_radius && typeof detail.blast_radius === "object"
      ? (detail.blast_radius as {
          commands?: string[];
          targets?: BlastTarget[];
          cannot_determine?: string[];
        })
      : null;
  // Q73: the tool's own preview. `supported: false` is still an answer: the card says the
  // effect cannot be previewed rather than filling the gap with a guess.
  const dryRun =
    detail.dry_run && typeof detail.dry_run === "object"
      ? (detail.dry_run as {
          command?: string;
          supported?: boolean;
          output?: string;
          note?: string | null;
        })
      : null;
  // Q243: the exact script paths a loaded skill contributed to this command.
  const skillScripts = Array.isArray(detail.skill_scripts)
    ? (detail.skill_scripts as { skill_id?: string; path?: string }[])
    : [];
  // Q242: the real `tools/list` of a project MCP server.
  const mcpTools = Array.isArray(detail.tools)
    ? (detail.tools as { name?: string; description?: string }[])
    : [];
  const deliberation =
    detail.deliberation && typeof detail.deliberation === "object"
      ? (detail.deliberation as { note?: string })
      : null;
  const planned =
    pending.kind === "plan" && Array.isArray(detail.agents)
      ? (detail.agents as { role?: string; files?: string[]; task?: string; plan?: string }[])
      : [];

  // D32: a destructive command only runs after a snapshot of work with no other copy. The card
  // says which of the two situations it is in, last, because the decision above already carries
  // the consequence.
  const reportedSnapshot =
    typeof detail.snapshot === "string"
      ? detail.snapshot
      : detail.snapshot && typeof detail.snapshot === "object"
        ? JSON.stringify(detail.snapshot)
        : null;
  const snapshotState = reportedSnapshot
    ? reportedSnapshot
    : destructive.length > 0
      ? "a snapshot is taken before this command runs; refusing leaves the tree untouched"
      : "not needed — this command does not delete anything";

  const deny = () => onAnswer(pending, "deny");

  if (pending.kind === "plan") {
    return (
      <section
        className="approval-card"
        aria-label="Writable agents awaiting approval"
        data-order-host="plan"
        onKeyDown={(event) => {
          if (event.key === "Escape" && !lapsed) {
            event.preventDefault();
            deny();
          }
        }}
      >
        <header>
          <span className="pill running">plan</span>
          <span className="grow" />
          <span className={`approval-wait${lapsed ? " refused" : ""}`}>
            {lapsed ? "refused after 10 minutes" : `refuses in ${countdown}`}
          </span>
        </header>
        <p className="approval-command mono" data-order="command">
          {pending.title}
        </p>
        <p className="approval-meta mono">
          each child works in its own worktree; its changes land only after a review
        </p>
        <div data-order="blast">
          <ul className="approval-rows plan-rows">
            {planned.map((agent, index) => (
              <li key={`${agent.role ?? "agent"}-${index}`}>
                <span className="pill running">{agent.role ?? "agent"}</span>
                <span className="mono plan-files">{(agent.files ?? []).join(", ")}</span>
                {agent.task ? <p className="fine">{agent.task}</p> : null}
                <pre className="plan-body">{agent.plan ?? ""}</pre>
              </li>
            ))}
          </ul>
        </div>
        {skippingPlan ? (
          <div className="plan-skip">
            <p className="fine">
              Skipping records a waiver instead of an approval. What is the reason?
            </p>
            <div className="cap-form">
              <input
                value={skipReason}
                onChange={(event) => setSkipReason(event.target.value)}
                placeholder="why this fan-out does not need a plan check"
                aria-label="Reason for skipping the plan check"
              />
              <button
                className="chip warn"
                onClick={() => onAnswer(pending, "deny", { skip: true, note: skipReason })}
              >
                Record skip
              </button>
            </div>
          </div>
        ) : null}
        <div className="approval-actions" role="group" aria-label="Grant scope" data-order="scope">
          {lapsed ? (
            <button className="chip" onClick={() => onDismiss?.(pending)}>
              Dismiss
            </button>
          ) : (
            <>
              <button
                className="primary"
                data-primary="true"
                onClick={() => onAnswer(pending, "once")}
              >
                Approve plan
              </button>
              <button className="chip warn" onClick={() => setSkippingPlan(true)}>
                Skip check, with a reason
              </button>
              <button
                className="chip danger"
                data-testid="approval-deny"
                aria-keyshortcuts="Escape"
                onClick={deny}
              >
                Reject
              </button>
            </>
          )}
        </div>
        <p className="approval-snapshot fine" data-order="snapshot">
          snapshot state: {snapshotState}
        </p>
      </section>
    );
  }

  return (
    <section
      className="approval-card"
      aria-label="Approval required"
      onKeyDown={(event) => {
        if (event.key === "Escape" && !lapsed) {
          event.preventDefault();
          deny();
        }
      }}
    >
      <header>
        <span className={`pill ${destructive.length > 0 ? "failed" : "running"}`}>
          {pending.kind}
        </span>
        <span className="grow" />
        <span className={`approval-wait${lapsed ? " refused" : ""}`}>
          {lapsed ? "refused after 10 minutes" : `refuses in ${countdown}`}
        </span>
      </header>

      {/* 1. the command, before anything about it */}
      <p className="approval-command mono" data-order="command">
        {pending.title}
      </p>
      <p className="approval-meta mono">
        {cwd ? `cwd ${cwd}` : null}
        {timeout ? ` · ${timeout}s` : null}
      </p>

      {/* 2. what it would touch */}
      <div data-order="blast">
        {destructive.length > 0 ? (
          <ul className="approval-rows">
            {destructive.map((row, index) => (
              <li key={`${row.id ?? "row"}-${index}`}>
                <span className="pill danger">{row.id ?? "policy"}</span> {row.summary ?? ""}
              </li>
            ))}
          </ul>
        ) : null}
        {blast ? (
          <div className="approval-blast" data-testid="approval-blast">
            <p className="fine">
              blast radius: {(blast.targets ?? []).length} resolved target(s)
              {blast.commands?.length ? ` · ${blast.commands.join(" · ")}` : null}
            </p>
            {(blast.targets ?? []).length > 0 ? (
              <ul className="approval-rows">
                {(blast.targets ?? []).map((target, index) => (
                  <li key={`${target.path ?? "target"}-${index}`}>
                    <span className="pill danger">{target.kind ?? "unknown"}</span>
                    <span className="mono">{target.path ?? "unknown"}</span>
                    {typeof target.file_count === "number" ? (
                      <span className="fine"> · {target.file_count} file(s)</span>
                    ) : null}
                    {typeof target.byte_count === "number" ? (
                      <span className="fine"> · {target.byte_count} byte(s)</span>
                    ) : null}
                    {target.note ? <span className="fine"> · {target.note}</span> : null}
                  </li>
                ))}
              </ul>
            ) : null}
            {/* Never collapsed: an unknown blast radius is the point of asking. */}
            {(blast.cannot_determine ?? []).length > 0 ? (
              <ul className="approval-rows approval-unknown" aria-label="Cannot determine">
                {(blast.cannot_determine ?? []).map((row, index) => (
                  <li key={`unknown-${index}`} data-testid="approval-cannot-determine">
                    <span className="pill danger">cannot determine</span> {row}
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
        ) : null}
        {dryRun ? (
          <div className="approval-blast" data-testid="approval-dry-run">
            <p className="fine">
              dry run: <span className="mono">{dryRun.command ?? "preview"}</span>
            </p>
            {dryRun.supported ? (
              <pre className="mono approval-dry-run-output">
                {dryRun.output?.trim() ? dryRun.output : "(no output)"}
              </pre>
            ) : (
              <p className="fine">{dryRun.note ?? "this tool has no preview"}</p>
            )}
          </div>
        ) : null}
        {skillScripts.length > 0 ? (
          <div className="approval-blast">
            <p className="fine">skill scripts: a loaded skill contributed these paths</p>
            <ul className="approval-rows">
              {skillScripts.map((script, index) => (
                <li key={`${script.path ?? "script"}-${index}`}>
                  <span className="pill danger">skill</span>
                  <span className="mono">{script.path ?? "unknown"}</span>
                  {script.skill_id ? <span className="fine"> · {script.skill_id}</span> : null}
                </li>
              ))}
            </ul>
          </div>
        ) : null}
        {mcpTools.length > 0 ? (
          <div className="approval-blast">
            <p className="fine">
              MCP tools from this server ({mcpTools.length}), read from its own tools/list
            </p>
            <ul className="approval-rows">
              {mcpTools.map((tool, index) => (
                <li key={`${tool.name ?? "tool"}-${index}`}>
                  <span className={`pill ${tool.name === detail.tool ? "danger" : "running"}`}>
                    {tool.name ?? "unnamed"}
                  </span>
                  {tool.description ? <span className="fine"> · {tool.description}</span> : null}
                </li>
              ))}
            </ul>
            <p className="approval-note">
              Starting the server does not approve any single tool: each tool asks on its own
              first use.
            </p>
          </div>
        ) : null}
        {deliberation?.note ? <p className="approval-note">{deliberation.note}</p> : null}
      </div>

      {/* 3. how long the grant lasts; exactly one primary action */}
      <div className="approval-actions" role="group" aria-label="Grant scope" data-order="scope">
        {lapsed ? (
          <>
            {onRetry ? (
              <button
                className="chip warn"
                disabled={retryDisabled}
                title="Start a new turn asking the same agent to request this again"
                onClick={() => onRetry(pending)}
              >
                Retry
              </button>
            ) : null}
            <button className="chip" onClick={() => onDismiss?.(pending)}>
              Dismiss
            </button>
          </>
        ) : (
          <>
            <button className="primary" data-primary="true" onClick={() => onAnswer(pending, "once")}>
              Allow once
            </button>
            <button className="chip" onClick={() => onAnswer(pending, "session")}>
              This session
            </button>
            <button className="chip" onClick={() => onAnswer(pending, "project")}>
              This project
            </button>
            <button className="chip" onClick={() => onAnswer(pending, "always")}>
              Always
            </button>
            <button
              className="chip danger"
              data-testid="approval-deny"
              aria-keyshortcuts="Escape"
              onClick={deny}
            >
              Reject
            </button>
          </>
        )}
      </div>

      {/* 4. snapshot state, last */}
      <p className="approval-snapshot fine" data-order="snapshot">
        snapshot state: {snapshotState}
      </p>
    </section>
  );
}

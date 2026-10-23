/**
 * The merged diff (P5.6): what changed, who changed it, and where two agents disagreed.
 *
 * The strip at the top is the conflict surface. It names the claimant, the writer and both
 * hashes, because a claim is advice (N1, D5). Resolving a row records that a human looked at
 * it; the row stays in the ledger.
 *
 * The browser preview uses sample rows so the states are inspectable without a project.
 */

import { useEffect, useMemo, useState } from "react";
import {
  api,
  isTauri,
  type AttributedHunk,
  type ClaimConflict,
  type CoordinationView,
  type Patch,
} from "./api";

/** Split a stored diff into its blocks, keeping each `@@` header with its body. */
function diffBlocks(diff: string): { header: string; body: string }[] {
  const lines = diff.split("\n");
  const blocks: { header: string; body: string[] }[] = [];
  for (const line of lines) {
    if (line.startsWith("@@")) {
      blocks.push({ header: line, body: [] });
    } else if (blocks.length > 0) {
      blocks[blocks.length - 1].body.push(line);
    }
  }
  return blocks.map((block) => ({ header: block.header, body: block.body.join("\n") }));
}

/**
 * One patch, with a gutter per block. Before P5.6 the whole file carried one agent; now each
 * `@@` block names the agent whose patch produced it, which is the whole reason the merged view
 * exists when several agents touch one file.
 */
function PatchBody({
  patch,
  hunks,
  nameOf,
}: {
  patch: Patch;
  hunks: AttributedHunk[];
  nameOf: (agentId: string) => string;
}) {
  const blocks = diffBlocks(patch.diff);
  if (blocks.length === 0) {
    return <pre className="patch-diff">{patch.diff}</pre>;
  }
  return (
    <div className="hunk-list">
      {blocks.map((block, index) => {
        const hunk = hunks.find((row) => row.ordinal === index && row.path === patch.path);
        const agent = hunk ? nameOf(hunk.agent_id) : nameOf(patch.agent_id);
        return (
          <div className="hunk" key={`${patch.id}-${index}`}>
            <div className="hunk-head">
              <span className="hunk-agent" title={hunk ? hunk.agent_id : patch.agent_id}>
                {agent}
              </span>
              <span className="mono hunk-range">{block.header}</span>
              {hunk && hunk.status !== "applied" ? (
                <span className={`pill ${hunk.status}`}>{hunk.status}</span>
              ) : null}
            </div>
            <pre className="patch-diff">{block.body}</pre>
          </div>
        );
      })}
    </div>
  );
}

const PREVIEW: CoordinationView = {
  claims: [
    {
      id: "cl-preview-1",
      path: "ui/src/session.tsx",
      agent_id: "agent-worker",
      agent: "worker",
      harness: "native",
      base_hash: "9f2c4a…",
      acquired_at: Date.now() - 90_000,
      heartbeat_at: Date.now() - 2_000,
      released_at: null,
      release_reason: null,
      stale: false,
      held: true,
    },
    {
      id: "cl-preview-2",
      path: "ui/src/shell.css",
      agent_id: "agent-reviewer",
      agent: "reviewer",
      harness: "claude",
      base_hash: "41ab7d…",
      acquired_at: Date.now() - 600_000,
      heartbeat_at: Date.now() - 300_000,
      released_at: null,
      release_reason: null,
      stale: true,
      held: true,
    },
  ],
  unverified_changes: [
    {
      path: "ui/src/session.tsx",
      agent: "reviewer",
      claimant: "agent-worker",
      base_hash: "9f2c4a…",
      observed_hash: "7d1e02…",
      ambiguous: false,
    },
  ],
  conflicts: [
    {
      id: "cf-preview-1",
      path: "ui/src/session.tsx",
      claimant: "agent-worker",
      writer: "reviewer",
      base_hash: "9f2c4a…",
      observed_hash: "7d1e02…",
      detected_at: Date.now() - 30_000,
      resolved_at: null,
      resolution: null,
    },
  ],
};

const PREVIEW_PATCHES: Patch[] = [
  {
    id: "patch-preview-1",
    session_id: "preview",
    agent_id: "agent-worker",
    path: "ui/src/session.tsx",
    base_hash: "9f2c4a…",
    result_hash: "7d1e02…",
    diff: "--- a/ui/src/session.tsx\n+++ b/ui/src/session.tsx\n@@ -1,2 +1,2 @@\n-const pane = 1;\n+const panes = 2;\n",
    status: "applied",
    created_at: Date.now() - 120_000,
    applied_at: Date.now() - 120_000,
    reverted_at: null,
  },
];

function ago(at: number): string {
  const seconds = Math.max(0, Math.round((Date.now() - at) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  return `${Math.round(minutes / 60)}h ago`;
}

export default function DiffView({ project, session }: { project: string; session: string | null }) {
  const tauri = isTauri();
  const [view, setView] = useState<CoordinationView>(() => (tauri ? { claims: [], unverified_changes: [], conflicts: [] } : PREVIEW));
  const [prBusy, setPrBusy] = useState(false);
  const [prResult, setPrResult] = useState<string | null>(null);
  const [patches, setPatches] = useState<Patch[]>(() => (tauri ? [] : PREVIEW_PATCHES));
  const [hunks, setHunks] = useState<AttributedHunk[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      if (!tauri) return;
      try {
        const [next, nextPatches, nextHunks] = await Promise.all([
          api.claimList(project),
          session ? api.listPatches(project, session) : Promise.resolve([]),
          session ? api.patchHunks(project, session) : Promise.resolve([]),
        ]);
        if (alive) {
          setView(next);
          setPatches(nextPatches);
          setHunks(nextHunks);
        }
      } catch (problem) {
        if (alive) setError(problem instanceof Error ? problem.message : String(problem));
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 2000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [project, session, tauri]);

  const names = useMemo(() => {
    const map = new Map<string, string>();
    for (const claim of view.claims) map.set(claim.agent_id, claim.agent);
    return map;
  }, [view.claims]);

  const name = (id: string | null) => (id ? names.get(id) ?? id.slice(0, 8) : "unattributed");
  const hunksByPath = useMemo(() => {
    const map = new Map<string, AttributedHunk[]>();
    for (const hunk of hunks) {
      const rows = map.get(hunk.path) ?? [];
      rows.push(hunk);
      map.set(hunk.path, rows);
    }
    return map;
  }, [hunks]);

  const resolve = async (conflict: ClaimConflict, resolution: string) => {
    setBusy(conflict.id);
    try {
      if (tauri) await api.resolveConflict(project, conflict.id, resolution);
      setView((current) => ({
        ...current,
        conflicts: current.conflicts.filter((row) => row.id !== conflict.id),
      }));
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(null);
    }
  };

  const revert = async (patch: Patch) => {
    setBusy(patch.id);
    try {
      const reverted = await api.revertPatch(patch.id);
      setPatches((current) => current.map((row) => (row.id === patch.id ? reverted : row)));
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(null);
    }
  };

  const total = patches.reduce((sum, patch) => sum + (patch.status === "applied" ? 1 : 0), 0);

  return (
    <main className="detail diff-view">
      <header className="detail-head">
        <div>
          <p className="eyebrow">{project.split(/[\\/]/).pop() || project} · merged diff</p>
          <h2>One tree, several agents</h2>
          <p className="lede">
            {view.claims.length} claimed · {view.conflicts.length} conflict
            {view.conflicts.length === 1 ? "" : "s"} · {total} applied patch{total === 1 ? "" : "es"}
          </p>
        </div>
        <div className="session-tools">
          {session ? (
            <button
              className="chip"
              disabled={prBusy}
              onClick={() => {
                setPrBusy(true);
                setPrResult(null);
                void (async () => {
                  try {
                    const plan = await api.pullRequestPlan(project, session);
                    if (
                      !window.confirm(
                        `${plan.title}

${plan.body}

Open this pull request from ${plan.branch} into ${plan.base} with gh?`,
                      )
                    ) {
                      return;
                    }
                    setPrResult(await api.createPullRequest(project, session));
                  } catch (failure) {
                    setError(failure instanceof Error ? failure.message : "Pull request failed.");
                  } finally {
                    setPrBusy(false);
                  }
                })();
              }}
            >
              {prBusy ? "Opening…" : "Open pull request"}
            </button>
          ) : null}
        </div>
      </header>

      {prResult ? (
        <p className="notice" role="status">
          Pull request: {prResult}
        </p>
      ) : null}

      <p className="notice" role="status">
        {error
          ? error
          : "Claims are advisory: another agent may still write a claimed file. Rigel records the collision and shows both sides; no write is ever refused."}
      </p>

      {view.conflicts.length > 0 ? (
        <section className="conflict-strip" aria-label="Conflicts">
          <h3>Conflicts</h3>
          {view.conflicts.map((conflict) => (
            <article className="conflict-row" key={conflict.id}>
              <div className="conflict-head">
                <span className="pill failed">conflict</span>
                <span className="mono conflict-path">{conflict.path}</span>
                <span className="grow" />
                <span className="conflict-when">{ago(conflict.detected_at)}</span>
              </div>
              <p className="conflict-who">
                claimed by <strong>{name(conflict.claimant)}</strong> · written by{" "}
                <strong>{name(conflict.writer)}</strong>
                {conflict.writer ? null : " (more than one agent was running)"}
              </p>
              <p className="conflict-hashes mono">
                base {conflict.base_hash ?? "—"} → observed {conflict.observed_hash ?? "deleted"}
              </p>
              <div className="conflict-actions">
                <button
                  className="chip"
                  disabled={busy === conflict.id}
                  onClick={() => void resolve(conflict, "kept the writer's version")}
                >
                  Keep the writer's
                </button>
                <button
                  className="chip"
                  disabled={busy === conflict.id}
                  onClick={() => void resolve(conflict, "restored the base")}
                >
                  Mark for restore
                </button>
              </div>
            </article>
          ))}
        </section>
      ) : (
        <p className="empty">No conflicts. That is the normal state, not a guarantee.</p>
      )}

      <section className="claims-block">
        <h3>Claims</h3>
        {view.claims.length === 0 ? (
          <p className="empty">No agent has claimed a file yet.</p>
        ) : (
          <ul className="claim-list">
            {view.claims.map((claim) => (
              <li key={claim.id} className={claim.stale ? "claim-row stale" : "claim-row"}>
                <span className="mono claim-path">{claim.path}</span>
                <span className="claim-owner">
                  {claim.agent} <span className="faint">({claim.harness})</span>
                </span>
                <span className={`pill ${claim.stale ? "failed" : "running"}`}>
                  {claim.stale ? "stale" : "claimed"}
                </span>
                <span className="faint mono">{ago(claim.heartbeat_at)}</span>
              </li>
            ))}
          </ul>
        )}
        {view.unverified_changes.length > 0 ? (
          <p className="unverified">
            {view.unverified_changes.length} claimed file
            {view.unverified_changes.length === 1 ? " has" : "s have"} changed since the claim
            {view.unverified_changes.some((change) => change.ambiguous)
              ? ", and more than one agent was running for at least one of them"
              : ""}
            .
          </p>
        ) : null}
      </section>

      <section className="patches-block">
        <h3>Patches</h3>
        {patches.length === 0 ? (
          <p className="empty">No patch has been applied in this session.</p>
        ) : (
          patches.map((patch) => (
            <article className="patch" key={patch.id}>
              <header>
                <span className={`pill ${patch.status}`}>{patch.status}</span>
                <span className="mono patch-path">{patch.path}</span>
                <span className="grow" />
                <span className="faint mono">{ago(patch.created_at)}</span>
                <button
                  className="chip"
                  disabled={patch.status !== "applied" || busy === patch.id}
                  onClick={() => void revert(patch)}
                >
                  revert
                </button>
              </header>
              <PatchBody
                patch={patch}
                hunks={hunksByPath.get(patch.path) ?? []}
                nameOf={(id) => name(id)}
              />
            </article>
          ))
        )}
      </section>
    </main>
  );
}

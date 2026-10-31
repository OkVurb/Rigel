/**
 * The merged diff (P5.6): what changed, who changed it, and where two agents disagreed.
 *
 * The strip at the top is the conflict surface. It names the claimant, the writer and both
 * hashes, because a claim is advice (N1, D5). A row can be resolved without a write, or expanded
 * into the three versions Rigel captured — base, ours, theirs — merged line by line and written
 * back through the CAS pipeline (D79). Either way the row stays in the ledger.
 *
 * The browser preview uses sample rows so the states are inspectable without a project.
 */

import { useEffect, useMemo, useState } from "react";
import {
  api,
  isTauri,
  type AppliedMerge,
  type AttributedHunk,
  type Bookmark,
  type ClaimConflict,
  type ConflictSides,
  type CoordinationView,
  type HunkComment,
  type MergedConflict,
  type Patch,
  type Provenance,
  type WritableAgentView,
  type WritableDiffView,
} from "./api";

/** One rendered line of a hunk, with the number the reader would find in the new file. */
interface DiffLine {
  kind: "add" | "del" | "ctx" | "meta";
  number: number | null;
  text: string;
}

/**
 * D156: the line rows are what a provenance click starts from. Numbers are computed from the
 * `@@` header, so a row can say exactly which line in the new file it is; the header itself is
 * the hunk boundary the core recorded. A body without a parseable header still renders, with
 * no line numbers — the panel then says "unknown" rather than inventing one.
 */
function diffLineRows(header: string, body: string): DiffLine[] {
  const match = /@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(header);
  let oldLine = match ? Number(match[1]) : 0;
  let newLine = match ? Number(match[2]) : 0;
  const rows: DiffLine[] = [];
  for (const text of body.split("\n")) {
    if (text.startsWith("\\")) {
      rows.push({ kind: "meta", number: null, text });
    } else if (text.startsWith("+")) {
      rows.push({ kind: "add", number: match ? newLine : null, text });
      newLine += 1;
    } else if (text.startsWith("-")) {
      rows.push({ kind: "del", number: match ? oldLine : null, text });
      oldLine += 1;
    } else {
      rows.push({ kind: "ctx", number: match ? newLine : null, text });
      oldLine += 1;
      newLine += 1;
    }
  }
  return rows;
}

/** D156: one loaded chain, addressed to the hunk it was asked from. */
interface ChainView {
  patchId: string;
  hunkId: string | null;
  line: number | null;
  data: Provenance | null;
  error: string | null;
  busy: boolean;
}

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
  comments,
  nameOf,
  busy,
  onComment,
  onDeliver,
  bookmarks,
  onBookmark,
  chain,
  onProvenance,
  onCloseChain,
}: {
  patch: Patch;
  hunks: AttributedHunk[];
  comments: HunkComment[];
  nameOf: (agentId: string) => string;
  busy: string | null;
  onComment: (hunkId: string, body: string) => Promise<void>;
  onDeliver: (comment: HunkComment) => Promise<void>;
  bookmarks: Bookmark[];
  onBookmark: (hunkId: string, label: string) => void;
  chain: ChainView | null;
  onProvenance: (hunkId: string | null, line: number | null) => void;
  onCloseChain: () => void;
}) {
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const blocks = diffBlocks(patch.diff);
  if (blocks.length === 0) {
    return <pre className="patch-diff">{patch.diff}</pre>;
  }
  return (
    <div className="hunk-list">
      {blocks.map((block, index) => {
        const hunk = hunks.find((row) => row.ordinal === index && row.path === patch.path);
        const agent = hunk ? nameOf(hunk.agent_id) : nameOf(patch.agent_id);
        const thread = hunk ? comments.filter((row) => row.hunk_id === hunk.id) : [];
        const draft = hunk ? drafts[hunk.id] ?? "" : "";
        // A merge hunk is the human's own text (`user`); there is nobody to send a comment to.
        const own = (hunk?.agent_id ?? patch.agent_id) === "user";
        const marked =
          hunk !== undefined &&
          bookmarks.some((row) => row.kind === "hunk" && row.target_id === hunk.id);
        return (
          <div
            className="hunk"
            key={`${patch.id}-${index}`}
            id={hunk ? `hunk-${hunk.id}` : undefined}
          >
            <div className="hunk-head">
              <span className="hunk-agent" title={hunk ? hunk.agent_id : patch.agent_id}>
                {agent}
              </span>
              <span className="mono hunk-range">{block.header}</span>
              {hunk && hunk.status !== "applied" ? (
                <span className={`pill ${hunk.status}`}>{hunk.status}</span>
              ) : null}
              <button
                type="button"
                className="chip chain-open"
                title="Show where these lines came from — hunk, patch, turn, tool, model"
                onClick={() => onProvenance(hunk?.id ?? null, null)}
              >
                where from
              </button>
              {hunk ? (
                <button
                  type="button"
                  className={`bookmark-star in-head${marked ? " on" : ""}`}
                  aria-pressed={marked}
                  aria-label={marked ? "Remove hunk bookmark" : "Bookmark this hunk"}
                  title={marked ? "Remove bookmark" : "Keep this hunk in the session list"}
                  onClick={() =>
                    onBookmark(hunk.id, `${patch.path} · ${block.header}`.slice(0, 160))
                  }
                >
                  {marked ? "★" : "☆"}
                </button>
              ) : null}
            </div>
            <div className="patch-diff">
              {diffLineRows(block.header, block.body).map((row, lineIndex) => (
                <div
                  key={`${patch.id}-${index}-${lineIndex}`}
                  className={`diff-line ${row.kind}`}
                  title="Show where this line came from"
                  onClick={() => onProvenance(hunk?.id ?? null, row.number)}
                >
                  <span className="diff-no">{row.number ?? ""}</span>
                  <span className="diff-text">{row.text || " "}</span>
                </div>
              ))}
            </div>
            {chain && chain.hunkId === (hunk?.id ?? null) ? (
              <ProvenancePanel
                chain={chain}
                hunk={hunk ?? null}
                patch={patch}
                onClose={onCloseChain}
              />
            ) : null}
            {hunk && !own ? (
              <div className="hunk-comments">
                {thread.map((row) => (
                  <div className="hunk-comment" key={row.id}>
                    <span className="hunk-comment-author">
                      {row.author === "user" ? "you" : nameOf(row.author)}
                    </span>
                    <p>{row.body}</p>
                    {row.delivered_at ? (
                      <span className="pill sent" title={row.delivered_turn_id ?? undefined}>
                        sent
                      </span>
                    ) : (
                      <button
                        className="chip"
                        disabled={busy === row.id}
                        title={`Send this comment to ${agent} as a turn`}
                        onClick={() => void onDeliver(row)}
                      >
                        {busy === row.id ? "sending…" : `send to ${agent}`}
                      </button>
                    )}
                  </div>
                ))}
                <div className="hunk-comment-new">
                  <textarea
                    value={draft}
                    rows={2}
                    placeholder={`Comment on ${agent}'s lines — it goes back to ${agent} as a turn`}
                    onChange={(event) =>
                      setDrafts((current) => ({ ...current, [hunk.id]: event.target.value }))
                    }
                  />
                  <button
                    className="chip"
                    disabled={!draft.trim()}
                    onClick={() =>
                      void onComment(hunk.id, draft).then(() =>
                        setDrafts((current) => ({ ...current, [hunk.id]: "" })),
                      )
                    }
                  >
                    comment
                  </button>
                </div>
              </div>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}

/** D156: the chain, printed link by link. A missing link says "unknown". */
function ProvenancePanel({
  chain,
  hunk,
  onClose,
}: {
  chain: ChainView;
  hunk: AttributedHunk | null;
  patch: Patch;
  onClose: () => void;
}) {
  const data = chain.data;
  const range = hunk
    ? `old ${hunk.old_start}–${hunk.old_start + hunk.old_lines - 1} → new ${hunk.new_start}–${
        hunk.new_start + hunk.new_lines - 1
      }`
    : null;
  return (
    <div className="provenance" role="status">
      <div className="provenance-head">
        <strong>Where this came from</strong>
        <span className="grow" />
        <button type="button" className="link" onClick={onClose}>
          close
        </button>
      </div>
      {chain.busy ? (
        <p className="fine">Reading the journal…</p>
      ) : chain.error ? (
        <p className="fine">{chain.error}</p>
      ) : data ? (
        <dl>
          <dt>line</dt>
          <dd>
            {chain.line !== null ? `new line ${chain.line} · ` : ""}
            {range ? `${hunk?.header} · ${range}` : "unknown — no patch_hunks row for this block"}
          </dd>
          <dt>patch</dt>
          <dd className="mono">
            {data.patch_id.slice(0, 8)} · {data.path} · {data.status}
          </dd>
          <dt>turn</dt>
          <dd>
            {data.turn_id !== null
              ? `turn ${data.turn_seq} · ${data.turn_status}`
              : "unknown — no recorded item names this patch"}
          </dd>
          <dt>tool</dt>
          <dd>
            {data.tool ?? "unknown"}
            {data.item_kind ? ` · ${data.item_kind}` : ""}
          </dd>
          <dt>agent</dt>
          <dd>
            {data.agent_role ?? data.agent_id.slice(0, 8)}
            {data.harness ? ` · ${data.harness}` : ""} · {" "}
            {data.model ?? "model unknown"}
          </dd>
        </dl>
      ) : null}
      <p className="fine">
        Read-only. Every link is a recorded row; a missing one is “unknown”, never inferred, and
        nothing is re-run.
      </p>
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

const PREVIEW_SIDES: ConflictSides = {
  id: "cf-preview-1",
  path: "ui/src/session.tsx",
  base: { hash: "9f2c4a71", text: "export function pane() {\n  return 1;\n}\n" },
  ours: { hash: "7d1e02a4", text: "export function pane() {\n  return 2;\n}\n" },
  theirs: { hash: "41ab7dc9", text: "export function pane() {\n  return 3;\n}\n" },
  note: "the file changed again after the conflict was recorded.",
};

const PREVIEW_MERGE: MergedConflict = {
  id: "cf-preview-1",
  path: "ui/src/session.tsx",
  merged: "export function pane() {\n<<<<<<< ours\n  return 2;\n=======\n  return 3;\n>>>>>>> theirs\n}\n",
  conflicts: 1,
  clean: false,
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

// D150: a hunk row gives the preview a star to exercise; without it the sample patch has
// rendered blocks but no `patch_hunks` identity to bookmark.
const PREVIEW_HUNKS: AttributedHunk[] = [
  {
    id: "hunk-preview-1",
    patch_id: "patch-preview-1",
    path: "ui/src/session.tsx",
    ordinal: 0,
    agent_id: "agent-worker",
    header: "@@ -1,2 +1,2 @@",
    old_start: 1,
    old_lines: 2,
    new_start: 1,
    new_lines: 2,
    created_at: Date.now() - 120_000,
    status: "applied",
  },
];

const PREVIEW_OPEN_REVIEW: WritableAgentView = {
  agent_id: "c4d18e77-2b90-4a51-8f0e-6d3a9b5c7e21",
  role: "writer",
  model: "openai/gpt-5-mini",
  status: "exited",
  worktree: "C:/Users/you/AppData/Local/rigel/worktrees/rigel-c4d18e77",
  plan: "Replace the retry loop with a single attempt and let the caller retry.",
  plan_note: null,
  review: {
    id: "review-preview-3",
    session_id: "preview",
    agent_id: "c4d18e77-2b90-4a51-8f0e-6d3a9b5c7e21",
    reviewer_model: "openai/gpt-5-nano",
    verdict: "blocked",
    reason:
      "the retry loop is the only thing bounding a failing endpoint; removing it is a regression",
    diff_sha: "5b0e7c21d8a4",
    files: ["crates/rigel-core/src/retry.rs"],
    override_note: null,
    merged: false,
    created_at: Date.now() - 20_000,
  },
};

const PREVIEW_WRITABLE: WritableAgentView[] = [
  {
    agent_id: "a1f4c9d2-0c11-4f7e-9d33-5c2b8e6a1f04",
    role: "writer",
    model: "openai/gpt-5-mini",
    status: "exited",
    worktree: null,
    plan: "Add the retry helper and call it from the poller.",
    plan_note: null,
    review: {
      id: "review-preview-1",
      session_id: "preview",
      agent_id: "a1f4c9d2-0c11-4f7e-9d33-5c2b8e6a1f04",
      reviewer_model: "openai/gpt-5-nano",
      verdict: "approved",
      reason: "one file, a helper and its call site; nothing else moved",
      diff_sha: "c1d2e3f4a5b6",
      files: ["crates/rigel-core/src/retry.rs"],
      override_note: null,
      merged: true,
      created_at: Date.now() - 240_000,
    },
  },
  {
    agent_id: "b7e30211-8a4d-4c62-ae19-77d0f3b6c2a8",
    role: "writer",
    model: "openai/gpt-5-mini",
    status: "exited",
    worktree: null,
    plan: "Thread the abort signal through the retry helper.",
    plan_note: null,
    review: {
      id: "review-preview-2",
      session_id: "preview",
      agent_id: "b7e30211-8a4d-4c62-ae19-77d0f3b6c2a8",
      reviewer_model: "openai/gpt-5-nano",
      verdict: "blocked",
      reason: "the patch removes the only check that the token is still valid",
      diff_sha: "7d1e02a4b9c3",
      files: ["crates/rigel-core/src/retry.rs"],
      override_note: "the check moved into the caller; I read both files",
      merged: true,
      created_at: Date.now() - 60_000,
    },
  },
  PREVIEW_OPEN_REVIEW,
];

const PREVIEW_WORKTREE: WritableDiffView = {
  agent_id: "c4d18e77-2b90-4a51-8f0e-6d3a9b5c7e21",
  files: ["crates/rigel-core/src/retry.rs"],
  diff: "--- a/crates/rigel-core/src/retry.rs\n+++ b/crates/rigel-core/src/retry.rs\n@@ -1,6 +1,3 @@\n pub fn retry(n: u32) {\n-    for attempt in 0..n {\n-        call(attempt)?;\n-    }\n+    call(0)?;\n     Ok(())\n }\n",
};

function ago(at: number): string {
  const seconds = Math.max(0, Math.round((Date.now() - at) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  return `${Math.round(minutes / 60)}h ago`;
}

export default function DiffView({
  project,
  session,
  focus,
  onFocusHandled,
}: {
  project: string;
  session: string | null;
  /** D147: the patch the palette asked for, if any. */
  focus?: { sessionId: string; patchId?: string; hunkId?: string } | null;
  onFocusHandled?: () => void;
}) {
  const tauri = isTauri();
  const [view, setView] = useState<CoordinationView>(() => (tauri ? { claims: [], unverified_changes: [], conflicts: [] } : PREVIEW));
  const [prBusy, setPrBusy] = useState(false);
  const [prResult, setPrResult] = useState<string | null>(null);
  const [patches, setPatches] = useState<Patch[]>(() => (tauri ? [] : PREVIEW_PATCHES));
  const [hunks, setHunks] = useState<AttributedHunk[]>(() =>
    tauri ? [] : PREVIEW_HUNKS,
  );
  const [comments, setComments] = useState<HunkComment[]>([]);
  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);
  // D156: one chain panel at a time, keyed to the patch and hunk it was asked from.
  const [chain, setChain] = useState<ChainView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [opened, setOpened] = useState<string | null>(null);
  const [sides, setSides] = useState<ConflictSides | null>(null);
  const [merged, setMerged] = useState<MergedConflict | null>(null);
  const [mergedText, setMergedText] = useState("");
  const [writable, setWritable] = useState<WritableAgentView[]>(() =>
    tauri ? [] : PREVIEW_WRITABLE,
  );
  const [worktree, setWorktree] = useState<Record<string, WritableDiffView>>({});
  const [openWorktree, setOpenWorktree] = useState<string | null>(null);
  const [reasons, setReasons] = useState<Record<string, string>>({});
  const [merging, setMerging] = useState<string | null>(null);
  const [applied, setApplied] = useState<Record<string, AppliedMerge>>({});
  // The first read gates a palette jump: scrolling before the patch list exists would miss.
  const [loaded, setLoaded] = useState(!tauri);

  useEffect(() => {
    if (tauri) setLoaded(false);
  }, [project, session, tauri]);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      if (!tauri) return;
      try {
        const [next, nextPatches, nextHunks, nextComments, nextWritable, nextBookmarks] =
          await Promise.all([
            api.claimList(project),
            session ? api.listPatches(project, session) : Promise.resolve([]),
            session ? api.patchHunks(project, session) : Promise.resolve([]),
            session ? api.hunkComments(project, session) : Promise.resolve([]),
            session ? api.writableList(project, session) : Promise.resolve([]),
            session ? api.listBookmarks(project, session) : Promise.resolve([]),
          ]);
        if (alive) {
          setView(next);
          setPatches(nextPatches);
          setHunks(nextHunks);
          setComments(nextComments);
          setWritable(nextWritable);
          setBookmarks(nextBookmarks);
        }
      } catch (problem) {
        if (alive) setError(problem instanceof Error ? problem.message : String(problem));
      } finally {
        if (alive) setLoaded(true);
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 2000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [project, session, tauri]);

  // D147/D150: land on the patch the palette named, or the hunk a bookmark named. The rows
  // arrive just after the view mounts, so wait a paint and clear the request even when gone.
  useEffect(() => {
    if (!focus || !loaded) return;
    const target = focus.hunkId
      ? `hunk-${focus.hunkId}`
      : focus.patchId
        ? `patch-${focus.patchId}`
        : null;
    if (!target) return;
    const timer = window.setTimeout(() => {
      const row = document.getElementById(target);
      if (row) {
        row.scrollIntoView({ block: "center" });
        row.classList.add("jump");
        window.setTimeout(() => row.classList.remove("jump"), 1800);
      }
      onFocusHandled?.();
    }, 200);
    return () => window.clearTimeout(timer);
  }, [focus, loaded, patches.length, hunks.length, onFocusHandled]);

  const openChain = async (patchId: string, hunkId: string | null, line: number | null) => {
    setChain({ patchId, hunkId, line, data: null, error: null, busy: true });
    try {
      const data = await api.provenance(project, patchId);
      setChain((current) =>
        current && current.patchId === patchId && current.busy
          ? { ...current, data, busy: false }
          : current,
      );
    } catch (problem) {
      const message = problem instanceof Error ? problem.message : String(problem);
      setChain((current) =>
        current && current.patchId === patchId && current.busy
          ? { ...current, error: message, busy: false }
          : current,
      );
    }
  };

  const toggleHunkBookmark = async (hunkId: string, label: string) => {
    if (!session) return;
    const marked = bookmarks.some(
      (row) => row.kind === "hunk" && row.target_id === hunkId,
    );
    try {
      const next = tauri
        ? await api.toggleBookmark(project, session, "hunk", hunkId, label)
        : marked
          ? null
          : {
              id: `preview-${hunkId}`,
              session_id: session,
              kind: "hunk" as const,
              target_id: hunkId,
              label,
              created_at: Date.now(),
            };
      setBookmarks((rows) =>
        next
          ? [
              next,
              ...rows.filter((row) => !(row.kind === "hunk" && row.target_id === hunkId)),
            ]
          : rows.filter((row) => !(row.kind === "hunk" && row.target_id === hunkId)),
      );
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  };

  // Claims name most writers, but a merged subagent has already released its claim: its patch
  // would otherwise show a bare id. The writable rows carry the role it was registered under.
  const names = useMemo(() => {
    const map = new Map<string, string>();
    for (const claim of view.claims) map.set(claim.agent_id, claim.agent);
    for (const row of writable) {
      if (!map.has(row.agent_id)) map.set(row.agent_id, row.role ?? row.agent_id.slice(0, 8));
    }
    return map;
  }, [view.claims, writable]);

  const name = (id: string | null) =>
    id ? (id === "user" ? "you" : names.get(id) ?? id.slice(0, 8)) : "unattributed";
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

  const openSides = async (conflict: ClaimConflict) => {
    if (opened === conflict.id) {
      setOpened(null);
      setSides(null);
      setMerged(null);
      return;
    }
    setOpened(conflict.id);
    setSides(null);
    setMerged(null);
    setError(null);
    if (!tauri) {
      setSides(PREVIEW_SIDES);
      return;
    }
    setBusy(conflict.id);
    try {
      setSides(await api.conflictSides(project, conflict.id));
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(null);
    }
  };

  const mergeSides = async (conflict: ClaimConflict) => {
    setBusy(conflict.id);
    setError(null);
    try {
      const result = tauri ? await api.mergeConflict(project, conflict.id) : PREVIEW_MERGE;
      setMerged(result);
      setMergedText(result.merged);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(null);
    }
  };

  const writeMerge = async (conflict: ClaimConflict) => {
    // The desktop needs an open session to write; the browser preview has none and only demos.
    if (tauri) {
      if (!session) return;
      setBusy(conflict.id);
      setError(null);
    }
    try {
      if (tauri && session) {
        await api.resolveMergedConflict(project, session, conflict.id, mergedText);
      }
      setView((current) => ({
        ...current,
        conflicts: current.conflicts.filter((row) => row.id !== conflict.id),
      }));
      setOpened(null);
      setSides(null);
      setMerged(null);
      setMergedText("");
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

  const comment = async (hunkId: string, body: string) => {
    try {
      const added = await api.addHunkComment(project, hunkId, body);
      setComments((current) => [...current, added]);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  };

  const deliver = async (row: HunkComment) => {
    if (!session) return;
    setBusy(row.id);
    try {
      const reply = await api.deliverHunkComment(project, session, row.id);
      setComments((current) =>
        current.map((comment) =>
          comment.id === row.id
            ? { ...comment, delivered_turn_id: reply.turn.id, delivered_at: Date.now() }
            : comment,
        ),
      );
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(null);
    }
  };

  // Always re-read: a merge may have landed since the row was last opened, and a cached diff
  // would then describe a tree that no longer exists.
  const toggleWorktree = async (agentId: string) => {
    if (openWorktree === agentId) {
      setOpenWorktree(null);
      return;
    }
    setOpenWorktree(agentId);
    setError(null);
    setWorktree((current) => {
      const next = { ...current };
      delete next[agentId];
      return next;
    });
    if (!tauri) {
      setWorktree({ [agentId]: PREVIEW_WORKTREE });
      return;
    }
    setBusy(agentId);
    try {
      const next = await api.writableDiff(project, agentId);
      setWorktree((current) => ({ ...current, [agentId]: next }));
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
      setOpenWorktree(null);
    } finally {
      setBusy(null);
    }
  };

  // The Diff view's decision about a review. A block is not silently crossed: the button stays
  // disabled until a reason is typed, and that reason is what the merge is recorded under.
  const decide = async (row: WritableAgentView, overriding: boolean) => {
    if (!session || !tauri) return;
    const note = (reasons[row.agent_id] ?? "").trim();
    if (overriding && !note) return;
    setMerging(row.agent_id);
    setError(null);
    try {
      const result = await api.writableMerge(
        project,
        session,
        row.agent_id,
        overriding ? note : null,
      );
      setWritable((current) =>
        current.map((item) =>
          item.agent_id === row.agent_id
            ? {
                ...item,
                review: result.review,
                worktree: result.worktree_removed ? null : item.worktree,
              }
            : item,
        ),
      );
      setApplied((current) => ({ ...current, [row.agent_id]: result }));
      setReasons((current) => {
        const next = { ...current };
        delete next[row.agent_id];
        return next;
      });
      if (result.worktree_removed) {
        setOpenWorktree((open) => (open === row.agent_id ? null : open));
        setWorktree((current) => {
          const next = { ...current };
          delete next[row.agent_id];
          return next;
        });
      }
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setMerging(null);
    }
  };

  const total = patches.reduce((sum, patch) => sum + (patch.status === "applied" ? 1 : 0), 0);

  return (
    <main id="main" tabIndex={-1} className="detail diff-view">
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
                <button
                  className="chip"
                  disabled={busy === conflict.id && opened !== conflict.id}
                  onClick={() => void openSides(conflict)}
                >
                  {opened === conflict.id ? "hide the three versions" : "see the three versions"}
                </button>
              </div>
              {opened === conflict.id ? (
                <div className="conflict-merge">
                  {sides?.note ? <p className="conflict-note">{sides.note}</p> : null}
                  {!sides ? (
                    <p className="empty">Reading the three versions…</p>
                  ) : (
                    <div className="conflict-sides">
                      {(
                        [
                          ["base", sides.base],
                          ["ours", sides.ours],
                          ["theirs", sides.theirs],
                        ] as const
                      ).map(([label, side]) => (
                        <div className="conflict-side" key={label}>
                          <header>
                            <strong>{label}</strong>
                            <span className="mono">
                              {side ? `${side.hash.slice(0, 8)}…` : "missing"}
                            </span>
                          </header>
                          {side ? (
                            <pre className="conflict-text">{side.text}</pre>
                          ) : (
                            <p className="empty">Rigel could not rebuild this side.</p>
                          )}
                        </div>
                      ))}
                    </div>
                  )}
                  {sides?.base && sides.ours && sides.theirs ? (
                    <div className="conflict-actions">
                      <button
                        className="chip"
                        disabled={busy === conflict.id}
                        onClick={() => void mergeSides(conflict)}
                      >
                        {busy === conflict.id ? "merging…" : "three-way merge"}
                      </button>
                      <button
                        className="chip"
                        disabled={!merged || busy === conflict.id || (tauri && !session)}
                        title={
                          tauri && !session ? "Open a session to write the merged file" : undefined
                        }
                        onClick={() => void writeMerge(conflict)}
                      >
                        write merged file
                      </button>
                      {merged ? (
                        <span className={`pill ${merged.clean ? "applied" : "failed"}`}>
                          {merged.clean
                            ? "clean merge"
                            : `${merged.conflicts} conflict block${merged.conflicts === 1 ? "" : "s"}`}
                        </span>
                      ) : null}
                    </div>
                  ) : null}
                  {merged ? (
                    <label className="conflict-editor">
                      <span className="faint">
                        Edit the merge, then write it. The write is a patch: it only applies if the
                        file is still what it was when the merge was shown.
                      </span>
                      <textarea
                        className="merged-text"
                        value={mergedText}
                        rows={Math.min(18, Math.max(4, mergedText.split("\n").length))}
                        spellCheck={false}
                        onChange={(event) => setMergedText(event.target.value)}
                      />
                    </label>
                  ) : null}
                </div>
              ) : null}
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

      <section className="writable-block">
        <h3>Subagent worktrees</h3>
        <p className="fine">
          A writable child works in its own checkout. Its changes reach this tree only through a
          merge, and only after a clean-context review; the verdict stays here once it has one.
        </p>
        {writable.length === 0 ? (
          <p className="empty">No subagent has been given write access in this session.</p>
        ) : (
          <ul className="writable-list">
            {writable.map((row) => {
              const review = row.review;
              // A blocked or unavailable verdict that landed anyway: the override is the point.
              const overridden = Boolean(review && review.verdict !== "approved" && review.merged);
              return (
                <li
                  className={overridden ? "writable-row overridden" : "writable-row"}
                  key={row.agent_id}
                >
                  <div className="writable-head">
                    <span className="pill running">{row.role ?? "agent"}</span>
                    <span className="mono writable-id" title={row.agent_id}>
                      {row.agent_id.slice(0, 8)}
                    </span>
                    {row.model ? <span className="faint mono">{row.model}</span> : null}
                    <span className="grow" />
                    <span className="faint">{row.status}</span>
                    {review ? (
                      <span
                        className={`pill ${review.verdict === "approved" ? "applied" : "failed"}`}
                        title={`reviewed by ${review.reviewer_model}`}
                      >
                        {review.verdict}
                      </span>
                    ) : (
                      <span className="pill cancelled" title="no review has been recorded">
                        not reviewed
                      </span>
                    )}
                    {overridden ? (
                      <span className="pill failed" title="merged although the reviewer said no">
                        merged over a block
                      </span>
                    ) : review?.merged ? (
                      <span className="pill sent">merged</span>
                    ) : null}
                  </div>

                  {row.plan ? (
                    <>
                      <p className="fine writable-plan-label">approved plan</p>
                      <pre className="plan-body">{row.plan}</pre>
                    </>
                  ) : row.plan_note ? (
                    <p className="approval-note">plan gate skipped: {row.plan_note}</p>
                  ) : (
                    <p className="fine writable-plan-label">no plan was approved</p>
                  )}

                  {review ? (
                    <div className="review-line">
                      <p className="fine">
                        {review.files.length} file{review.files.length === 1 ? "" : "s"} ·{" "}
                        <span className="mono">{review.reviewer_model}</span> · diff{" "}
                        <span className="mono">{review.diff_sha.slice(0, 8)}</span> ·{" "}
                        {ago(review.created_at)} · {review.merged ? "merged" : "not merged"}
                      </p>
                      {review.files.length > 0 ? (
                        <p className="fine mono writable-files">{review.files.join(", ")}</p>
                      ) : null}
                      {review.reason ? (
                        <p
                          className={
                            review.verdict === "approved"
                              ? "review-reason"
                              : "review-reason blocked"
                          }
                        >
                          {review.reason}
                        </p>
                      ) : null}
                      {review.override_note ? (
                        <p className="review-override">override: {review.override_note}</p>
                      ) : null}
                    </div>
                  ) : null}

                  {review && !review.merged && row.worktree ? (
                    <div className="review-decision">
                      <p className="fine">
                        {review.verdict === "approved"
                          ? "The reviewer approved this diff, but the merge has not landed."
                          : `The reviewer ${
                              review.verdict === "blocked" ? "blocked" : "could not judge"
                            } this diff. A reason is required to cross it, and it is stored beside the verdict, not in place of it.`}
                      </p>
                      {review.verdict !== "approved" ? (
                        <textarea
                          className="override-reason"
                          rows={2}
                          placeholder="Why is this block wrong enough to merge anyway?"
                          value={reasons[row.agent_id] ?? ""}
                          disabled={!tauri || merging === row.agent_id}
                          onChange={(event) =>
                            setReasons((current) => ({
                              ...current,
                              [row.agent_id]: event.target.value,
                            }))
                          }
                        />
                      ) : null}
                      <div className="review-decision-actions">
                        <button
                          className={review.verdict === "approved" ? "chip" : "chip warn"}
                          disabled={
                            !tauri ||
                            merging === row.agent_id ||
                            (review.verdict !== "approved" &&
                              !(reasons[row.agent_id] ?? "").trim())
                          }
                          title={tauri ? undefined : "Merging runs in the desktop app"}
                          onClick={() => void decide(row, review.verdict !== "approved")}
                        >
                          {merging === row.agent_id
                            ? "merging…"
                            : review.verdict === "approved"
                              ? "merge this review"
                              : "override & merge"}
                        </button>
                        {!tauri ? (
                          <span className="fine">sample row; merging runs in the desktop app</span>
                        ) : null}
                      </div>
                    </div>
                  ) : null}

                  {applied[row.agent_id] ? (
                    <p
                      className={
                        applied[row.agent_id].report.conflicted > 0
                          ? "fine merge-report conflicted"
                          : "fine merge-report"
                      }
                      role="status"
                    >
                      {applied[row.agent_id].report.applied} file
                      {applied[row.agent_id].report.applied === 1 ? "" : "s"} applied
                      {applied[row.agent_id].report.conflicted > 0
                        ? ` · ${applied[row.agent_id].report.conflicted} conflicted, nothing written for those`
                        : ""}
                      {applied[row.agent_id].report.skipped > 0
                        ? ` · ${applied[row.agent_id].report.skipped} skipped (deletions and renames are not patches)`
                        : ""}
                      {applied[row.agent_id].review.merged ? "" : " — the review stays open"}
                    </p>
                  ) : null}

                  {row.worktree ? (
                    <div className="writable-actions">
                      <span className="mono faint writable-path" title={row.worktree}>
                        {row.worktree}
                      </span>
                      <span className="grow" />
                      <button
                        className="chip"
                        disabled={busy === row.agent_id}
                        onClick={() => void toggleWorktree(row.agent_id)}
                      >
                        {openWorktree === row.agent_id ? "hide pending diff" : "pending diff"}
                      </button>
                    </div>
                  ) : (
                    <p className="fine">
                      No checkout: the worktree was removed after the merge, or never created.
                    </p>
                  )}

                  {openWorktree === row.agent_id ? (
                    <div className="writable-diff">
                      {worktree[row.agent_id] ? (
                        worktree[row.agent_id].diff ? (
                          <>
                            <p className="fine">
                              {worktree[row.agent_id].files.length} file
                              {worktree[row.agent_id].files.length === 1 ? "" : "s"} not yet in this
                              tree
                            </p>
                            <pre className="patch-diff">{worktree[row.agent_id].diff}</pre>
                          </>
                        ) : (
                          <p className="empty">
                            Nothing pending: the checkout matches the commit it started from.
                          </p>
                        )
                      ) : (
                        <p className="empty">Reading the worktree…</p>
                      )}
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section className="patches-block">
        <h3>Patches</h3>
        {patches.length === 0 ? (
          <p className="empty">No patch has been applied in this session.</p>
        ) : (
          patches.map((patch) => (
            <article className="patch" key={patch.id} id={`patch-${patch.id}`}>
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
                comments={comments.filter((row) => row.patch_id === patch.id)}
                nameOf={(id) => name(id)}
                busy={busy}
                onComment={comment}
                onDeliver={deliver}
                bookmarks={bookmarks}
                onBookmark={(hunkId, label) => void toggleHunkBookmark(hunkId, label)}
                chain={chain?.patchId === patch.id ? chain : null}
                onProvenance={(hunkId, line) => void openChain(patch.id, hunkId, line)}
                onCloseChain={() => setChain(null)}
              />
            </article>
          ))
        )}
      </section>
    </main>
  );
}

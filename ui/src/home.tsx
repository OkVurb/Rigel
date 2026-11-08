/**
 * Home: the Rigel Paper.
 *
 * The window opens on the last day the agents filed work, set as a broadsheet: one story per
 * session that ran that day, a plate for each browser frame they captured, the day's numbers in
 * a market box, and a rack of earlier editions. It reads the same tables Session and Diff read,
 * so nothing here is invented — a day with no work has no edition, and a session with no turns
 * prints as a notice rather than a story.
 *
 * The edition is derived, not stored: `turn.started_at` is grouped by the local calendar day, so
 * no new table or migration is needed for the paper to keep printing.
 */

import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";

import {
  api,
  isTauri,
  type Agent,
  type GravityAd,
  type Item,
  type Patch,
  type Session,
  type TurnTranscript,
} from "./api";
import { previewDossiers } from "./homeMock";
import { hashString, sponsorSession } from "./sponsor";

/** How far back the paper reads, and how many sessions it opens at once. */
const MAX_SESSIONS = 24;
const WINDOW_DAYS = 60;
const CHUNK = 6;
const MAX_DISPATCHES = 9;
const REFRESH_MS = 60_000;

type Tone = "said" | "ran" | "wrote" | "read" | "searched" | "called" | "error" | "note";

interface Dispatch {
  tone: Tone;
  text: string;
  detail: string | null;
  at: number;
}

interface Picture {
  sessionId: string;
  agentId: string;
  path: string | null;
  url: string | null;
  tool: string;
  at: number;
}

interface Story {
  key: string;
  day: string;
  session: Session;
  at: number;
  headline: string;
  asked: string | null;
  lede: string | null;
  dispatches: Dispatch[];
  pictures: Picture[];
  patches: Patch[];
  agents: Agent[];
  turns: TurnTranscript[];
  score: number;
}

interface Crew {
  label: string;
  detail: string;
  turns: number;
}

interface Edition {
  day: string;
  date: Date;
  issue: number;
  stories: Story[];
  lead: Story;
  turns: number;
  patches: number;
  tokens: number;
  micros: number;
  pictures: Picture[];
  crew: Crew[];
}

interface Dossier {
  session: Session;
  turns: TurnTranscript[];
  patches: Patch[];
  agents: Agent[];
}

/* ---------------------------- reading the record ---------------------------- */

const record = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

const text = (value: unknown): string | null =>
  typeof value === "string" && value.trim() ? value.trim() : null;

const clean = (value: string): string => value.replace(/\s+/g, " ").trim();

const clamp = (value: string, limit: number): string =>
  value.length <= limit ? value : `${value.slice(0, limit - 1).trimEnd()}…`;

const count = (value: number, one: string, many = `${one}s`): string =>
  `${value.toLocaleString()} ${value === 1 ? one : many}`;

function httpUrl(value: string | null): string | null {
  if (!value) return null;
  try {
    const parsed = new URL(value);
    return parsed.protocol === "http:" || parsed.protocol === "https:" ? value : null;
  } catch {
    return null;
  }
}

function dayKey(ms: number): string {
  const date = new Date(ms);
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

function dayNumber(date: Date): number {
  const start = new Date(date.getFullYear(), 0, 1).getTime();
  return Math.floor((date.getTime() - start) / 86_400_000) + 1;
}

function longDate(date: Date): string {
  return date.toLocaleDateString(undefined, {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}

function shortDate(date: Date): string {
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

function headlineOf(prompt: string | null, fallback: string | null): string {
  const raw = clean(prompt ?? "");
  if (!raw) return fallback ? clamp(clean(fallback), 82) : "A session was opened";
  const first = raw.split(/(?<=[.?!])\s/)[0] ?? raw;
  const cut = clamp(first.replace(/[.;:,]+$/, ""), 82);
  return cut.charAt(0).toUpperCase() + cut.slice(1);
}

function diffStat(diff: string): string | null {
  if (!diff) return null;
  let added = 0;
  let removed = 0;
  for (const line of diff.split("\n")) {
    if (line.startsWith("+++") || line.startsWith("---")) continue;
    if (line.startsWith("+")) added += 1;
    else if (line.startsWith("-")) removed += 1;
  }
  return added === 0 && removed === 0 ? null : `+${added} / −${removed}`;
}

function dispatchOf(item: Item): Dispatch | null {
  const body = record(item.body) ?? {};
  const result = record(body.result) ?? {};
  const args = record(body.arguments) ?? record(body.input) ?? record(body.raw_input) ?? {};
  const at = item.created_at;
  switch (item.kind) {
    case "message": {
      const said = text(body.text);
      if (!said || text(body.role) === "user") return null;
      return { tone: "said", text: clamp(clean(said), 360), detail: null, at };
    }
    case "command": {
      const command = text(body.command) ?? text(args.command) ?? text(result.command);
      if (!command) return null;
      const code =
        typeof body.exit_code === "number"
          ? body.exit_code
          : typeof result.exit_code === "number"
            ? result.exit_code
            : null;
      return {
        tone: "ran",
        text: clamp(clean(command), 150),
        detail: code === null ? null : code === 0 ? "exit 0" : `exit ${code}`,
        at,
      };
    }
    case "file_change": {
      const path =
        text(result.path) ?? text(body.path) ?? text(args.path) ?? text(result.file);
      const diff = text(result.diff) ?? text(body.diff) ?? "";
      return path ? { tone: "wrote", text: path, detail: diffStat(diff), at } : null;
    }
    case "mcp_call": {
      const tool = text(body.tool) ?? text(body.name);
      if (!tool) return null;
      const target =
        text(args.path) ??
        text(args.file) ??
        text(args.url) ??
        text(args.query) ??
        text(args.pattern) ??
        text(args.command);
      const reading =
        /^(read|list|grep|search|glob|view|inspect)/i.test(tool) || tool.includes("read_file");
      return {
        tone: reading ? "read" : "called",
        text: target ? `${tool} · ${clamp(clean(target), 120)}` : tool,
        detail: null,
        at,
      };
    }
    case "web_search": {
      const query = text(args.query) ?? text(body.query);
      return query ? { tone: "searched", text: clamp(clean(query), 150), detail: null, at } : null;
    }
    case "error": {
      const message = text(body.message) ?? text(body.text) ?? "The turn stopped with an error.";
      return { tone: "error", text: clamp(clean(message), 240), detail: null, at };
    }
    case "note": {
      const note = text(body.text);
      return note ? { tone: "note", text: clamp(clean(note), 240), detail: null, at } : null;
    }
    case "approval": {
      const title = text(body.title) ?? text(args.command);
      return title
        ? { tone: "note", text: `approval asked · ${clamp(clean(title), 130)}`, detail: null, at }
        : null;
    }
    default:
      return null;
  }
}

function pictureOf(item: Item, sessionId: string, agentId: string): Picture | null {
  const body = record(item.body) ?? {};
  const tool = text(body.tool) ?? "";
  const browser =
    tool === "browser_open" ||
    tool.startsWith("browser_") ||
    tool.toLowerCase().includes("obscura");
  if (!browser) return null;
  const result = record(body.result) ?? {};
  const args = record(body.arguments) ?? record(body.input) ?? {};
  const shot = text(result.screenshot);
  const url = httpUrl(text(args.url) ?? text(result.url) ?? text(body.url));
  if (!shot && !url) return null;
  return {
    sessionId,
    agentId,
    path: shot && !shot.startsWith("data:") ? shot : null,
    url,
    tool: tool || "browser",
    at: item.created_at,
  };
}

/* ------------------------------ building a day ------------------------------ */

function buildStory(dossier: Dossier, day: string, turns: TurnTranscript[]): Story {
  const ordered = [...turns].sort((a, b) => a.turn.started_at - b.turn.started_at);
  const items = ordered.flatMap((entry) =>
    entry.items.map((item) => ({ item, agentId: entry.turn.agent_id })),
  );
  items.sort((a, b) => a.item.created_at - b.item.created_at);
  const dispatches = items
    .map((row) => dispatchOf(row.item))
    .filter((row): row is Dispatch => row !== null)
    .slice(0, MAX_DISPATCHES);
  const pictures: Picture[] = [];
  for (const row of items) {
    const picture = pictureOf(row.item, dossier.session.id, row.agentId);
    if (picture) pictures.push(picture);
  }
  const dayPatches = dossier.patches.filter((row) => dayKey(row.created_at) === day);
  const asked = ordered.map((entry) => text(entry.turn.prompt)).find((row) => row !== null) ?? null;
  const lede = dispatches.find((row) => row.tone === "said")?.text ?? null;
  const at = ordered.reduce(
    (latest, entry) => Math.max(latest, entry.turn.completed_at ?? entry.turn.started_at),
    dossier.session.created_at,
  );
  const score =
    ordered.length * 3 +
    dayPatches.length * 2 +
    pictures.length * 3 +
    dispatches.filter((row) => row.tone === "said").length;

  return {
    key: `${dossier.session.id}:${day}`,
    day,
    session: dossier.session,
    at,
    headline: headlineOf(asked, dossier.session.title),
    asked,
    lede,
    dispatches,
    pictures: pictures.slice(0, 4),
    patches: dayPatches,
    agents: dossier.agents,
    turns: ordered,
    score,
  };
}

function buildEditions(dossiers: Dossier[]): Edition[] {
  const byDay = new Map<string, Story[]>();
  for (const dossier of dossiers) {
    const perDay = new Map<string, TurnTranscript[]>();
    for (const entry of dossier.turns) {
      const key = dayKey(entry.turn.started_at);
      const list = perDay.get(key) ?? [];
      list.push(entry);
      perDay.set(key, list);
    }
    if (perDay.size === 0) perDay.set(dayKey(dossier.session.created_at), []);
    for (const [day, turns] of perDay) {
      const list = byDay.get(day) ?? [];
      list.push(buildStory(dossier, day, turns));
      byDay.set(day, list);
    }
  }

  const editions: Edition[] = [];
  for (const [day, stories] of byDay) {
    const sorted = [...stories].sort((a, b) => b.score - a.score || b.at - a.at);
    const date = new Date(`${day}T12:00:00`);
    const turns = sorted.reduce((sum, story) => sum + story.turns.length, 0);
    const patches = sorted.reduce((sum, story) => sum + story.patches.length, 0);
    const tokens = sorted.reduce(
      (sum, story) =>
        sum +
        story.turns.reduce(
          (inner, entry) =>
            inner + (entry.turn.usage?.input_tokens ?? 0) + (entry.turn.usage?.output_tokens ?? 0),
          0,
        ),
      0,
    );
    const micros = sorted.reduce(
      (sum, story) =>
        sum + story.turns.reduce((inner, entry) => inner + (entry.turn.usage?.cost_micros ?? 0), 0),
      0,
    );
    const crew = new Map<string, Crew>();
    for (const story of sorted) {
      for (const agent of story.agents) {
        const label = agent.role ?? agent.harness ?? "agent";
        const key = `${label}:${agent.model ?? agent.harness}`;
        const row = crew.get(key) ?? {
          label,
          detail: agent.model ?? agent.harness,
          turns: 0,
        };
        row.turns += story.turns.filter((entry) => entry.turn.agent_id === agent.id).length;
        crew.set(key, row);
      }
    }
    editions.push({
      day,
      date,
      issue: dayNumber(date),
      stories: sorted,
      lead: sorted[0],
      turns,
      patches,
      tokens,
      micros,
      pictures: sorted.flatMap((story) => story.pictures).slice(0, 4),
      crew: [...crew.values()].filter((row) => row.turns > 0).sort((a, b) => b.turns - a.turns),
    });
  }

  return editions.sort((a, b) => b.day.localeCompare(a.day));
}

async function readDossier(project: string, session: Session): Promise<Dossier | null> {
  try {
    const [turns, patches, agents] = await Promise.all([
      api.transcript(project, session.id),
      api.listPatches(project, session.id),
      api.listAgents(project, session.id),
    ]);
    return { session, turns, patches, agents };
  } catch {
    // One unreadable session must not blank the paper: the other days still print.
    return null;
  }
}

/* --------------------------------- pictures --------------------------------- */

function rng(seed: number): () => number {
  let state = seed || 1;
  return () => {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return ((state >>> 0) % 10_000) / 10_000;
  };
}

function hostOf(url: string | null): string | null {
  if (!url) return null;
  try {
    return new URL(url).host;
  } catch {
    return null;
  }
}

/** A deterministic engraving for a story whose browser frame is not on disk. */
function Engraving({ seed }: { seed: string }) {
  const raw = useId();
  const clip = `engrave-${raw.replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const { sky, hills, birds, sun } = useMemo(() => {
    const random = rng(hashString(seed));
    return {
      sky: Array.from({ length: 26 }, (_, index) => ({
        y: 8 + index * 3.2,
        inset: random() * 12,
        opacity: 0.14 + random() * 0.24,
      })),
      hills: Array.from({ length: 3 }, (_, layer) => {
        const base = 124 + layer * 26;
        const points = Array.from(
          { length: 11 },
          (_, index) => `${-10 + index * 34},${(base - random() * 30).toFixed(1)}`,
        );
        return {
          points: `-10,210 ${points.join(" ")} 330,210`,
          opacity: 0.07 + layer * 0.05,
        };
      }),
      birds: Array.from({ length: 3 }, () => ({
        x: 36 + random() * 240,
        y: 34 + random() * 58,
        scale: 0.7 + random() * 0.8,
      })),
      sun: { x: 66 + random() * 180, y: 36 + random() * 20, r: 20 + random() * 14 },
    };
  }, [seed]);

  return (
    <svg
      className="paper-engraving"
      viewBox="0 0 320 210"
      role="img"
      aria-label="An engraved plate drawn from the day's record"
      preserveAspectRatio="xMidYMid slice"
    >
      <defs>
        <clipPath id={clip}>
          <circle cx={sun.x} cy={sun.y} r={sun.r} />
        </clipPath>
      </defs>
      <g stroke="currentColor" strokeWidth="0.35" fill="none">
        {sky.map((line, index) => (
          <line
            key={`sky-${index}`}
            y1={line.y}
            y2={line.y}
            x1={-8 + line.inset}
            x2={328 - line.inset}
            opacity={line.opacity}
          />
        ))}
      </g>
      <circle cx={sun.x} cy={sun.y} r={sun.r} fill="none" stroke="currentColor" strokeWidth="1.1" />
      <g clipPath={`url(#${clip})`} stroke="currentColor" strokeWidth="0.7" opacity="0.45">
        {Array.from({ length: 13 }, (_, index) => (
          <line
            key={`sun-${index}`}
            x1={sun.x - sun.r}
            x2={sun.x + sun.r + 6}
            y1={sun.y - sun.r + (index * (sun.r * 2)) / 12}
            y2={sun.y - sun.r + (index * (sun.r * 2)) / 12 + 3}
          />
        ))}
      </g>
      {hills.map((hill, index) => (
        <polygon key={`hill-${index}`} points={hill.points} fill="currentColor" opacity={hill.opacity} />
      ))}
      <g stroke="currentColor" strokeWidth="0.9" fill="none" opacity="0.7">
        {birds.map((bird, index) => (
          <path
            key={`bird-${index}`}
            transform={`translate(${bird.x} ${bird.y}) scale(${bird.scale})`}
            d="M0 0q4 -4 8 0q4 -4 8 0"
          />
        ))}
      </g>
      <rect x="0" y="196" width="320" height="14" fill="currentColor" opacity="0.16" />
    </svg>
  );
}

function captionFor(picture: Picture | null): string {
  if (!picture) return "No plate was filed; the engraving is drawn from the day's record.";
  const host = hostOf(picture.url);
  const time = new Date(picture.at).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
  const where = host ?? picture.tool;
  return `From the agent's browser, ${time} · ${where}`;
}

function Plate({ picture, seed }: { picture: Picture | null; seed: string }) {
  const [image, setImage] = useState<string | null>(null);
  const [missing, setMissing] = useState(false);

  useEffect(() => {
    let live = true;
    setImage(null);
    setMissing(false);
    if (!picture || !isTauri()) return;
    void (async () => {
      try {
        if (picture.path) {
          const next = await api.browserImage(picture.path);
          if (live) setImage(next);
          return;
        }
        if (!picture.url) return;
        const frame = await api.browserHarnessCapture(picture.sessionId, picture.agentId);
        if (frame.captured_at < picture.at) throw new Error("older than the call");
        if (live) setImage(frame.image);
      } catch {
        if (live) setMissing(true);
      }
    })();
    return () => {
      live = false;
    };
  }, [picture?.sessionId, picture?.agentId, picture?.path, picture?.url, picture?.at]);

  return (
    <figure className="paper-plate">
      <div className="paper-plate-frame">
        {image ? (
          <img src={image} alt={captionFor(picture)} />
        ) : (
          <Engraving seed={`${seed}:${picture?.tool ?? "plate"}`} />
        )}
      </div>
      <figcaption>
        {captionFor(picture)}
        {missing ? " The frame was no longer on disk." : ""}
      </figcaption>
    </figure>
  );
}

/* --------------------------------- the page --------------------------------- */

export default function HomeView({
  project,
  onSessions,
  onOpenSession,
  onOpenProject,
}: {
  project: string;
  onSessions: (rows: Session[]) => void;
  onOpenSession: (id: string) => void;
  onOpenProject: () => void;
}) {
  const [editions, setEditions] = useState<Edition[]>(() => (isTauri() ? [] : buildEditions(previewDossiers())));
  const [selected, setSelected] = useState<string | null>(null);
  const [loading, setLoading] = useState(isTauri());
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [updated, setUpdated] = useState<number | null>(() => (isTauri() ? null : Date.now()));
  const [nonce, setNonce] = useState(0);
  // The sponsored card is fetched after the edition renders, so the paper never waits on an ad.
  const [ad, setAd] = useState<GravityAd | null>(null);

  const load = useCallback(async () => {
    if (!isTauri()) {
      setEditions(buildEditions(previewDossiers()));
      setUpdated(Date.now());
      setLoading(false);
      setError(null);
      return;
    }
    if (!project) {
      setEditions([]);
      setLoading(false);
      return;
    }
    try {
      const listed = await api.listSessions(project);
      onSessions(listed);
      const cutoff = Date.now() - WINDOW_DAYS * 86_400_000;
      const recent = listed
        .filter((row) => (row.ended_at ?? Date.now()) >= cutoff)
        .sort((a, b) => b.created_at - a.created_at)
        .slice(0, MAX_SESSIONS);
      const dossiers: Dossier[] = [];
      for (let index = 0; index < recent.length; index += CHUNK) {
        const slice = recent.slice(index, index + CHUNK);
        const read = await Promise.all(slice.map((session) => readDossier(project, session)));
        dossiers.push(...read.filter((row): row is Dossier => row !== null));
      }
      setEditions(buildEditions(dossiers));
      setUpdated(Date.now());
      setError(null);
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setLoading(false);
    }
  }, [project, onSessions]);

  useEffect(() => {
    void load();
  }, [load, nonce]);

  useEffect(() => {
    const timer = window.setInterval(() => void load(), REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [load]);

  const current = useMemo(
    () => editions.find((edition) => edition.day === selected) ?? editions[0] ?? null,
    [editions, selected],
  );
  const index = current ? editions.findIndex((edition) => edition.day === current.day) : -1;
  const newer = index > 0 ? editions[index - 1] : null;
  const older = index >= 0 && index < editions.length - 1 ? editions[index + 1] : null;

  const refresh = useCallback(async () => {
    setRefreshing(true);
    try {
      await load();
    } finally {
      setRefreshing(false);
    }
  }, [load]);

  const place = project.split(/[\\/]/).filter(Boolean).pop() ?? "Rigel";
  const rest = current ? current.stories.slice(1) : [];
  const extras = current ? current.pictures.slice(0, 3) : [];
  // The deck is the first report in the record; printing it again as a dispatch would read
  // like the paper repeating its own front page.
  const leadDispatches = current
    ? current.lead.dispatches.filter((row) => row.text !== current.lead.lede)
    : [];

  // One request per edition. Disabled or unconfigured answers `[]`, so the slot is simply
  // absent; a failure is silent for the same reason. The two messages are the ad's whole
  // context: the day's headline and its deck.
  useEffect(() => {
    let live = true;
    if (!current) {
      setAd(null);
      return;
    }
    const context = `${current.lead.headline}. ${current.lead.lede ?? current.lead.asked ?? ""}`;
    void api
      .gravityAd(
        sponsorSession(project, current.day),
        [
          { role: "user", content: `Show me the day's work in ${place}.` },
          { role: "assistant", content: context.slice(0, 1_200) },
        ],
        {
          ua: navigator.userAgent,
          ip: "",
          os: navigator.userAgent.includes("Windows") ? "Windows" : "",
        },
      )
      .then((ads) => live && setAd(ads[0] ?? null))
      .catch(() => live && setAd(null));
    return () => {
      live = false;
    };
  }, [project, place, current]);

  return (
    <main id="main" tabIndex={-1} className="detail paper-view">
      <div className="paper-toolbar">
        <div className="paper-nav">
          <button
            className="chip"
            onClick={() => newer && setSelected(newer.day)}
            disabled={!newer}
            title="Newer edition"
          >
            ◀ Newer
          </button>
          <button
            className="chip"
            onClick={() => older && setSelected(older.day)}
            disabled={!older}
            title="Older edition"
          >
            Older ▶
          </button>
          <button
            className="chip"
            onClick={() => setSelected(editions[0]?.day ?? null)}
            disabled={index === 0 || editions.length === 0}
            title="Back to the latest paper"
          >
            Latest
          </button>
        </div>
        <span className="paper-stamp">
          {updated
            ? `Set ${new Date(updated).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
            : "reading the record…"}
          {!isTauri() ? " · sample edition" : ""}
        </span>
        <button className="chip" onClick={() => void refresh()} disabled={refreshing || loading}>
          {refreshing ? "Refreshing…" : "Refresh"}
        </button>
      </div>

      {error ? (
        <p className="notice" role="status">
          {error}
        </p>
      ) : null}

      {loading && editions.length === 0 ? (
        <p className="empty">Setting the type…</p>
      ) : !current ? (
        <article className="paper">
          <header className="paper-masthead">
            <div className="paper-ears">
              <span>Vol. {new Date().getFullYear()}</span>
              <span>Price: one good prompt</span>
              <span>Telegraphed from the local host</span>
            </div>
            <h1 className="paper-name">The Rigel Paper</h1>
            <div className="paper-dateline">
              <span>{place}</span>
              <span>{longDate(new Date())}</span>
              <span>No edition</span>
            </div>
          </header>
          <div className="paper-idle">
            <h2>The press is idle</h2>
            <p>
              No agent has filed work in this project yet. Open a session, give it something to
              do, and today's edition sets itself from the transcript — turns, patches, commands
              and the browser frames the agents captured.
            </p>
            <div className="paper-idle-actions">
              {!project ? (
                <button className="primary" onClick={onOpenProject}>
                  Open a project folder
                </button>
              ) : null}
              <button className="chip" onClick={() => setNonce((value) => value + 1)}>
                Look again
              </button>
            </div>
            <p className="paper-fine">
              The paper is derived from the project's session database. Nothing leaves this
              machine and nothing is written to make it.
            </p>
          </div>
        </article>
      ) : (
        <article className="paper" aria-label={`The Rigel Paper, ${longDate(current.date)}`}>
          <header className="paper-masthead">
            <div className="paper-ears">
              <span>Vol. {current.date.getFullYear()} · No. {current.issue}</span>
              <span>Price: one good prompt</span>
              <span>Telegraphed from the local host</span>
            </div>
            <h1 className="paper-name">The Rigel Paper</h1>
            <div className="paper-dateline">
              <span>{place}</span>
              <span>{longDate(current.date)}</span>
              <span>{count(current.stories.length, "dispatch", "dispatches")}</span>
            </div>
          </header>

          <div className="paper-body">
            <div className="paper-column">
              <section className="paper-lead">
                <p className="paper-kicker">
                  {current.lead.session.status === "active" ? "Still at work" : "The day's work"} ·{" "}
                  {current.lead.agents[0]?.role ?? "agent"}
                </p>
                <h2 className="paper-headline">{current.lead.headline}</h2>
                <p className="paper-deck">
                  {current.lead.lede ??
                    "The session filed no words of its own. What it ran is printed below."}
                </p>
                <div className="paper-lead-body">
                  <div className="paper-copy">
                    {current.lead.asked ? (
                      <p className="paper-asked">
                        <span>You asked</span> {current.lead.asked}
                      </p>
                    ) : null}
                    {leadDispatches.length > 0 ? (
                      <ul className="paper-dispatches">
                        {leadDispatches.map((dispatch, position) => (
                          <li key={`${current.lead.key}-${position}`}>
                            <span className={`paper-verb ${dispatch.tone}`}>
                              {DISPATCH_LABEL[dispatch.tone]}
                            </span>
                            <span className="paper-dispatch-text">
                              {dispatch.text}
                              {dispatch.detail ? (
                                <em className="paper-detail">{dispatch.detail}</em>
                              ) : null}
                            </span>
                          </li>
                        ))}
                      </ul>
                    ) : current.lead.lede === null ? (
                      <p className="paper-blank">Nothing was recorded beyond the session row.</p>
                    ) : null}
                    <p className="paper-byline">
                      {current.lead.patches.length > 0
                        ? `${count(current.lead.patches.length, "patch", "patches")} landed. `
                        : ""}
                      {count(current.lead.turns.length, "turn")} on the record.
                    </p>
                    <button
                      className="paper-link"
                      onClick={() => onOpenSession(current.lead.session.id)}
                    >
                      Read the session →
                    </button>
                  </div>
                  <Plate
                    picture={current.lead.pictures[0] ?? extras[0] ?? null}
                    seed={current.lead.key}
                  />
                </div>
              </section>

              {rest.length > 0 ? (
                <section className="paper-stories" aria-label="Also in this edition">
                  {rest.map((story) => (
                    <article className="paper-story" key={story.key}>
                      <p className="paper-story-kicker">
                        {story.agents[0]?.role ?? "agent"} ·{" "}
                        {new Date(story.at).toLocaleTimeString([], {
                          hour: "2-digit",
                          minute: "2-digit",
                        })}
                      </p>
                      <h3>{story.headline}</h3>
                      <p>{story.lede ?? story.asked ?? "No words were filed for this session."}</p>
                      <button
                        className="paper-link"
                        onClick={() => onOpenSession(story.session.id)}
                      >
                        Read the session →
                      </button>
                    </article>
                  ))}
                </section>
              ) : null}

              {extras.length > 1 ? (
                <section className="paper-gallery" aria-label="Pictures from the day">
                  <h3>Pictures from the Day</h3>
                  <div className="paper-gallery-row">
                    {extras.slice(1).map((picture, position) => (
                      <Plate
                        key={`${picture.sessionId}-${picture.at}-${position}`}
                        picture={picture}
                        seed={`${current.day}:${position}`}
                      />
                    ))}
                  </div>
                </section>
              ) : null}
            </div>

            <aside className="paper-market" aria-label="The day's numbers">
              <h3>Prices Current</h3>
              <dl>
                <div>
                  <dt>Turns filed</dt>
                  <dd>{current.turns.toLocaleString()}</dd>
                </div>
                <div>
                  <dt>Patches landed</dt>
                  <dd>{current.patches.toLocaleString()}</dd>
                </div>
                <div>
                  <dt>Sessions at work</dt>
                  <dd>{current.stories.length.toLocaleString()}</dd>
                </div>
                <div>
                  <dt>Tokens spent</dt>
                  <dd>{current.tokens.toLocaleString()}</dd>
                </div>
                <div>
                  <dt>Cost of the day</dt>
                  <dd>${(current.micros / 1_000_000).toFixed(4)}</dd>
                </div>
              </dl>

              <h3>Agents on the Beat</h3>
              {current.crew.length === 0 ? (
                <p className="paper-fine">No agent turns were recorded on this day.</p>
              ) : (
                <ul className="paper-crew">
                  {current.crew.map((row) => (
                    <li key={`${row.label}:${row.detail}`}>
                      <span className="paper-crew-role">{row.label}</span>
                      <span className="paper-crew-model" title={row.detail}>
                        {row.detail}
                      </span>
                      <span className="paper-crew-turns">{row.turns}</span>
                    </li>
                  ))}
                </ul>
              )}

              <h3>Standing Notice</h3>
              <p className="paper-fine">
                Every edition is set from this project's own session database and opens in the
                Session surface. A day with no work prints no paper.
              </p>

            </aside>
          </div>

          {ad ? <Sponsored ad={ad} /> : null}

          <footer className="paper-rack">
            <h3>Other Days' Papers</h3>
            <nav className="paper-rack-row" aria-label="Editions by day">
              {editions.map((edition) => (
                <button
                  key={edition.day}
                  className={`paper-issue${edition.day === current.day ? " on" : ""}`}
                  onClick={() => setSelected(edition.day)}
                  aria-current={edition.day === current.day ? "page" : undefined}
                >
                  <span className="paper-issue-day">{shortDate(edition.date)}</span>
                  <span className="paper-issue-head">{edition.lead.headline}</span>
                  <span className="paper-issue-meta">
                    {count(edition.stories.length, "story", "stories")} ·{" "}
                    {count(edition.turns, "turn")}
                  </span>
                </button>
              ))}
            </nav>
            <p className="paper-fine">
              Published whenever the agents stop. Refreshed every minute, or press Refresh.
            </p>
          </footer>
        </article>
      )}
    </main>
  );
}

/** A sponsored card, labelled as one. It never takes the lead story's type or byline. */
function Sponsored({ ad }: { ad: GravityAd }) {
  const [opening, setOpening] = useState(false);
  const [failed, setFailed] = useState(false);

  const cardRef = useRef<HTMLElement>(null);
  const impressed = useRef<string | null>(null);

  // Count a view only when at least half the card is visible, not merely mounted below the fold.
  useEffect(() => {
    const card = cardRef.current;
    if (!card || !ad.impUrl || impressed.current === ad.impUrl) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (document.visibilityState !== "visible") return;
        if (!entries.some((entry) => entry.isIntersecting && entry.intersectionRatio >= 0.5)) return;
        if (impressed.current === ad.impUrl) return;
        impressed.current = ad.impUrl;
        void api.gravityImpression(ad.impUrl);
        observer.disconnect();
      },
      { threshold: 0.5 },
    );
    const observeVisible = () => {
      if (document.visibilityState !== "visible" || impressed.current === ad.impUrl) return;
      observer.unobserve(card);
      observer.observe(card);
    };
    observer.observe(card);
    document.addEventListener("visibilitychange", observeVisible);
    return () => {
      observer.disconnect();
      document.removeEventListener("visibilitychange", observeVisible);
    };
  }, [ad.impUrl]);

  const open = async () => {
    if (!ad.clickUrl || opening) return;
    setOpening(true);
    setFailed(false);
    try {
      await api.gravityClick(ad.clickUrl);
    } catch {
      setFailed(true);
    } finally {
      setOpening(false);
    }
  };

  return (
    <section ref={cardRef} className="paper-sponsor" aria-label="Sponsored">
      <p className="paper-sponsor-label">Advertisement</p>
      <div className="paper-sponsor-body">
        {ad.brandName ? <p className="paper-sponsor-brand">{ad.brandName}</p> : null}
        <h3>{ad.title || "From the wire"}</h3>
        <p className="paper-sponsor-copy">{ad.adText}</p>
      </div>
      <button
        className="paper-link"
        type="button"
        onClick={() => void open()}
        disabled={!ad.clickUrl || opening}
      >
        {opening ? "Opening…" : ad.cta || "Learn more"} →
      </button>
      {failed ? (
        <p className="paper-sponsor-fine">The sponsor's page could not be opened.</p>
      ) : null}
      <p className="paper-sponsor-fine">
        Matched to this edition by Gravity. One sponsored card per paper; turn it off in Settings →
        Sponsors.
      </p>
    </section>
  );
}

const DISPATCH_LABEL: Record<Tone, string> = {
  said: "reported",
  ran: "ran",
  wrote: "wrote",
  read: "read",
  searched: "sought",
  called: "called",
  error: "struck",
  note: "noted",
};

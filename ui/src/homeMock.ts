/**
 * Sample editions for `npm run dev`.
 *
 * Home reads the project's session database, which the browser preview does not have. These
 * dossiers are the same shapes the real reader receives — sessions, turns, items, patches and
 * agents — so the whole paper can be inspected without a Tauri host. Nothing is billed,
 * persisted or streamed; the surface labels itself a sample edition.
 */

import type { Agent, Item, Patch, Session, Turn, TurnTranscript } from "./api";
import {
  previewAgents,
  previewPatches,
  previewSession,
  previewTranscript,
} from "./sessionMock";

export interface MockDossier {
  session: Session;
  turns: TurnTranscript[];
  patches: Patch[];
  agents: Agent[];
}

const at = (dayOffset: number, hour: number, minute: number): number => {
  const date = new Date();
  date.setDate(date.getDate() + dayOffset);
  date.setHours(hour, minute, 0, 0);
  return date.getTime();
};

function session(
  id: string,
  title: string,
  created: number,
  ended: number | null,
  micros: number,
  tasks: number,
): Session {
  return {
    id,
    project_root: "/preview/rigel",
    title,
    status: ended ? "ended" : "idle",
    budget_cents: 500,
    spend_cents: Math.round(micros / 10_000),
    spend_micros: micros,
    interventions: 1,
    tasks_done: tasks,
    created_at: created,
    ended_at: ended,
    pinned_at: null,
    archived_at: null,
  };
}

function agent(
  id: string,
  sessionId: string,
  role: string,
  harness: string,
  model: string,
  started: number,
  micros: number,
): Agent {
  return {
    id,
    session_id: sessionId,
    harness,
    role,
    model,
    status: "idle",
    started_at: started,
    spend_cents: Math.round(micros / 10_000),
    spend_micros: micros,
    tokens_in: 14_200,
    tokens_out: 1_050,
    claims_held: 0,
    claims_stale: 0,
  };
}

function turn(
  id: string,
  agentId: string,
  seq: number,
  prompt: string,
  started: number,
  minutes: number,
  tokensIn: number,
  micros: number,
): Turn {
  return {
    id,
    agent_id: agentId,
    parent_turn_id: null,
    seq,
    status: "completed",
    prompt,
    started_at: started,
    completed_at: started + minutes * 60_000,
    usage: {
      input_tokens: tokensIn,
      cached_input_tokens: Math.round(tokensIn * 0.6),
      output_tokens: Math.round(tokensIn * 0.08),
      reasoning_tokens: 0,
      cost_micros: micros,
      provider_micros: micros,
      source: "provider",
    },
    stop_reason: "completed",
  };
}

function item(
  id: string,
  turnId: string,
  seq: number,
  kind: Item["kind"],
  body: Record<string, unknown>,
  createdAt: number,
): Item {
  return { id, turn_id: turnId, seq, kind, status: "complete", body, created_at: createdAt };
}

const DIFF = [
  "--- a/crates/rigel-core/src/shelf.rs",
  "+++ b/crates/rigel-core/src/shelf.rs",
  "@@ -212,6 +212,9 @@ impl Shelf {",
  "     pub fn page(&self, query: &str, limit: usize, offset: usize) -> Vec<Row> {",
  "+        if offset >= self.total(query) {",
  "+            return Vec::new();",
  "+        }",
  "         self.scan(query).skip(offset).take(limit).collect()",
  "     }",
  "",
].join("\n");

function patch(id: string, sessionId: string, agentId: string, path: string, createdAt: number): Patch {
  return {
    id,
    session_id: sessionId,
    agent_id: agentId,
    path,
    base_hash: "6a1f…",
    result_hash: "b40c…",
    diff: DIFF,
    status: "applied",
    created_at: createdAt,
    applied_at: createdAt + 60_000,
    reverted_at: null,
  };
}

export function previewDossiers(): MockDossier[] {
  const pageStart = at(-1, 9, 12);
  const layoutStart = at(-1, 14, 5);
  const ciStart = at(-2, 10, 2);
  const soundStart = at(-2, 15, 26);
  const shellStart = at(-4, 11, 8);

  const pagerId = "paper-preview-pager";
  const pagerAgent = "paper-preview-pager-agent";
  const layoutId = "paper-preview-layout";
  const layoutAgent = "paper-preview-layout-agent";
  const ciId = "paper-preview-ci";
  const ciAgent = "paper-preview-ci-agent";
  const soundId = "paper-preview-sound";
  const soundAgent = "paper-preview-sound-agent";
  const shellId = "paper-preview-shell";
  const shellAgent = "paper-preview-shell-agent";

  const pagerTurnOne = turn(
    "paper-preview-pager-t1",
    pagerAgent,
    1,
    "The skill shelf stalls at ten thousand rows. Page the index and keep the count honest.",
    pageStart,
    9,
    9_800,
    21_400,
  );
  const pagerTurnTwo = turn(
    "paper-preview-pager-t2",
    pagerAgent,
    2,
    "Add the offset page and a load-more row; do not re-rank the results.",
    pageStart + 11 * 60_000,
    7,
    6_100,
    12_900,
  );

  const pager: MockDossier = {
    session: session(pagerId, "Skill shelf pages past 10,000 rows", pageStart, pageStart + 26 * 60_000, 34_300, 2),
    agents: [
      agent(pagerAgent, pagerId, "worker", "native", "anthropic/claude-sonnet-4.5", pageStart, 34_300),
    ],
    patches: [patch("paper-preview-pager-patch", pagerId, pagerAgent, "crates/rigel-core/src/shelf.rs", pageStart + 17 * 60_000)],
    turns: [
      {
        turn: pagerTurnOne,
        items: [
          item(
            "paper-preview-pager-i1",
            pagerTurnOne.id,
            1,
            "mcp_call",
            {
              tool: "read_file",
              arguments: { path: "crates/rigel-core/src/shelf.rs" },
              result: { path: "crates/rigel-core/src/shelf.rs", bytes: 12_804 },
            },
            pageStart + 60_000,
          ),
          item(
            "paper-preview-pager-i2",
            pagerTurnOne.id,
            2,
            "message",
            {
              role: "assistant",
              text:
                "The scan is rebuilt on every read, and the window is taken after the count. At ten thousand rows the count becomes the expensive half. I have not changed anything yet.",
            },
            pageStart + 4 * 60_000,
          ),
        ],
      },
      {
        turn: pagerTurnTwo,
        items: [
          item(
            "paper-preview-pager-i3",
            pagerTurnTwo.id,
            1,
            "file_change",
            {
              result: { path: "crates/rigel-core/src/shelf.rs", diff: DIFF, applied: true },
            },
            pageStart + 18 * 60_000,
          ),
          item(
            "paper-preview-pager-i4",
            pagerTurnTwo.id,
            2,
            "command",
            {
              command: "cargo test -p rigel-core shelf",
              exit_code: 0,
              output: "8 passed; 0 failed",
            },
            pageStart + 21 * 60_000,
          ),
          item(
            "paper-preview-pager-i5",
            pagerTurnTwo.id,
            3,
            "message",
            {
              role: "assistant",
              text:
                "One hunk, applied. The load-more row now asks for the next offset instead of rebuilding the first page, and the eight shelf tests pass.",
            },
            pageStart + 22 * 60_000,
          ),
        ],
      },
    ],
  };

  const layoutTurn = turn(
    "paper-preview-layout-t1",
    layoutAgent,
    1,
    "Lay out a front page: masthead, a lead story, a plate of the work and the day's numbers.",
    layoutStart,
    12,
    7_400,
    16_200,
  );

  const layout: MockDossier = {
    session: session(layoutId, "Set the Rigel Paper in type", layoutStart, layoutStart + 34 * 60_000, 16_200, 1),
    agents: [
      agent(layoutAgent, layoutId, "designer", "native", "anthropic/claude-opus-4.6", layoutStart, 16_200),
    ],
    patches: [],
    turns: [
      {
        turn: layoutTurn,
        items: [
          item(
            "paper-preview-layout-i1",
            layoutTurn.id,
            1,
            "mcp_call",
            {
              tool: "browser_open",
              arguments: { url: "https://rigel.local/paper/draft" },
              result: { url: "https://rigel.local/paper/draft" },
            },
            layoutStart + 3 * 60_000,
          ),
          item(
            "paper-preview-layout-i2",
            layoutTurn.id,
            2,
            "message",
            {
              role: "assistant",
              text:
                "The masthead is set in blackletter, the lead runs across two columns, and the plates are boxed in a single hairline. The page is readable at arm's length, which is the whole point of a front page.",
            },
            layoutStart + 9 * 60_000,
          ),
        ],
      },
    ],
  };

  const ciTurn = turn(
    "paper-preview-ci-t1",
    ciAgent,
    1,
    "Run the whole workspace on Windows and tell me what actually fails.",
    ciStart,
    14,
    11_200,
    24_800,
  );

  const ci: MockDossier = {
    session: session(ciId, "Windows CI, all green", ciStart, ciStart + 40 * 60_000, 24_800, 1),
    agents: [agent(ciAgent, ciId, "runner", "native", "anthropic/claude-sonnet-4.5", ciStart, 24_800)],
    patches: [],
    turns: [
      {
        turn: ciTurn,
        items: [
          item(
            "paper-preview-ci-i1",
            ciTurn.id,
            1,
            "command",
            {
              command: "cargo test --workspace",
              exit_code: 0,
              output: "342 passed; 0 failed; 1 ignored",
            },
            ciStart + 9 * 60_000,
          ),
          item(
            "paper-preview-ci-i2",
            ciTurn.id,
            2,
            "message",
            {
              role: "assistant",
              text:
                "The whole workspace is green on Windows: 342 passed, one ignored live test. The ignored one needs Roblox Studio open, so it stays opt-in.",
            },
            ciStart + 12 * 60_000,
          ),
        ],
      },
    ],
  };

  const soundTurn = turn(
    "paper-preview-sound-t1",
    soundAgent,
    1,
    "Give the toy town a jump sound and a pickup chime, and keep them short.",
    soundStart,
    8,
    5_400,
    11_100,
  );

  const sound: MockDossier = {
    session: session(soundId, "A sound pack for the toy town", soundStart, soundStart + 22 * 60_000, 11_100, 2),
    agents: [agent(soundAgent, soundId, "composer", "native", "anthropic/gpt-5.4", soundStart, 11_100)],
    patches: [
      patch("paper-preview-sound-patch", soundId, soundAgent, ".rigel/roblox/sounds/jump.wav", soundStart + 6 * 60_000),
    ],
    turns: [
      {
        turn: soundTurn,
        items: [
          item(
            "paper-preview-sound-i1",
            soundTurn.id,
            1,
            "mcp_call",
            {
              tool: "roblox_sound",
              arguments: { preset: "jump", duration_secs: 0.22 },
              result: { path: ".rigel/roblox/sounds/jump.wav", sample_rate: 44_100 },
            },
            soundStart + 2 * 60_000,
          ),
          item(
            "paper-preview-sound-i2",
            soundTurn.id,
            2,
            "message",
            {
              role: "assistant",
              text:
                "Two effects rendered at 44.1 kHz: a 220 ms jump and a 600 ms pickup chime. Both are deterministic, so a rebuild writes the same bytes.",
            },
            soundStart + 5 * 60_000,
          ),
        ],
      },
    ],
  };

  const shellTurn = turn(
    "paper-preview-shell-t1",
    shellAgent,
    1,
    "Start the native shell and prove a session can be opened from the rail.",
    shellStart,
    6,
    4_800,
    9_600,
  );

  const shell: MockDossier = {
    session: session(shellId, "First light on the native shell", shellStart, shellStart + 18 * 60_000, 9_600, 1),
    agents: [agent(shellAgent, shellId, "builder", "native", "anthropic/claude-sonnet-4.5", shellStart, 9_600)],
    patches: [],
    turns: [
      {
        turn: shellTurn,
        items: [
          item(
            "paper-preview-shell-i1",
            shellTurn.id,
            1,
            "message",
            {
              role: "assistant",
              text:
                "The window opens on one grid, the rail switches surfaces without losing the terminal, and a session opens from the list. The first light is on.",
            },
            shellStart + 4 * 60_000,
          ),
        ],
      },
    ],
  };

  // Today's sample is the session the Session surface shows, so a click from the paper lands
  // on the transcript the preview already carries.
  const today: MockDossier = {
    session: previewSession,
    turns: previewTranscript,
    patches: previewPatches,
    agents: previewAgents,
  };

  return [today, pager, layout, ci, sound, shell];
}

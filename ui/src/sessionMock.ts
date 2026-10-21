/**
 * Sample data for `npm run dev`.
 *
 * Same shapes as the Rust structs so the Session surface can be inspected in a browser, but
 * nothing here is billed, persisted or streamed. The view labels it "sample transcript".
 */

import type { Agent, BudgetSnapshot, Item, Patch, Session, Turn, TurnTranscript } from "./api";

const now = Date.now();
const ago = (minutes: number) => now - minutes * 60_000;

export const previewSession: Session = {
  id: "7c1e4a3d-0000-4000-8000-000000000000",
  project_root: "/preview/rigel",
  title: "Fix the flaky resume test",
  status: "active",
  budget_cents: 500,
  spend_cents: 3,
  spend_micros: 32_600,
  interventions: 1,
  tasks_done: 2,
  created_at: ago(90),
  ended_at: null,
};

export const previewAgents: Agent[] = [
  {
    id: "agent-preview-worker",
    session_id: previewSession.id,
    harness: "native",
    role: "worker",
    model: "anthropic/claude-sonnet-4.5",
    status: "idle",
    started_at: ago(90),
    spend_cents: 3,
    spend_micros: 31_400,
    tokens_in: 18_420,
    tokens_out: 1_240,
  },
  {
    id: "agent-preview-reader",
    session_id: previewSession.id,
    harness: "native",
    role: "reader",
    model: "openai/gpt-5.4-nano",
    status: "idle",
    started_at: ago(5),
    spend_cents: 0,
    spend_micros: 1_200,
    tokens_in: 2_900,
    tokens_out: 180,
  },
];

const turnOne: Turn = {
  id: "turn-preview-1",
  agent_id: "agent-preview-worker",
  parent_turn_id: null,
  seq: 1,
  status: "completed",
  prompt: "The resume test fails on Windows. Find the cause; do not patch anything yet.",
  started_at: ago(12),
  completed_at: ago(11),
  usage: {
    input_tokens: 12_100,
    cached_input_tokens: 8_000,
    output_tokens: 640,
    reasoning_tokens: 210,
    cost_micros: 18_900,
    provider_micros: 18_900,
    source: "provider",
  },
  stop_reason: "completed",
};

const items: Item[] = [
  {
    id: "item-preview-1",
    turn_id: turnOne.id,
    seq: 1,
    kind: "reasoning",
    status: "complete",
    body: {
      text: "The test writes a file and immediately reads the tree hash. On Windows the rename lands after the read, so the checkpoint sees the old tree.",
    },
    created_at: ago(12),
  },
  {
    id: "item-preview-2",
    turn_id: turnOne.id,
    seq: 2,
    kind: "mcp_call",
    status: "ok",
    body: {
      tool: "read_file",
      arguments: { path: "crates/rigel-core/tests/resume.rs" },
      result: { path: "crates/rigel-core/tests/resume.rs", bytes: 4_210 },
    },
    created_at: ago(12),
  },
  {
    id: "item-preview-3",
    turn_id: turnOne.id,
    seq: 3,
    kind: "message",
    status: "complete",
    body: {
      role: "assistant",
      text: "Cause: `Store::write` fsyncs the file but the test hashes the tree before the parent directory entry is durable. On Windows that ordering is observable. I did not change any file — say the word and I will patch the test to hash after the rename.",
    },
    created_at: ago(11),
  },
  {
    id: "item-preview-4",
    turn_id: turnOne.id,
    seq: 4,
    kind: "usage",
    status: "complete",
    body: {
      input_tokens: 12_100,
      output_tokens: 640,
      cost_micros: 18_900,
      source: "provider",
    },
    created_at: ago(11),
  },
];

const turnTwo: Turn = {
  id: "turn-preview-2",
  agent_id: "agent-preview-worker",
  parent_turn_id: null,
  seq: 2,
  status: "completed",
  prompt: "Patch the test: hash after the rename. Keep it one hunk.",
  started_at: ago(5),
  completed_at: ago(4),
  usage: {
    input_tokens: 6_320,
    cached_input_tokens: 5_000,
    output_tokens: 600,
    reasoning_tokens: 0,
    cost_micros: 12_500,
    provider_micros: 12_500,
    source: "provider",
  },
  stop_reason: "completed",
};

const turnChild: Turn = {
  id: "turn-preview-child",
  agent_id: "agent-preview-reader",
  parent_turn_id: turnTwo.id,
  seq: 1,
  status: "completed",
  prompt: "Read crates/rigel-core/tests/resume.rs and summarise the failing ordering.",
  started_at: ago(5),
  completed_at: ago(4),
  usage: {
    input_tokens: 2_900,
    cached_input_tokens: 2_000,
    output_tokens: 180,
    reasoning_tokens: 0,
    cost_micros: 1_200,
    provider_micros: 1_200,
    source: "provider",
  },
  stop_reason: "completed",
};

const childItems: Item[] = [
  {
    id: "item-preview-child-1",
    turn_id: turnChild.id,
    seq: 1,
    kind: "message",
    status: "complete",
    body: {
      role: "assistant",
      text: "The test hashes the tree before the rename is durable on Windows; hash after the write, not before.",
    },
    created_at: ago(4),
  },
];

export const previewTranscript: TurnTranscript[] = [
  { turn: turnOne, items },
  {
    turn: turnTwo,
    items: [
      {
        id: "item-preview-5",
        turn_id: turnTwo.id,
        seq: 1,
        kind: "mcp_call",
        status: "ok",
        body: {
          tool: "edit_file",
          arguments: { path: "crates/rigel-core/tests/resume.rs" },
          result: {
            path: "crates/rigel-core/tests/resume.rs",
            patch_id: "patch-preview-1",
            applied: true,
            diff: "--- a/crates/rigel-core/tests/resume.rs\n+++ b/crates/rigel-core/tests/resume.rs\n@@ -44,3 +44,3 @@\n-    let before = tree_hash(&root)?;\n     store.write(\"note.txt\", b\"one\").await?;\n+    let before = tree_hash(&root)?;\n",
          },
        },
        created_at: ago(5),
      },
      {
        id: "item-preview-6",
        turn_id: turnTwo.id,
        seq: 2,
        kind: "file_change",
        status: "applied",
        body: {
          result: {
            path: "crates/rigel-core/tests/resume.rs",
            patch_id: "patch-preview-1",
            diff: "--- a/crates/rigel-core/tests/resume.rs\n+++ b/crates/rigel-core/tests/resume.rs\n@@ -44,3 +44,3 @@\n-    let before = tree_hash(&root)?;\n     store.write(\"note.txt\", b\"one\").await?;\n+    let before = tree_hash(&root)?;\n",
          },
        },
        created_at: ago(5),
      },
      {
        id: "item-preview-7",
        turn_id: turnTwo.id,
        seq: 3,
        kind: "message",
        status: "complete",
        body: {
          role: "assistant",
          text: "Applied one hunk to the test. `cargo test -p rigel-core --test resume` passes locally. Run the Windows suite before you trust it.",
        },
        created_at: ago(4),
      },
    ],
  },
  { turn: turnChild, items: childItems },
];

export const previewBudget: BudgetSnapshot = {
  session_id: previewSession.id,
  agent_id: previewAgents[0].id,
  session_micros: previewSession.spend_micros,
  agent_micros: previewAgents[0].spend_micros,
  cap_micros: 5_000_000,
  remaining_micros: 4_967_400,
  session_cents: 3,
  agent_cents: 3,
  cap_cents: 500,
  remaining_cents: 497,
};

export const previewPatches: Patch[] = [
  {
    id: "patch-preview-1",
    session_id: previewSession.id,
    agent_id: previewAgents[0].id,
    path: "crates/rigel-core/tests/resume.rs",
    base_hash: "9f2c…",
    result_hash: "41ab…",
    diff: "--- a/crates/rigel-core/tests/resume.rs\n+++ b/crates/rigel-core/tests/resume.rs\n@@ -44,3 +44,3 @@\n-    let before = tree_hash(&root)?;\n     store.write(\"note.txt\", b\"one\").await?;\n+    let before = tree_hash(&root)?;\n",
    status: "applied",
    created_at: ago(5),
    applied_at: ago(5),
    reverted_at: null,
  },
];

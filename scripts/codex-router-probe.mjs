#!/usr/bin/env node
/**
 * Probe Codex against the local OmniRoute router (D39, P4.5).
 *
 * Answers one question: does Codex really work through the router, on both of
 * its transports? Runs `codex exec --json` and one `codex app-server` turn, then
 * prints PASS/FAIL per transport and the token usage of the exec turn.
 *
 * No key is needed: the router answers /v1/responses from localhost without
 * auth (see §4.2 of the handoff, which also records why that is a gap rather
 * than a feature). Set CODEX_EXE if `codex.exe` is not on PATH.
 *
 *   node scripts/codex-router-probe.mjs
 *   node scripts/codex-router-probe.mjs --model gpt-5.6-sol-medium --base http://127.0.0.1:20128/v1
 */
import { spawn } from "node:child_process";

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const index = args.indexOf(`--${name}`);
  return index >= 0 && args[index + 1] ? args[index + 1] : fallback;
};

const EXE = process.env.CODEX_EXE || "codex.exe";
const BASE = flag("base", "http://127.0.0.1:20128/v1");
const MODEL = flag("model", "gpt-5.6-sol-medium");
const CWD = flag("cwd", process.cwd());
const PROMPT = "reply with exactly: pong";

const OVERRIDES = [
  'model_provider="omniroute"',
  'model_providers.omniroute.name="OmniRoute"',
  `model_providers.omniroute.base_url="${BASE}"`,
  'model_providers.omniroute.wire_api="responses"',
  "model_providers.omniroute.requires_openai_auth=false",
];

function baseArgs() {
  return OVERRIDES.flatMap((pair) => ["-c", pair]);
}

/** One `codex exec --json` turn. Resolves to {ok, text, usage, stderr}. */
function runExec(timeoutMs = 120_000) {
  return new Promise((resolve) => {
    const child = spawn(EXE, ["exec", "--json", "--skip-git-repo-check", ...baseArgs(), "-m", MODEL, PROMPT], {
      cwd: CWD,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let out = "";
    let err = "";
    let text = null;
    let usage = null;
    let settled = false;
    const timer = setTimeout(() => {
      if (!settled) {
        settled = true;
        child.kill();
        resolve({ ok: false, reason: `timeout after ${timeoutMs}ms`, text, usage, stderr: err });
      }
    }, timeoutMs);

    child.stdout.on("data", (chunk) => {
      out += chunk.toString();
      for (const line of out.split(/\r?\n/)) {
        if (!line.trim().startsWith("{")) continue;
        let event;
        try {
          event = JSON.parse(line);
        } catch {
          continue;
        }
        if (event.type === "item.completed" && event.item?.type === "agent_message") text = event.item.text;
        if (event.type === "turn.completed") usage = event.usage ?? null;
      }
      // Keep only the tail so the buffer cannot grow without bound.
      if (out.length > 1 << 20) out = out.slice(-(1 << 19));
    });
    child.stderr.on("data", (chunk) => {
      err += chunk.toString();
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      if (!settled) {
        settled = true;
        resolve({ ok: false, reason: `spawn failed: ${error.message}`, text, usage, stderr: err });
      }
    });
    child.on("exit", (code) => {
      clearTimeout(timer);
      if (settled) return;
      settled = true;
      resolve({ ok: code === 0 && text !== null, reason: `exit ${code}`, text, usage, stderr: err });
    });
  });
}

/** One `codex app-server` turn: initialize → thread/start → turn/start. */
function runAppServer(timeoutMs = 120_000) {
  return new Promise((resolve) => {
    const child = spawn(EXE, ["app-server", ...baseArgs()], { cwd: CWD, stdio: ["pipe", "pipe", "pipe"] });
    let buffer = "";
    let text = null;
    let turnStatus = null;
    let settled = false;
    const timer = setTimeout(() => done(`timeout after ${timeoutMs}ms`), timeoutMs);

    function done(reason) {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        child.kill();
      } catch {}
      resolve({ ok: turnStatus === "completed" && text !== null, reason, text, status: turnStatus });
    }

    const send = (id, method, params) =>
      child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");

    child.on("error", (error) => done(`spawn failed: ${error.message}`));
    child.on("exit", (code) => done(`exit ${code}`));
    child.stdout.on("data", (chunk) => {
      buffer += chunk.toString();
      let index;
      while ((index = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, index).trim();
        buffer = buffer.slice(index + 1);
        if (!line) continue;
        let message;
        try {
          message = JSON.parse(line);
        } catch {
          continue;
        }
        if (message.id === 1) {
          if (message.error) return done("initialize failed");
          send(2, "thread/start", {
            model: MODEL,
            modelProvider: "omniroute",
            cwd: CWD,
            approvalPolicy: "never",
            sandbox: "read-only",
          });
        } else if (message.id === 2) {
          if (message.error) return done(`thread/start failed: ${JSON.stringify(message.error).slice(0, 200)}`);
          const threadId = message.result?.threadId ?? message.result?.thread?.id ?? message.result?.id;
          send(3, "turn/start", { threadId, input: [{ type: "text", text: PROMPT }] });
        } else if (message.id === 3) {
          if (message.error) return done(`turn/start failed: ${JSON.stringify(message.error).slice(0, 200)}`);
        } else if (message.method === "item/agentMessage/delta") {
          text = (text ?? "") + (message.params?.delta ?? "");
        } else if (message.method === "item/completed" && message.params?.item?.type === "agentMessage") {
          text = message.params.item.text ?? text;
        } else if (message.method === "turn/completed") {
          turnStatus = message.params?.turn?.status ?? "unknown";
          done("turn/completed");
        } else if (message.method === "turn/failed") {
          return done("turn/failed");
        }
      }
    });

    send(1, "initialize", { clientInfo: { name: "rigel-probe", version: "0.0.0" } });
  });
}

const exec = await runExec();
console.log(`exec --json       ${exec.ok ? "PASS" : "FAIL"}  text=${JSON.stringify(exec.text)}  ${exec.reason}`);
if (exec.usage) {
  console.log(
    `                  input=${exec.usage.input_tokens} output=${exec.usage.output_tokens} cached=${exec.usage.cached_input_tokens ?? 0}`,
  );
}
if (!exec.ok && exec.stderr) console.log(`                  stderr: ${exec.stderr.trim().split(/\r?\n/).slice(-2).join(" | ").slice(0, 300)}`);

const server = await runAppServer();
console.log(`app-server        ${server.ok ? "PASS" : "FAIL"}  text=${JSON.stringify(server.text)}  ${server.reason}`);

process.exit(exec.ok && server.ok ? 0 : 1);

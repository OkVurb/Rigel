#!/usr/bin/env node
/**
 * Item 1: run the built Tauri app under tauri-driver + msedgedriver.
 *
 * `cargo build` sets `cfg(dev)`, so that binary points its webview at `devUrl` and shows an error
 * page when no Vite server is up. `tauri build --debug` embeds `ui/dist` instead; that is the
 * binary this runner drives.
 *
 * msedgedriver's own launch path no longer reaches the WebView2 debug port on this runtime
 * (`DevToolsActivePort file doesn't exist`), so the app is started here with a fixed
 * `--remote-debugging-port` and msedgedriver attaches to it through `ms:edgeOptions`. tauri-driver
 * stays in the middle: it is the WebDriver endpoint the specs talk to.
 *
 * The turn itself needs no key: `RIGEL_E2E_SCRIPT` makes `turn_send` use the core's ScriptedModel,
 * while approvals, tools, patches, the transcript and the bill stay on the production path.
 *
 * Usage: node e2e/run.mjs [--skip-build] [--spec <file>]
 */

import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, rmSync, statSync, symlinkSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
// A run-scoped directory: a killed WebView2 can keep files locked, and a wipe that fails would
// fail the suite for reasons that have nothing to do with the app.
const STAMP = new Date().toISOString().replace(/[:.]/g, "-");
const WORK = path.join(ROOT, "tmp", "e2e", `run-${STAMP}`);
const PROJECT = path.join(WORK, "e2e-project");
const APP = path.join(ROOT, "target", "debug", "rigel-app.exe");
const DRIVER = path.join(process.env.USERPROFILE ?? "", ".cargo", "bin", "tauri-driver.exe");
const PORT = 4446;
const NATIVE_PORT = 9516;
// Derived from the pid so a leftover browser process cannot hold the port a new run needs.
const CDP_PORT = 9300 + (process.pid % 300);

const args = process.argv.slice(2);
const skipBuild = args.includes("--skip-build");
const specIndex = args.indexOf("--spec");
const spec = specIndex >= 0 ? args[specIndex + 1] : null;

function run(command, commandArgs, options = {}) {
  console.log(`\n$ ${command} ${commandArgs.join(" ")}`);
  execFileSync(command, commandArgs, { cwd: ROOT, stdio: "inherit", ...options });
}

/**
 * The specs live beside this file but the WebdriverIO packages are installed in `ui/`. Node
 * resolves `@wdio/globals` upward from the spec, so the nearest `node_modules` has to be this one.
 */
function ensureModuleLink() {
  const link = path.join(ROOT, "e2e", "node_modules");
  if (existsSync(link)) return;
  symlinkSync(
    path.join(ROOT, "ui", "node_modules"),
    link,
    process.platform === "win32" ? "junction" : "dir",
  );
}

/** Old run directories can hold WebView2 locks; best effort, never fatal. */
function pruneRuns(keepMs = 12 * 60 * 60 * 1000) {
  const root = path.join(ROOT, "tmp", "e2e");
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    if (!entry.isDirectory() || !entry.name.startsWith("run-")) continue;
    const full = path.join(root, entry.name);
    if (full === WORK) continue;
    try {
      if (Date.now() - statSync(full).mtimeMs < keepMs) continue;
      rmSync(full, { recursive: true, force: true, maxRetries: 2 });
    } catch {
      // A live WebView2 process still owns it; a later run will try again.
    }
  }
}

function prepareWorkspace() {
  mkdirSync(WORK, { recursive: true });
  pruneRuns();
  for (const dir of ["appdata", "localappdata", "index", "webview"]) {
    mkdirSync(path.join(WORK, dir), { recursive: true });
  }
  // The window's Home surface reads settings.toml from APPDATA. Seed the two Gravity switches
  // here so the real window test proves the spawned card without driving the Settings UI first.
  const appData = path.join(WORK, "appdata", "Rigel");
  mkdirSync(appData, { recursive: true });
  writeFileSync(
    path.join(appData, "settings.toml"),
    [
      'theme = "dark"',
      "default_budget_cents = 500",
      "gravity_ads = true",
      "gravity_test_ads = true",
      "",
    ].join(String.fromCharCode(10)),
  );
  mkdirSync(PROJECT, { recursive: true });
  writeFileSync(path.join(PROJECT, "README.md"), "# E2E\n");
  writeFileSync(path.join(PROJECT, "note.txt"), "unchanged\n");

  // Three steps: ask for a destructive command, edit a file, close. A reply with no tool calls
  // ends the turn, so no announce text can come first. The approval card and the diff come from
  // the real coordinator and the real patch pipeline, not from a fixture.
  const changed = ["# E2E", "", "Approved change.", ""].join(String.fromCharCode(10));
  const script = {
    steps: [
      {
        tool_calls: [
          {
            name: "run_command",
            arguments: { command: "rm -rf $RIGEL_E2E_MISSING_DIR" },
          },
        ],
      },
      {
        tool_calls: [
          {
            name: "read_file",
            arguments: { path: "README.md" },
          },
        ],
      },
      {
        tool_calls: [
          {
            name: "edit_file",
            arguments: {
              path: "README.md",
              old_string: "# E2E",
              new_string: changed,
            },
          },
        ],
      },
      {
        text: "Done.",
        usage: { input_tokens: 30, output_tokens: 5, cost_usd: 0.0001 },
      },
    ],
  };
  writeFileSync(path.join(WORK, "script.json"), JSON.stringify(script, null, 2));
}

async function waitForCdp(timeoutMs = 40_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${CDP_PORT}/json/version`);
      if (response.ok) {
        const body = await response.json();
        console.log(`webview: ${body.Browser}`);
        return;
      }
    } catch {
      // Not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`the app never opened a debug port on ${CDP_PORT}`);
}

async function waitForDriver(timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${PORT}/status`);
      if (response.ok) {
        const body = await response.json();
        if (body?.value?.ready) {
          console.log(`driver: ${body.value.message}`);
          return;
        }
      }
    } catch {
      // Not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`tauri-driver did not become ready on port ${PORT}`);
}

function stopTree(child) {
  if (!child || child.exitCode !== null) return;
  if (process.platform === "win32") {
    try {
      execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
      return;
    } catch {
      // Fall through to the portable kill.
    }
  }
  child.kill("SIGTERM");
}

async function main() {
  ensureModuleLink();
  if (!skipBuild) {
    // The Tauri CLI parses `CI`; `CI=1` is valid for most tools but not for it.
    run("cargo", ["tauri", "build", "--debug", "--no-bundle"], {
      cwd: path.join(ROOT, "crates", "rigel-app"),
      env: { ...process.env, CI: "true" },
    });
  }
  if (!existsSync(APP)) throw new Error(`missing app binary: ${APP}`);
  if (!existsSync(DRIVER)) throw new Error(`missing tauri-driver: ${DRIVER}`);
  prepareWorkspace();

  const env = {
    ...process.env,
    RIGEL_E2E_PROJECT: PROJECT,
    RIGEL_E2E_SCRIPT: path.join(WORK, "script.json"),
    RIGEL_INDEX_DIR: path.join(WORK, "index"),
    APPDATA: path.join(WORK, "appdata"),
    LOCALAPPDATA: path.join(WORK, "localappdata"),
    // USERPROFILE and HOME stay real: redirecting them stops WebView2 from opening the debug
    // port at all (measured: arg reaches the browser, nothing binds). Settings and the index are
    // already isolated by APPDATA, LOCALAPPDATA, RIGEL_INDEX_DIR and the webview folder.
    RIGEL_OMNIROUTE_URL: "http://127.0.0.1:9",
    // A fixed port, so msedgedriver can attach to a webview this runner started.
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`,
    // WebView2 honours the debug port only while it starts a browser process. A folder that a
    // still-running process owns is joined without it, so every run gets its own folder.
    WEBVIEW2_USER_DATA_FOLDER: path.join(WORK, "webview"),
  };

  const app = spawn(APP, [], {
    cwd: ROOT,
    env,
    stdio: ["ignore", "pipe", "pipe"],
  });
  app.stdout.on("data", (chunk) => process.stdout.write(`app: ${chunk}`));
  app.stderr.on("data", (chunk) => process.stderr.write(`app: ${chunk}`));
  app.on("exit", (code) => {
    if (code !== 0) console.error(`app exited early with ${code}`);
  });

  let driver = null;
  let exitCode = 1;
  try {
    await waitForCdp();
    driver = spawn(DRIVER, ["--port", String(PORT), "--native-port", String(NATIVE_PORT)], {
      cwd: ROOT,
      env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    driver.stdout.on("data", (chunk) => process.stdout.write(`driver: ${chunk}`));
    driver.stderr.on("data", (chunk) => process.stderr.write(`driver: ${chunk}`));
    await waitForDriver();

    const wdio = path.join(ROOT, "ui", "node_modules", "@wdio", "cli", "bin", "wdio.js");
    const wdioArgs = ["run", "e2e/wdio.conf.ts"];
    if (spec) wdioArgs.push("--spec", spec);
    console.log(`\n$ node ${wdioArgs.join(" ")}`);
    exitCode = await new Promise((resolve) => {
      const child = spawn(process.execPath, [wdio, ...wdioArgs], {
        cwd: ROOT,
        env: { ...env, RIGEL_E2E_CDP_PORT: String(CDP_PORT), RIGEL_E2E_PORT: String(PORT) },
        stdio: "inherit",
      });
      child.on("exit", (code) => resolve(code ?? 1));
    });
  } finally {
    stopTree(driver);
    stopTree(app);
  }
  console.log(`\nwindow e2e exit: ${exitCode}`);
  process.exit(exitCode);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});

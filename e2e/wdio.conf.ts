import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/**
 * Item 1: the window suite. `e2e/run.mjs` builds the asset-embedded app, starts it with a fixed
 * `--remote-debugging-port`, then starts tauri-driver. This file only names the endpoint: the
 * `ms:edgeOptions.debuggerAddress` form is what lets msedgedriver attach to a webview it did not
 * spawn, and tauri-driver remains the WebDriver endpoint the specs talk to.
 */
const cdpPort = Number(process.env.RIGEL_E2E_CDP_PORT ?? 9336);
const driverPort = Number(process.env.RIGEL_E2E_PORT ?? 4446);


export const config: WebdriverIO.Config = {
  runner: "local",
  specs: ["./specs/**/*.spec.ts"],
  maxInstances: 1,
  capabilities: [
    {
      maxInstances: 1,
      browserName: "MicrosoftEdge",
      "ms:edgeOptions": {
        debuggerAddress: `127.0.0.1:${cdpPort}`,
      },
    } as WebdriverIO.Capabilities,
  ],
  hostname: "127.0.0.1",
  port: driverPort,
  path: "/",
  logLevel: "warn",
  framework: "mocha",
  mochaOpts: {
    ui: "bdd",
    timeout: 120_000,
  },
  reporters: ["spec"],
  waitforTimeout: 20_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 1,
  // Each spec shares the one window; a session is a detach, not a fresh app.
  specFileRetries: 0,
  bail: 0,

  // A new session opens on the blank target. Navigating it to the app origin loads the same
  // UI the shipped window shows, and the app reopens the prepared project on load.
  before: async () => {
    await browser.url("http://tauri.localhost/");
  },

  // The tray window is destroyed in E2E builds, so the driver attaches to the session window
  // itself; a shipped build keeps both windows and no spec has to pick one.
};

export const appBinary = path.join(ROOT, "target", "debug", "rigel-app.exe");

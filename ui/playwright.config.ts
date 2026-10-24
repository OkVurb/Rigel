import { defineConfig } from "@playwright/test";

// The window talks to Rust through Tauri IPC, which a browser cannot provide. This suite runs
// the built renderer in its browser preview instead: the same components, the sample rows the
// preview already ships, and no database. It is a smoke test for the five surfaces — a blank
// or throwing tab fails here before anyone opens a build.
export default defineConfig({
  testDir: "./tests",
  timeout: 30_000,
  expect: { timeout: 5_000 },
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? "line" : "list",
  use: {
    baseURL: "http://127.0.0.1:4173",
    trace: "off",
    screenshot: "off",
    video: "off",
  },
  webServer: {
    command: "npm run preview -- --host 127.0.0.1 --port 4173 --strictPort",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
  projects: [{ name: "chromium", use: { browserName: "chromium" } }],
});

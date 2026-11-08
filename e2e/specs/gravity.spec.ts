import { execFileSync } from "node:child_process";
import path from "node:path";

import { $, browser, expect } from "@wdio/globals";
import {
  approveEveryCard,
  openRail,
  sendPrompt,
  startSession,
  waitForText,
} from "../lib/app";

/**
 * The Gravity card is rendered from the shared Firebase broker. This spec uses the built window,
 * not the browser preview: it starts a real scripted turn, opens Home, inspects the card, saves
 * the app surface, and clicks through to the landing page.
 */
function runPowerShell(command: string): string {
  return execFileSync("powershell.exe", ["-NoProfile", "-Command", command], {
    encoding: "utf8",
    timeout: 30_000,
  }).trim();
}

function windowTitles(): string[] {
  const text = runPowerShell(
    "Get-Process | Where-Object { $_.MainWindowTitle } | ForEach-Object { $_.MainWindowTitle }",
  );
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

/** Ask the real host for its Gravity status, which never contains a publisher key. */
async function gravitySettings(): Promise<Record<string, unknown>> {
  await browser.execute(() => {
    const holder = window as unknown as {
      __rigelSettings?: Record<string, unknown>;
      __TAURI_INTERNALS__?: {
        invoke: (command: string) => Promise<Record<string, unknown>>;
      };
    };
    holder.__rigelSettings = { pending: true };
    const internals = holder.__TAURI_INTERNALS__;
    if (!internals) {
      holder.__rigelSettings = { error: "no Tauri internals" };
      return;
    }
    internals
      .invoke("gravity_settings")
      .then((value) => {
        holder.__rigelSettings = value;
      })
      .catch((error) => {
        holder.__rigelSettings = { error: String(error) };
      });
  });

  let settings: Record<string, unknown> | undefined;
  await browser.waitUntil(
    async () => {
      settings = (await browser.execute(
        () => (window as unknown as { __rigelSettings?: Record<string, unknown> }).__rigelSettings,
      )) as Record<string, unknown> | undefined;
      return Boolean(settings && !settings.pending);
    },
    { timeout: 10_000, interval: 200, timeoutMsg: "gravity_settings did not answer" },
  );
  return settings as Record<string, unknown>;
}

describe("the sponsored card in the built window", () => {
  it("renders from the shared broker", async () => {
    await (await $('nav[aria-label="Primary"]')).waitForDisplayed({ timeout: 40_000 });

    // One scripted turn gives Home an edition to set the card beside.
    await startSession();
    await sendPrompt("Make the approved change.");
    await approveEveryCard();
    await waitForText("Done.");
    await openRail("Home");

    const card = await $('section[aria-label="Sponsored"]');
    await card.waitForDisplayed({ timeout: 60_000 });
    await expect(card).toHaveText(/advertisement/i);
    await expect(card).toHaveText(/test ad/i);

    // The window must be asking the shared broker, not carrying a publisher key.
    const settings = await gravitySettings();
    expect(settings).toMatchObject({
      enabled: true,
      test_ads: true,
      placement_id: "rigel-sponsor",
      endpoint: "https://rigel-25128.web.app/api/gravity-ad",
    });
    expect(settings).not.toHaveProperty("key_present");

    // WebDriver is attached to the real Tauri window; this is its app surface, including the card.
    await browser.saveScreenshot(path.resolve("docs/qa/rigel-gravity.png"));

    // Opening the ad uses the system default browser. Keep that opt-in so a normal suite run cannot
    // disturb the person at the keyboard; the product-level click path has its own Rust test.
    if (process.env.RIGEL_E2E_SPONSOR_CLICK !== "1") {
      console.log("sponsor click-through skipped; set RIGEL_E2E_SPONSOR_CLICK=1 to open it");
      return;
    }

    await (await card.$("button")).click();
    await browser.waitUntil(
      async () => !/opening/i.test(await card.getText()),
      { timeout: 20_000, interval: 250, timeoutMsg: "the sponsor click never finished" },
    );
    await expect(card).not.toHaveText(/could not be opened/i);

    let opened = "";
    await browser.waitUntil(
      async () => {
        opened = windowTitles().find((title) => /gravity|trygravity/i.test(title)) ?? "";
        return Boolean(opened);
      },
      {
        timeout: 30_000,
        interval: 1_000,
        timeoutMsg: "clicking the sponsored card left no Gravity browser window",
      },
    );
    console.log(`sponsor browser window: ${opened}`);
  });
});

import { $, browser, expect } from "@wdio/globals";
import path from "node:path";
import {
  approvalCard,
  approveEveryCard,
  openRail,
  sendPrompt,
  startSession,
  waitForText,
} from "../lib/app";

/**
 * The roadmap's window evidence: the same real turn the window spec drives, saved as PNGs in the
 * repo. The session shot is taken while the approval card is up, so it shows the coordinator's
 * own question; the diff shot shows the patch the edit produced.
 */
describe("the built window, captured", () => {
  it("saves the session and diff surfaces", async () => {
    await (await $('nav[aria-label="Primary"]')).waitForDisplayed({ timeout: 40_000 });
    await waitForText("e2e-project");

    await startSession();
    await sendPrompt("Make the approved change.");

    const card = await approvalCard();
    await card.waitForDisplayed({ timeout: 30_000 });
    await browser.saveScreenshot(path.resolve("docs/qa/rigel-window.png"));

    await approveEveryCard();
    await waitForText("Done.");
    await openRail("Diff");
    await waitForText("README.md");
    await expect($("body")).toHaveText(expect.stringContaining("Approved change."));
    await browser.saveScreenshot(path.resolve("docs/qa/rigel-window-diff.png"));
  });
});

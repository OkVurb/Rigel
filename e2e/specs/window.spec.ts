import { $, expect } from "@wdio/globals";
import {
  approvalCard,
  approveEveryCard,
  openRail,
  sendPrompt,
  startSession,
  waitForText,
} from "../lib/app";

/**
 * Item 1's first spec: the real app, a real turn through the coordinator, a real approval and the
 * patch the edit produced. No preview rows and no stubbed commands.
 */
describe("the built window", () => {
  it("opens the prepared project", async () => {
    // The rail only exists in the main window; the tray panel has no nav.
    await (await $('nav[aria-label="Primary"]')).waitForDisplayed({ timeout: 40_000 });
    await waitForText("e2e-project");
  });

  it("lands a scripted turn as an approval, then an applied patch", async () => {
    await startSession();
    await sendPrompt("Make the approved change.");

    const card = await approvalCard();
    await card.waitForDisplayed({ timeout: 30_000 });
    await approveEveryCard();

    await waitForText("Done.");
    await openRail("Diff");
    await waitForText("README.md");
    await expect($("body")).toHaveText(expect.stringContaining("Approved change."));
  });
});

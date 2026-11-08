import { browser, $, expect } from "@wdio/globals";
import {
  approvalCard,
  approveEveryCard,
  bodyText,
  sendPrompt,
  startSession,
  waitForText,
} from "../lib/app";

const cardShape = () =>
  browser.execute(() => {
    const card = document.querySelector('section[aria-label="Approval required"]');
    if (!card) return null;
    return {
      order: Array.from(card.querySelectorAll("[data-order]")).map((section) =>
        section.getAttribute("data-order"),
      ),
      primaries: Array.from(card.querySelectorAll('[data-primary="true"]')).map((button) =>
        (button.textContent ?? "").trim(),
      ),
      unknown: card.querySelectorAll('[data-testid="approval-cannot-determine"]').length,
      details: card.querySelectorAll("details").length,
      snapshot: (card.querySelector('[data-order="snapshot"]')?.textContent ?? "").trim(),
    };
  });

/**
 * Roadmap 135: approval card v2.
 *
 * The card has to be read in one order, offer exactly one primary action, keep an unknown blast
 * radius visible, and let a keyboard user refuse without reaching for the mouse.
 */
describe("the approval card", () => {
  it("reads command, blast radius, scope, snapshot and denies from the keyboard", async () => {
    await startSession();
    await sendPrompt("Run the destructive command from the script.");

    const card = await approvalCard();
    await card.waitForDisplayed({ timeout: 30_000 });

    const shape = (await cardShape()) as {
      order: string[];
      primaries: string[];
      unknown: number;
      details: number;
      snapshot: string;
    } | null;
    expect(shape).not.toBeNull();
    if (!shape) throw new Error("the approval card disappeared before it could be read");

    expect(shape.order).toEqual(["command", "blast", "scope", "snapshot"]);
    expect(shape.primaries).toEqual(["Allow once"]);
    // `cannot_determine` is rendered, never collapsed behind a disclosure.
    expect(shape.unknown).toBeGreaterThan(0);
    expect(shape.details).toBe(0);
    expect(shape.snapshot).toContain("snapshot state:");

    // Tab from the primary: session, project, always, then Reject.
    await browser.execute(() => {
      document
        .querySelector<HTMLButtonElement>(
          'section[aria-label="Approval required"] [data-primary="true"]',
        )
        ?.focus();
    });
    const stops: string[] = [];
    for (let step = 0; step < 4; step += 1) {
      await browser.keys(["Tab"]);
      stops.push(
        (await browser.execute(
          () => document.activeElement?.textContent?.trim() ?? "",
        )) as string,
      );
    }
    expect(stops).toEqual(["This session", "This project", "Always", "Reject"]);

    await browser.keys(["Enter"]);
    await browser.waitUntil(async () => !(await bodyText()).includes("Approval required"), {
      timeout: 20_000,
      timeoutMsg: "the card stayed after Reject",
    });
    // The refusal is recorded, and the turn carries on to its closing step.
    expect(await bodyText()).toMatch(/denied|refused|skipped/i);
    await approveEveryCard();
    await waitForText("Done.");
  });
});

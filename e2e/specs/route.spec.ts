import { browser, $, expect } from "@wdio/globals";
import {
  approvalCard,
  approveEveryCard,
  openRail,
  sendPrompt,
  startSession,
  waitForText,
} from "../lib/app";

/**
 * Roadmap 13: the route inspector and `turn_send` must report the same RouteExplanation.
 *
 * The turn stores what `turn_send` returned; `route_explain` is the command the inspector reads.
 * Feeding the turn's own requested pair back through `route_explain` and comparing field for
 * field is the equality the roadmap asks for. The inspector's rendered rows are then checked
 * against that same answer, so the panel cannot drift from the resolver either.
 */
type Route = { provider: string; model: string };
type Explanation = {
  saved: Route;
  requested: Route | null;
  resolved: Route;
  reason: string;
  mismatch: string | null;
};

describe("the route inspector", () => {
  it("reports the same explanation the turn used", async () => {
    await startSession();
    await sendPrompt("Route check.");
    const card = await approvalCard();
    await card.waitForDisplayed({ timeout: 30_000 });
    await approveEveryCard();
    await waitForText("Done.");

    const turnRoute = (await browser.execute(
      () =>
        (window as unknown as { __rigelLastTurnRoute?: Explanation }).__rigelLastTurnRoute ?? null,
    )) as Explanation | null;
    expect(turnRoute).not.toBeNull();
    expect(turnRoute && turnRoute.saved.provider.length).toBeGreaterThan(0);

    const explain = (await browser.execute(
      (provider: string | null, model: string | null) =>
        (
          window as unknown as {
            __TAURI_INTERNALS__: {
              invoke: (command: string, args: unknown) => Promise<Explanation>;
            };
          }
        ).__TAURI_INTERNALS__.invoke("route_explain", { provider, model }),
      turnRoute?.requested?.provider ?? null,
      turnRoute?.requested?.model ?? null,
    )) as Explanation;

    expect(explain).toEqual(turnRoute);
  });

  it("shows selected, requested, resolved, reason and the mismatch sentence", async () => {
    await openRail("Settings");
    const inspector = await $('section[aria-label="Route inspector"]');
    await inspector.waitForDisplayed({ timeout: 20_000 });

    const saved = await (await inspector.$('[data-testid="route-selected"]')).getText();
    expect(saved).toContain("/");
    const [savedProvider] = saved.split(" / ");

    await (await inspector.$('input[aria-label="Requested provider"]')).setValue(savedProvider);
    await (await inspector.$('input[aria-label="Requested model"]')).setValue("e2e/probe-model");

    await browser.waitUntil(
      async () =>
        (await (await inspector.$('[data-testid="route-reason"]')).getText()) === "picker",
      { timeout: 10_000, timeoutMsg: "the inspector did not resolve the previewed pair" },
    );
    expect(await (await inspector.$('[data-testid="route-resolved"]')).getText()).toBe(
      `${savedProvider} / e2e/probe-model`,
    );
    expect(await (await inspector.$('[data-testid="route-requested"]')).getText()).toContain(
      savedProvider,
    );
    expect(await (await inspector.$('[data-testid="route-mismatch"]')).getText()).toContain(
      "asked for",
    );
  });
});

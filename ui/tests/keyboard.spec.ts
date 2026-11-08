import { expect, test, type Page } from "@playwright/test";

/**
 * Roadmap 128: the keyboard-only exit test.
 *
 * Open project, send, approve and diff with no pointer input at all. Every Tab stop must show a
 * visible focus indication; a control that cannot be reached is a failure, not a skip.
 *
 * The shell signals focus in more than one way: rail buttons paint an outline, text fields move
 * their border, and pane handles draw a ring on `::after`. So each stop is compared with the same
 * element's own unfocused paint — baseline captured before it is ever focused — and any change in
 * outline, shadow, border or colour counts as the visible ring.
 */
const FOCUSABLE = "a[href],button,input,textarea,select,[tabindex],.pane-handle";

type Target = { kind: "label" | "placeholder" | "text"; value: string };

const refreshBaseline = (page: Page) =>
  page.evaluate((selector) => {
    const read = (element: Element, pseudo?: string) => {
      const style = getComputedStyle(element, pseudo);
      return [
        style.outlineStyle,
        style.outlineWidth,
        style.outlineColor,
        style.outlineOffset,
        style.boxShadow,
        style.borderColor,
        style.backgroundColor,
        style.color,
      ].join(";");
    };
    const scope = window as unknown as { __rigelFocusBaseline?: Map<Element, string[]> };
    const store = scope.__rigelFocusBaseline ?? new Map<Element, string[]>();
    scope.__rigelFocusBaseline = store;
    const active = document.activeElement;
    document.querySelectorAll(selector).forEach((element) => {
      if (store.has(element) || element === active) return;
      store.set(element, [read(element), read(element, "::after"), read(element, "::before")]);
    });
  }, FOCUSABLE);

const ring = (page: Page) =>
  page.evaluate(() => {
    const read = (element: Element, pseudo?: string) => {
      const style = getComputedStyle(element, pseudo);
      return [
        style.outlineStyle,
        style.outlineWidth,
        style.outlineColor,
        style.outlineOffset,
        style.boxShadow,
        style.borderColor,
        style.backgroundColor,
        style.color,
      ].join(";");
    };
    const element = document.activeElement as HTMLElement | null;
    if (!element || element === document.body) return null;
    const scope = window as unknown as { __rigelFocusBaseline?: Map<Element, string[]> };
    const baseline = scope.__rigelFocusBaseline?.get(element);
    const current = [read(element), read(element, "::after"), read(element, "::before")];
    const changed = baseline ? current.some((value, index) => value !== baseline[index]) : false;
    const style = getComputedStyle(element);
    const ownRing =
      (style.outlineStyle !== "none" && parseFloat(style.outlineWidth) > 0) ||
      style.boxShadow !== "none";
    return {
      label:
        element.getAttribute("aria-label") ??
        element.getAttribute("placeholder") ??
        (element.textContent ?? "").trim().slice(0, 40),
      ring: changed || ownRing,
    };
  });

type Stop = { label: string; ring: boolean };

/** Tab until the focused element matches `target`, recording every stop on the way. */
async function tabTo(page: Page, target: Target, limit = 80) {
  const stops: Stop[] = [];
  for (let step = 0; step < limit; step += 1) {
    await refreshBaseline(page);
    await page.keyboard.press("Tab");
    const focused = await ring(page);
    if (!focused) continue;
    stops.push(focused);
    const reached = await page.evaluate((wanted: Target) => {
      const element = document.activeElement as HTMLElement | null;
      if (!element) return false;
      if (wanted.kind === "label") return element.getAttribute("aria-label") === wanted.value;
      if (wanted.kind === "placeholder") {
        return (element.getAttribute("placeholder") ?? "").includes(wanted.value);
      }
      return (element.textContent ?? "").trim() === wanted.value;
    }, target);
    if (reached) return stops;
  }
  throw new Error(`Tab never reached ${target.value}; last stop was ${stops.at(-1)?.label ?? "none"}`);
}

const label = (value: string): Target => ({ kind: "label", value });
const placeholder = (value: string): Target => ({ kind: "placeholder", value });
const text = (value: string): Target => ({ kind: "text", value });

test("open, send, approve and diff with Tab and Enter only", async ({ page }) => {
  await page.goto("/");

  // Open project: the preview already has one open, so the keyboard path is the rail itself.
  const railStops = await tabTo(page, label("Session"));
  await page.keyboard.press("Enter");
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();

  // Send: the composer takes a draft, Enter opens the review card, and its send is a Tab away.
  const composerStops = await tabTo(page, placeholder("Ask the agent"));
  await page.keyboard.type("Keyboard-only turn");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("region", { name: "Prompt refinement" })).toBeVisible();
  const reviewStops = await tabTo(page, text("Send original"));
  await page.keyboard.press("Enter");

  // Approve: the sample approval is waiting on the session; its primary action is reachable.
  const approvalStops = await tabTo(page, text("Allow once"), 120);
  await page.keyboard.press("Enter");
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Allow once" })).toHaveCount(0);

  // Diff: the last surface, still by keyboard.
  const diffStops = await tabTo(page, label("Diff"), 160);
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "One tree, several agents" })).toBeVisible();

  const stops = [...railStops, ...composerStops, ...reviewStops, ...approvalStops, ...diffStops];
  expect(stops.length).toBeGreaterThan(10);
  const ringless = stops.filter((stop) => !stop.ring).map((stop) => stop.label);
  expect(ringless.join(" | "), "these Tab stops show no focus indication").toBe("");
});

import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "@playwright/test";

/**
 * Roadmap 127: the eight-state audit.
 *
 * `docs/qa/eight-states.md` carries one row per control and one cell per state; this spec fails on
 * a cell that is neither answered (`yes`), impossible (`n/a`) nor owned (`gap (owner: …)`). The
 * live checks below then prove the states the browser preview can actually reach.
 */
const SURFACES = ["session", "approval", "picker", "settings"];
const STATES = ["default", "hover", "focus", "active", "disabled", "loading", "error", "success"];

const audit = readFileSync(path.join(process.cwd(), "..", "docs", "qa", "eight-states.md"), "utf8");

const rows = audit
  .split(/\r?\n/)
  .filter((line) => line.startsWith("| ") && !line.startsWith("| Surface") && !line.startsWith("|---"))
  .map((line) =>
    line
      .split("|")
      .slice(1, -1)
      .map((cell) => cell.trim()),
  );

test("every audited cell is answered, impossible, or owned", () => {
  expect(rows.length).toBeGreaterThan(20);
  for (const [surface, control, ...cells] of rows) {
    expect(SURFACES, `${surface} is not an audited surface`).toContain(surface);
    expect(control.length).toBeGreaterThan(0);
    expect(cells.length, `${surface} / ${control} must list eight states`).toBe(STATES.length);
    cells.forEach((cell, index) => {
      const where = `${surface} / ${control} / ${STATES[index]}`;
      if (cell === "yes" || cell === "n/a") return;
      expect(cell, `${where} is missing without an owner`).toMatch(/^gap \(owner: [a-z-]+\)$/);
    });
  }
  for (const surface of SURFACES) {
    const answered = rows.filter((row) => row[0] === surface && row.slice(2).includes("yes"));
    expect(answered.length, `${surface} has no answered state`).toBeGreaterThan(0);
  }
});

const focusedRing = (page: import("@playwright/test").Page) =>
  page.evaluate(() => {
    const element = document.activeElement as HTMLElement | null;
    if (!element || element === document.body) return null;
    const style = getComputedStyle(element);
    return style.outlineStyle !== "none" && parseFloat(style.outlineWidth) > 0;
  });

test("the reachable states are real, not just tabulated", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();

  // default: an empty composer keeps Send visible but disabled, and typing enables it.
  const send = page.getByRole("button", { name: "Send", exact: true });
  await expect(send).toBeVisible();
  await expect(send).toBeDisabled();
  await page.getByPlaceholder(/Ask the agent/).fill("State check");
  await expect(send).toBeEnabled();

  // focus: the first Tab stop paints a ring.
  await page.keyboard.press("Tab");
  expect(await focusedRing(page)).toBe(true);

  // hover and active: the rail's paint changes while hovered and while pressed.
  const rail = page.getByRole("button", { name: "Diff", exact: true });
  const paint = () =>
    rail.evaluate((element) => {
      const style = getComputedStyle(element);
      return [style.backgroundColor, style.color, style.transform, style.filter].join("|");
    });
  const resting = await paint();
  await rail.hover();
  // The rail paints its hover fill through a 120 ms transition, so poll instead of sampling once.
  await expect.poll(paint).not.toBe(resting);

  const box = await rail.boundingBox();
  if (!box) throw new Error("the Diff rail item has no box");
  await page.mouse.move(0, 0);
  await expect.poll(paint).toBe(resting);
  const hovered = await paint();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  const pressed = await paint();
  await page.mouse.up();
  expect(pressed).not.toBe(hovered);

  // That press was a real click, so the session surface has to be reopened for the send step.
  await page.getByRole("button", { name: "Session", exact: true }).click();
  // Leaving the surface dropped the draft, so type again before asserting the send path.
  await page.getByPlaceholder(/Ask the agent/).fill("State check");
  await expect(send).toBeEnabled();

  // loading: the review card exposes its in-flight state to assistive tech.
  await send.click();
  const refine = page.getByRole("region", { name: "Prompt refinement" });
  await expect(refine).toBeVisible();
  await expect(refine).toHaveAttribute("aria-busy", /true|false/);
  await page.getByRole("button", { name: "Send original" }).click();

  // success: the diff surface reports provenance through a status region.
  await page.getByRole("button", { name: "Diff", exact: true }).click();
  await expect(page.getByRole("status").first()).toBeVisible();
  await expect(page.getByRole("heading", { name: "One tree, several agents" })).toBeVisible();

  // settings: the route inspector is the settings control with a resolver-backed contract. Its
  // error state is exercised where it can actually fail: the refusal alert is covered by
  // e2e/specs/approval.spec.ts against the real window.
  await page.getByRole("button", { name: "Settings" }).click();
  const inspector = page.getByRole("region", { name: "Route inspector" });
  await expect(inspector).toBeVisible();
  await expect(inspector.getByLabel("Requested provider")).toBeVisible();
  await expect(inspector.getByLabel("Requested model")).toBeVisible();
});

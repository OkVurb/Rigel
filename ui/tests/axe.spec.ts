import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

/**
 * Roadmap 140: axe runs in CI on the two flows the audit names, and the target-size rule is
 * measured rather than asserted in prose. A new violation fails this file; a known exception
 * belongs in `docs/qa/accessibility.md` with an owner and a date.
 */
const scan = (page: import("@playwright/test").Page) =>
  new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
    .analyze();

const summarize = (results: Awaited<ReturnType<typeof scan>>) =>
  results.violations.map(
    (violation) =>
      `${violation.id}: ${violation.nodes.map((node) => node.target.join(" ")).join(", ")}`,
  );

test("the session surface has no axe violations", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
  expect(summarize(await scan(page))).toEqual([]);
});

test("the approval surface has no axe violations", async ({ page }) => {
  await page.goto("/");
  const rail = page.locator("nav.rail");
  await rail.getByRole("button", { name: /^Attention/ }).click();
  await page.getByRole("button", { name: /cargo test --workspace/ }).click();
  await expect(page.getByTestId("approval-dry-run")).toBeVisible();
  expect(summarize(await scan(page))).toEqual([]);
});

test("dense icon controls still expose a 20px target", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();
  // The transcript is intentionally dense. Inline text disclosures and controls that axe's
  // WCAG 2.2 spacing rule exempts are not measured here; icon-only controls are.
  const undersized = await page.evaluate(() =>
    [...document.querySelectorAll("button[aria-label]")]
      .filter((element) => {
        const rect = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        return (
          rect.width > 0 &&
          rect.height > 0 &&
          style.visibility !== "hidden" &&
          style.display !== "none" &&
          !(element as HTMLButtonElement).disabled &&
          (rect.width < 20 || rect.height < 20)
        );
      })
      .map((element) => {
        const rect = element.getBoundingClientRect();
        const label = element.getAttribute("aria-label") ?? "unlabelled";
        return `${label.slice(0, 40)} (${Math.round(rect.width)}x${Math.round(rect.height)})`;
      }),
  );
  expect(undersized).toEqual([]);
});

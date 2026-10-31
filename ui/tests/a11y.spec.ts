import { expect, test } from "@playwright/test";

// D158: the first Tab stop is the skip link, a turn is a labelled article, and the OS's
// "more contrast" preference raises the metadata without touching the layout.
test("the first tab stop skips the chrome and turns are labelled", async ({ page }) => {
  await page.goto("/");

  await page.keyboard.press("Tab");
  const skip = page.getByRole("link", { name: "Skip to content" });
  await expect(skip).toBeFocused();
  await skip.press("Enter");
  await expect(page.locator("#main")).toBeFocused();

  await page.getByRole("button", { name: "Session" }).click();
  const turn = page.locator("article.turn").first();
  await expect(turn).toHaveAttribute("aria-label", /^Turn \d+, (completed|running|failed)/);
});

test("more contrast raises the metadata colour", async ({ page }) => {
  await page.goto("/");
  const faint = () =>
    page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--faint").trim());
  const before = await faint();

  await page.emulateMedia({ contrast: "more" });
  await expect.poll(faint).not.toBe(before);
  await expect(page.locator(".shell")).toBeVisible();
});

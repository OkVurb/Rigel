import { expect, test } from "@playwright/test";

// Home is the window's landing surface: the day's work set as the Rigel Paper. It reads the
// sample editions the browser preview ships, so the masthead, a lead story and the rack are all
// reachable without a Tauri host.
test("the window opens on the Rigel Paper", async ({ page }) => {
  await page.goto("/");

  await expect(page.locator(".shell")).toHaveClass(/home/);
  await expect(page.getByRole("heading", { name: "The Rigel Paper" })).toBeVisible();
  await expect(page.locator(".paper-headline")).toBeVisible();
  await expect(page.locator(".paper-issue.on")).toHaveAttribute("aria-current", "page");

  const rack = page.getByRole("navigation", { name: "Editions by day" });
  await expect(rack.locator("button").first()).toBeVisible();
  expect(await rack.locator("button").count()).toBeGreaterThan(1);
});

test("an earlier issue opens and the lead changes with it", async ({ page }) => {
  await page.goto("/");
  const rack = page.getByRole("navigation", { name: "Editions by day" });
  const issue = rack.locator("button").nth(1);
  const headline = (await issue.locator(".paper-issue-head").innerText()).trim();

  await issue.click();
  await expect(issue).toHaveAttribute("aria-current", "page");
  await expect(page.locator(".paper-headline")).toHaveText(headline);

  await page.getByRole("button", { name: "Latest" }).click();
  await expect(rack.locator("button").first()).toHaveAttribute("aria-current", "page");
});

test("a story opens the session behind it", async ({ page }) => {
  await page.goto("/");

  await page.getByRole("button", { name: "Read the session →" }).first().click();
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
  await expect(page.getByText("Browser preview: sample transcript.")).toBeVisible();
});

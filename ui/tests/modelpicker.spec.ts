import { expect, test } from "@playwright/test";

// Q287: favourites and recents are browser-local conveniences, so the preview can exercise the
// same storage paths the desktop window uses without a Rust host.
test("a model row names its resolved pair and never guesses an unknown price", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();
  await page.locator(".model-button").click();

  const unknown = page.locator(".model-row", { has: page.locator(".tag.model-cost.unknown") }).first();
  await expect(unknown).toBeVisible();
  const id = await unknown.locator(".mono").innerText();
  const provider = await unknown.locator(".model-tags .tag").last().innerText();
  expect(await unknown.getAttribute("title")).toContain(`${provider} / ${id}`);
  await expect(unknown.locator(".tag.model-cost.unknown")).toHaveText("unknown");
  await expect(unknown.locator(".tag.model-cost.unknown")).toHaveAttribute(
    "title",
    /does not guess/,
  );
});

test("the model picker remembers favourites and recents", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();
  await page.locator(".model-button").click();

  const list = page.getByRole("listbox", { name: "Models" });
  await expect(list).toBeVisible();
  const first = page.locator(".model-row-wrap").first();
  const star = first.getByRole("button", { name: /favourites$/ });
  await star.click();
  await expect(star).toHaveAttribute("aria-pressed", "true");

  await page.reload();
  await page.getByRole("button", { name: "Session", exact: true }).click();
  await page.locator(".model-button").click();
  await expect(
    page.locator(".model-row-wrap").first().getByRole("button", { name: /favourites$/ }),
  ).toHaveAttribute("aria-pressed", "true");

  const chosen = page.locator(".model-row-wrap").nth(1).locator(".model-row");
  const chosenId = await chosen.locator(".mono").innerText();
  await chosen.click();

  await page.reload();
  await page.getByRole("button", { name: "Session", exact: true }).click();
  await page.locator(".model-button").click();
  const recent = page.locator(".model-row-wrap", { hasText: chosenId }).first();
  await expect(recent.locator(".tag.model-recent")).toBeVisible();
});

// Items 15/18: the launch probe labels every credentialed door, and each row carries the facts
// this build knows — with a missing fact rendered as "unknown", never as "false".
test("the picker shows door health and an honest capability row", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();
  await page.locator(".model-button").click();

  const health = page.getByRole("status", { name: "Provider health" });
  await expect(health).toBeVisible();
  await expect(health).toContainText("openrouter");
  await expect(health).toContainText("ready");
  await expect(health).toContainText("627 models");
  await expect(health).toContainText("gemini");
  await expect(health).toContainText("auth");

  const caps = page.locator(".model-caps").first();
  await expect(caps).toBeVisible();
  await expect(caps).toContainText("tools unknown");
  await expect(caps).toContainText("ctx unknown");
  await expect(caps).not.toContainText("false");
  await expect(caps).toHaveAttribute("title", /2026-11-05/);

  const known = page
    .locator(".model-row-wrap", { hasText: "anthropic/claude-sonnet-4.5" })
    .locator(".model-caps")
    .first();
  await expect(known).toContainText("tools yes");
  await expect(known).toContainText("ctx 200k");
  await expect(known).toContainText("$15.00/M out");
});

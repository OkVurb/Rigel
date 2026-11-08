import { expect, test } from "@playwright/test";

// A selected door that returns 503 must be visible as a failed turn, with a same-door retry and
// a consent-only fallback. It must never look like an automatic provider switch.
test("a failed door shows the degraded banner and a consent-only fallback", async ({ page }) => {
  await page.addInitScript(() => {
    window.localStorage.setItem("rigel.preview.degraded", "1");
  });
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();

  const banner = page.locator(".degraded-banner");
  await expect(banner).toBeVisible();
  await expect(banner).toContainText("did not switch providers or models");
  await expect(banner).toContainText("provider returned 503");
  await expect(banner.getByRole("button", { name: "Retry this turn" })).toBeVisible();
  await expect(banner.locator(".degraded-offer")).toContainText("Consent-only fallback");

  const fallback = banner.locator(".degraded-offer button");
  await expect(fallback).toBeVisible();
  await fallback.click();
  await expect(page.locator(".degraded-banner")).toHaveCount(0);
});

import { expect, test } from "@playwright/test";

test("a crashed turn can be resumed, viewed read-only, or discarded", async ({ page }) => {
  await page.goto("/?recovery=1");
  await page.getByRole("button", { name: "Session", exact: true }).click();

  const offer = page.getByRole("region", { name: "Crash recovery" });
  await expect(offer).toBeVisible();
  await expect(offer.getByText("turn 3")).toBeVisible();
  await expect(offer.getByText("turn-preview-crashed")).toBeVisible();
  await expect(offer.getByRole("button", { name: "Resume the turn" })).toBeVisible();
  await expect(offer.getByRole("button", { name: "Read-only view" })).toBeVisible();
  await expect(offer.getByRole("button", { name: "Discard and tombstone" })).toBeVisible();

  await offer.getByRole("button", { name: "Read-only view" }).click();
  await expect(page.getByText("Read-only view. Sending is disabled")).toBeVisible();
  await expect(page.getByPlaceholder("Read-only transcript; sending is disabled")).toBeDisabled();
  await expect(page.getByRole("button", { name: "Send", exact: true })).toBeDisabled();
});

test("discarding the crashed turn removes the offer", async ({ page }) => {
  await page.goto("/?recovery=1");
  await page.getByRole("button", { name: "Session", exact: true }).click();
  await page
    .getByRole("region", { name: "Crash recovery" })
    .getByRole("button", { name: "Discard and tombstone" })
    .click();
  await expect(page.getByRole("region", { name: "Crash recovery" })).toBeHidden();
});

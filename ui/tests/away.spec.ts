import { expect, test } from "@playwright/test";

// D155: the card is built at focus from rows that changed since the window lost focus, it
// reports the approvals that are waiting now, and dismissing it is enough to keep it away.
test("a focus after work finished shows the away card once", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session" }).click();

  await page.evaluate(() => window.dispatchEvent(new Event("blur")));

  // Preview: a delivered prompt becomes a completed turn immediately.
  const composer = page.getByPlaceholder(/Ask the agent/);
  await composer.fill("Do one more thing while I am away");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await page.getByRole("button", { name: "Send original" }).click();
  await expect(composer).toHaveValue("");

  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  const card = page.locator(".away-card");
  await expect(card).toBeVisible();
  await expect(card).toContainText("1 turn finished");
  await expect(card).toContainText("1 approval waiting");

  await card.getByRole("button", { name: "Dismiss" }).click();
  await expect(card).toHaveCount(0);

  // Nothing changed between these two focuses, so the card stays dismissed.
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(page.locator(".away-card")).toHaveCount(0);
});

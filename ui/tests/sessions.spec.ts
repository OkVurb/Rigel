import { expect, test } from "@playwright/test";

// D148: the session list is an edit over the sample session, so the browser preview exercises
// the same handlers the desktop app uses. The core's persistence is covered by the Rust tests.
test("the session list filters, renames, pins and archives", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session" }).click();
  await page.getByRole("button", { name: "sessions" }).click();

  const row = page.locator(".session-row", { hasText: "Fix the flaky resume test" });
  await expect(row).toBeVisible();

  const filter = page.getByRole("textbox", { name: "Filter sessions" });
  await filter.fill("nothing matches");
  await expect(page.getByText("No session matches this filter.")).toBeVisible();
  await filter.fill("");

  await row.hover();
  await row.getByRole("button", { name: "Rename Fix the flaky resume test" }).click();
  await page.getByRole("textbox", { name: "Rename Fix the flaky resume test" }).fill("Release check");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "Release check" })).toBeVisible();

  const renamed = page.locator(".session-row", { hasText: "Release check" });
  await renamed.hover();
  await renamed.getByRole("button", { name: "☆" }).click();
  await expect(renamed.getByRole("button", { name: "★" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );

  await renamed.getByRole("button", { name: "Archive session" }).click();
  await expect(page.locator(".session-row", { hasText: "Release check" })).toHaveCount(0);

  await page.getByRole("button", { name: "archived", exact: true }).click();
  const restored = page.locator(".session-row", { hasText: "Release check" });
  await expect(restored).toBeVisible();
  await expect(restored).toContainText("archived");
});

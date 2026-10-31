import { expect, test } from "@playwright/test";

// D156: the panel reads the recorded chain back — hunk, patch, turn, tool and model — and is
// read-only. A line click starts the same panel at that line.
test("a diff line shows its recorded provenance", async ({ page }) => {
  await page.goto("/");
  await page
    .getByRole("navigation", { name: "Primary" })
    .getByRole("button", { name: "Diff", exact: true })
    .click();

  const hunk = page.locator(".hunk").first();
  await hunk.getByRole("button", { name: "where from" }).click();

  const panel = hunk.locator(".provenance");
  await expect(panel).toBeVisible();
  await expect(panel).toContainText("Where this came from");
  await expect(panel).toContainText("turn 2 · completed");
  await expect(panel).toContainText("edit_file · file_change");
  await expect(panel).toContainText("anthropic/claude-sonnet-4.5");

  await hunk.locator(".diff-line").first().click();
  await expect(panel).toContainText("new line 1");

  await panel.getByRole("button", { name: "close" }).click();
  await expect(panel).toHaveCount(0);
});

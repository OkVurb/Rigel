import { expect, test } from "@playwright/test";

// Q73: the approval card shows the tool's own preview, not a guess. The preview fixture carries
// the same `dry_run` shape the Rust WindowApprover puts in the pending detail.
test("a command card shows the tool's own dry-run preview", async ({ page }) => {
  await page.goto("/");

  const rail = page.locator("nav.rail");
  await rail.getByRole("button", { name: /^Attention/ }).click();
  await page.getByRole("button", { name: /cargo test --workspace/ }).click();

  const dryRun = page.getByTestId("approval-dry-run");
  await expect(dryRun).toBeVisible();
  await expect(dryRun).toContainText("git clean -ndx");
  await expect(dryRun).toContainText("Would remove src/tmp.txt");
});

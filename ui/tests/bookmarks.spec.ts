import { expect, test } from "@playwright/test";

// D150: the star is a toggle on both surfaces. Browser preview keeps the two lists in their
// own components; the shared per-session list is what the desktop database provides.
test("a transcript line and a diff hunk can be starred and cleared", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();

  // The transcript opens at the newest turn and an item can be taller than the viewport.
  // Pin the scroll container to the top first so the first item's star is not under the
  // composer; then hover the item's visible top edge.
  await page.locator(".transcripts").evaluate((element) => {
    element.scrollTop = 0;
  });
  const line = page.locator(".item-shell").first();
  await line.hover({ position: { x: 8, y: 8 } });
  await line.getByRole("button", { name: "Bookmark this line" }).click();
  await expect(line.getByRole("button", { name: "Remove bookmark" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Bookmarks" })).toBeVisible();
  await expect(page.getByText(/turn 1 · reasoning/)).toBeVisible();

  await page.getByRole("button", { name: /Remove bookmark turn 1/ }).click();
  await expect(page.getByText(/Star a line in the transcript/)).toBeVisible();

  await page
    .getByRole("navigation", { name: "Primary" })
    .getByRole("button", { name: "Diff", exact: true })
    .click();
  const hunk = page.locator(".hunk").first();
  await hunk.getByRole("button", { name: "Bookmark this hunk" }).click();
  await expect(hunk.getByRole("button", { name: "Remove hunk bookmark" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});

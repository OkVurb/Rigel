import { expect, test } from "@playwright/test";

// Q278: rebinding is a window preference, so the browser preview can exercise the same capture,
// storage and matching paths the desktop window uses.
test("a shortcut can be rebound and survives a reload", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("option", { name: /Keyboard/ }).click();

  const row = page.locator(".shortcut-row", { hasText: "Command palette" });
  await row.getByRole("button", { name: "Change" }).click();
  await page.keyboard.press("Control+Shift+K");
  await expect(row.locator("kbd").first()).toHaveText("Ctrl Shift K");

  await page.reload();
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("option", { name: /Keyboard/ }).click();
  await expect(
    page.locator(".shortcut-row", { hasText: "Command palette" }).locator("kbd").first(),
  ).toHaveText("Ctrl Shift K");

  await page.keyboard.press("Control+Shift+K");
  await expect(
    page.getByRole("dialog", { name: "Search actions, skills, sessions, turns and patches" }),
  ).toBeVisible();
});

test("a conflict is refused, one action resets alone, and the palette shows the binding", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("option", { name: /Keyboard/ }).click();

  const palette = page.locator(".shortcut-row", { hasText: "Command palette" });
  const search = page.locator(".shortcut-row", { hasText: "Search this surface" });

  // A binding already used by another action is refused before it is stored.
  await palette.getByRole("button", { name: "Change" }).click();
  await page.keyboard.press("Control+Shift+F");
  await expect(page.getByText("already used by Focus mode")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(palette.locator("kbd").first()).toHaveText("Ctrl K");

  // The palette footer shows the binding that is current, not a hard-coded default.
  await palette.getByRole("button", { name: "Change" }).click();
  await page.keyboard.press("Control+Shift+K");
  await expect(palette.locator("kbd").first()).toHaveText("Ctrl Shift K");
  const dialog = page.getByRole("dialog", {
    name: "Search actions, skills, sessions, turns and patches",
  });
  await page.keyboard.press("Control+Shift+K");
  await expect(dialog).toBeVisible();
  await expect(page.getByRole("textbox", { name: "Search Rigel" })).toBeFocused();
  await expect(dialog).toContainText("Ctrl Shift K");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();

  // Reset one row: the other rebound action keeps its binding.
  await search.getByRole("button", { name: "Change" }).click();
  await page.keyboard.press("Control+Shift+L");
  await expect(search.locator("kbd").first()).toHaveText("Ctrl Shift L");
  await palette.getByRole("button", { name: "Reset" }).click();
  await expect(palette.locator("kbd").first()).toHaveText("Ctrl K");
  await expect(search.locator("kbd").first()).toHaveText("Ctrl Shift L");

  await page.keyboard.press("Control+K");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Ctrl K");
});

test("the saved search shortcut focuses the settings filter", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("option", { name: /Keyboard/ }).click();

  const row = page.locator(".shortcut-row", { hasText: "Search this surface" });
  await row.getByRole("button", { name: "Change" }).click();
  await page.keyboard.press("Control+Shift+L");
  await expect(row.locator("kbd").first()).toHaveText("Ctrl Shift L");

  await page.keyboard.press("Control+Shift+L");
  await expect(page.getByRole("textbox", { name: "Search settings" })).toBeFocused();
});

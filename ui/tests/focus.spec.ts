import { expect, test } from "@playwright/test";

// Q289: focus hides chrome, but the way back must survive the narrowest supported window and
// the palette must keep advertising the same binding.
test("focus mode keeps its exit visible at every width and names it in the palette", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.locator(".focus-chip")).toBeVisible();

  // The palette advertises the same binding that the chip executes. Check it on a clean surface,
  // before the width loop leaves a click target focused.
  await page.keyboard.press("Control+K");
  const dialog = page.getByRole("dialog", {
    name: "Search actions, skills, sessions, turns and patches",
  });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Focus mode");
  await expect(dialog).toContainText("Ctrl Shift F");
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();

  for (const width of [320, 375, 414, 768]) {
    await page.setViewportSize({ width, height: 900 });
    const chip = page.locator(".focus-chip");
    await expect(chip).toBeVisible();
    await chip.click();
    await expect(chip).toHaveAttribute("aria-pressed", "true");
    await expect(chip).toContainText("Exit focus");

    const box = await chip.boundingBox();
    expect(box, `${width}px exit box`).not.toBeNull();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(width);

    await expect(page.locator(".rail")).toBeHidden();
    await expect(page.locator(".list-pane").first()).toBeHidden();

    await chip.click();
    await expect(chip).toHaveAttribute("aria-pressed", "false");
  }

});

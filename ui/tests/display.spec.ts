import { expect, test } from "@playwright/test";

// D159: scale and density are written to <html> and applied before the save returns; the pane
// handle nudges the width and keeps the shell exactly the size of the window.
test("scale, density and a pane width are applied from the window", async ({ page }) => {
  await page.goto("/");
  await page
    .getByRole("navigation", { name: "Primary" })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  await page.getByRole("option", { name: /^Appearance/ }).click();

  const scale = page.getByRole("group", { name: "Window scale" });
  await scale.getByRole("button", { name: "125%" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-scale", "125");

  const density = page.getByRole("group", { name: "Row density" });
  await density.getByRole("button", { name: "Compact" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-density", "compact");

  // Zooming #root must not push the shell past the window.
  // getBoundingClientRect reports the layout box, before zoom: at 125% the shell must be
  // 1280/1.25 wide so it paints at exactly the window's width.
  const box = await page.evaluate(() => {
    const shell = document.querySelector(".shell")?.getBoundingClientRect();
    const zoom = Number(document.documentElement.dataset.scale ?? 100) / 100;
    return shell
      ? {
          width: shell.width * zoom,
          height: shell.height * zoom,
          inner: [window.innerWidth, window.innerHeight],
        }
      : null;
  });
  expect(box).not.toBeNull();
  expect(Math.abs((box?.width ?? 0) - (box?.inner[0] ?? 0)) < 2, JSON.stringify(box)).toBe(true);
  expect(Math.abs((box?.height ?? 0) - (box?.inner[1] ?? 0)) < 2, JSON.stringify(box)).toBe(true);

  await page
    .getByRole("navigation", { name: "Primary" })
    .getByRole("button", { name: "Session", exact: true })
    .click();
  const handle = page.getByRole("separator", { name: "Resize the list" });
  await handle.focus();
  await page.keyboard.press("ArrowRight");
  await expect
    .poll(() =>
      page.evaluate(() =>
        getComputedStyle(document.querySelector(".shell")!).getPropertyValue("--list-w").trim(),
      ),
    )
    .toBe("276px");
});

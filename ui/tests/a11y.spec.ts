import { expect, test } from "@playwright/test";

// D158: the first Tab stop is the skip link, a turn is a labelled article, and the OS's
// "more contrast" preference raises the metadata without touching the layout.
test("the first tab stop skips the chrome and turns are labelled", async ({ page }) => {
  await page.goto("/");

  await page.keyboard.press("Tab");
  const skip = page.getByRole("link", { name: "Skip to content" });
  await expect(skip).toBeFocused();
  await skip.press("Enter");
  await expect(page.locator("#main")).toBeFocused();

  await page.getByRole("button", { name: "Session", exact: true }).click();
  const turn = page.locator("article.turn").first();
  await expect(turn).toHaveAttribute("aria-label", /^Turn \d+, (completed|running|failed)/);
});

test("both palettes clear AA and the terminal recolours without a reload", async ({ page }) => {
  await page.goto("/");

  const ratios = await page.evaluate(() => {
    const channel = (value: number) => {
      const v = value / 255;
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    };
    const luminance = (hex: string) => {
      const raw = hex.trim().replace("#", "");
      const n = Number.parseInt(raw, 16);
      return (
        0.2126 * channel((n >> 16) & 255) +
        0.7152 * channel((n >> 8) & 255) +
        0.0722 * channel(n & 255)
      );
    };
    const contrast = (foreground: string, background: string) => {
      const a = luminance(foreground);
      const b = luminance(background);
      return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
    };
    const measure = (theme: "dark" | "light") => {
      document.documentElement.dataset.theme = theme;
      const styles = getComputedStyle(document.documentElement);
      const bg = styles.getPropertyValue("--bg");
      return {
        text: contrast(styles.getPropertyValue("--text"), bg),
        faint: contrast(styles.getPropertyValue("--faint"), bg),
      };
    };
    const dark = measure("dark");
    const light = measure("light");
    document.documentElement.dataset.theme = "dark";
    return { dark, light };
  });

  for (const [palette, pair] of Object.entries(ratios)) {
    expect(pair.text, `${palette} text`).toBeGreaterThanOrEqual(4.5);
    expect(pair.faint, `${palette} faint`).toBeGreaterThanOrEqual(4.5);
  }

  await page.getByRole("button", { name: "Terminal" }).click();
  const host = page.locator(".terminal-host");
  await expect(host).toHaveAttribute("data-terminal-theme", "dark");
  await expect(host).toHaveAttribute("data-terminal-background", "#0b0c0e");

  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("option", { name: /^Appearance/ }).click();
  await page
    .getByRole("group", { name: "Theme" })
    .getByRole("button", { name: /^Light/ })
    .click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await expect(host).toHaveAttribute("data-terminal-theme", "light");
  await expect(host).toHaveAttribute("data-terminal-background", "#ffffff");

  await page.getByRole("button", { name: "Terminal" }).click();
  await expect(page.locator(".terminal-frame")).toBeVisible();
});

test("more contrast raises the metadata colour", async ({ page }) => {
  await page.goto("/");
  const faint = () =>
    page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--faint").trim());
  const before = await faint();

  await page.emulateMedia({ contrast: "more" });
  await expect.poll(faint).not.toBe(before);
  await expect(page.locator(".shell")).toBeVisible();
});

// Q129: both OS preferences reach the document, the measured metadata pair clears AAA, and
// spatial motion is collapsed rather than left running at full duration.
test("reduced motion and more contrast are asserted together, with the ratio recorded", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce", contrast: "more" });
  await page.goto("/");

  const measured = await page.evaluate(() => {
    const channel = (value: number) => {
      const v = value / 255;
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    };
    const luminance = (hex: string) => {
      const raw = hex.trim().replace("#", "");
      const n = Number.parseInt(raw, 16);
      return (
        0.2126 * channel((n >> 16) & 255) +
        0.7152 * channel((n >> 8) & 255) +
        0.0722 * channel(n & 255)
      );
    };
    const styles = getComputedStyle(document.documentElement);
    const bg = styles.getPropertyValue("--bg");
    const faint = styles.getPropertyValue("--faint");
    const a = luminance(faint);
    const b = luminance(bg);
    const ratio = (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
    const paint = getComputedStyle(document.querySelector(".focus-chip")! );
    const seconds = (value: string) =>
      value
        .split(",")
        .map((part) => Number.parseFloat(part))
        .filter(Number.isFinite);
    return {
      reduced: matchMedia("(prefers-reduced-motion: reduce)").matches,
      moreContrast: matchMedia("(prefers-contrast: more)").matches,
      ratio,
      faint,
      bg,
      motion: Math.max(...seconds(paint.transitionDuration), ...seconds(paint.animationDuration)),
    };
  });

  console.log(
    `measured contrast ${measured.ratio.toFixed(2)}:1 for ${measured.faint} on ${measured.bg}; ` +
      `longest motion ${measured.motion}s`,
  );
  expect(measured.reduced).toBe(true);
  expect(measured.moreContrast).toBe(true);
  expect(measured.ratio).toBeGreaterThanOrEqual(7);
  expect(measured.motion).toBeLessThan(0.01);
});

import { expect, test } from "@playwright/test";

// Q133: font and scrollback change on the open tab through the one settings save path; the shell
// surface itself stays mounted across navigation and its configured program follows the setting.
test("terminal settings apply live and the shell survives a tab switch", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Terminal", exact: true }).click();

  const host = page.locator(".terminal-host");
  await expect(host).toBeVisible();
  await expect(host).toHaveAttribute("data-terminal-font-size", "13");
  await expect(host).toHaveAttribute("data-terminal-scrollback", "8000");
  await expect(host).toHaveAttribute("data-terminal-program", "OS default");
  await expect(host).toHaveAttribute("data-terminal-generation", "0");

  // A marker on the DOM node proves the shell surface is not torn down and recreated by the
  // navigation; the generation stays zero for the same reason.
  await host.evaluate((element) => {
    (element as HTMLElement & { __rigelSurvives?: boolean }).__rigelSurvives = true;
  });

  await page
    .getByRole("navigation", { name: "Primary" })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  await page.getByRole("option", { name: /^Terminal/ }).click();

  const font = page.getByLabel("Font size");
  await font.fill("17");
  await font.press("Enter");
  await expect(font).toHaveValue("17");

  const scrollback = page.getByLabel("Scrollback");
  await scrollback.fill("12000");
  await scrollback.press("Enter");
  await expect(scrollback).toHaveValue("12000");

  const program = page.getByLabel("Program");
  await program.fill("/bin/zsh");
  await program.press("Enter");

  await page
    .getByRole("navigation", { name: "Primary" })
    .getByRole("button", { name: "Terminal", exact: true })
    .click();

  await expect(host).toHaveAttribute("data-terminal-font-size", "17");
  await expect(host).toHaveAttribute("data-terminal-scrollback", "12000");
  await expect(host).toHaveAttribute("data-terminal-program", "/bin/zsh");
  await expect(host).toHaveAttribute("data-terminal-generation", "0");
  expect(
    await host.evaluate(
      (element) => (element as HTMLElement & { __rigelSurvives?: boolean }).__rigelSurvives,
    ),
  ).toBe(true);
  await expect(page.locator(".terminal-status")).toContainText("17px · 12,000 lines");
  await expect(page.getByRole("application", { name: "Terminal" })).toBeVisible();

  // The program setting is read by the next shell. Restart creates that shell without reloading
  // the window, and the terminal keeps the saved program while the old surface is replaced.
  await page.getByRole("button", { name: "Restart", exact: true }).click();
  await expect(host).toHaveAttribute("data-terminal-generation", "1");
  await expect(host).toHaveAttribute("data-terminal-program", "/bin/zsh");
  await expect(host).toHaveAttribute("data-terminal-font-size", "17");
});

// The packaged window adds a 36px titlebar row. With `display: contents` on the terminal wrapper
// its panes were auto-placed into that row, so the canvas was 36px tall and the content row below
// stayed empty. The surface must own its grid and fill the row no matter the window chrome.
test("the terminal fills the content row in the desktop titlebar layout", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.goto("/");
  await page.getByRole("button", { name: "Terminal", exact: true }).click();
  await expect(page.locator(".terminal-host")).toBeVisible();

  await page.locator(".shell").evaluate((shell) => shell.classList.add("chrome"));

  const boxes = await page.evaluate(() => {
    const rect = (selector: string) => {
      const box = document.querySelector(selector)!.getBoundingClientRect();
      return { x: box.x, y: box.y, width: box.width, height: box.height };
    };
    return {
      pane: rect(".terminal-pane"),
      main: rect(".terminal-main"),
      host: rect(".terminal-host"),
    };
  });

  expect(boxes.main.y).toBeGreaterThanOrEqual(36);
  expect(boxes.main.x).toBeGreaterThan(boxes.pane.x);
  expect(boxes.main.height).toBeGreaterThan(600);
  expect(boxes.host.width).toBeGreaterThan(600);
  expect(boxes.host.height).toBeGreaterThan(550);
});

// The empty-space tools are the shell's own facts: the live grid, the directory and program it
// runs, and the way back to a blank screen. This test covers them as features in their own right.
test("the empty-space tools name the live grid and copy or clear the shell", async ({
  page,
  context,
}) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.goto("/");
  await page.getByRole("button", { name: "Terminal", exact: true }).click();
  await expect(page.locator(".terminal-host")).toBeVisible();

  const tools = page.locator(".terminal-tools");
  await expect(tools.getByText("This shell")).toBeVisible();

  // The size is the terminal's own fitted grid, not a placeholder: two positive numbers.
  const size = tools.locator(".terminal-tools-head code");
  await expect(size).toHaveText(/^\d+ × \d+$/);
  const [cols, rows] = (await size.innerText()).split(" × ").map(Number);
  expect(cols).toBeGreaterThan(20);
  expect(rows).toBeGreaterThan(5);

  // Copy cwd carries the same path the status panel reports, and says so on a live region.
  const cwd = await page
    .locator(".terminal-status-row", { hasText: "cwd" })
    .locator("code")
    .innerText();
  await expect(tools.getByRole("button", { name: "Copy cwd" })).toBeEnabled();
  await tools.getByRole("button", { name: "Copy cwd" }).click();
  await expect(tools.getByRole("status")).toContainText("Working directory copied");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(cwd);

  // Clear empties the scrollback; the shell itself keeps running, so the state does not change.
  await expect(page.locator(".xterm-rows")).toContainText("Browser preview");
  await tools.getByRole("button", { name: "Clear" }).click();
  await expect(tools.getByRole("status")).toContainText("Scrollback cleared");
  await expect(page.locator(".xterm-rows")).not.toContainText("Browser preview");
  await expect(page.locator(".terminal-status")).toContainText("running");

  // The legend names the keys the surface really forwards.
  await expect(tools.locator(".terminal-keys")).toContainText("interrupt");
  await expect(tools.locator(".terminal-keys")).toContainText("clear the screen");
});

// Resizing the window must re-fit the cell grid, not leave the canvas clipped at its old width.
test("the terminal refits when the window resizes", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.goto("/");
  await page.getByRole("button", { name: "Terminal", exact: true }).click();
  await expect(page.locator(".terminal-host")).toBeVisible();

  await page.setViewportSize({ width: 900, height: 700 });
  await expect
    .poll(async () =>
      page.evaluate(() => {
        const host = document.querySelector(".terminal-host")!.getBoundingClientRect();
        const screen = document
          .querySelector(".terminal-host .xterm-screen")!
          .getBoundingClientRect();
        return screen.width <= host.width && screen.height <= host.height;
      }),
    )
    .toBe(true);
});

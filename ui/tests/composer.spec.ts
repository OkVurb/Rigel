import { expect, test } from "@playwright/test";

// D151: history is recorded when a turn is actually delivered, and a snippet is only ever an
// insertion — the review card still stands between the draft and the model.
test("composer history recalls a sent prompt and a snippet fills its placeholder", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session" }).click();

  const composer = page.getByPlaceholder(/Ask the agent/);
  await composer.fill("First prompt for history");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await page.getByRole("button", { name: "Send original" }).click();
  await expect(composer).toHaveValue("");

  await composer.click();
  await page.keyboard.press("ArrowUp");
  await expect(composer).toHaveValue("First prompt for history");

  await composer.fill("");
  await page.getByRole("button", { name: "Snippets" }).click();
  await page.getByRole("menuitem", { name: /Review the diff/ }).click();
  await expect(composer).toHaveValue(/Review the current diff for \{risk\}/);

  // Inserting selects the first placeholder, so typing replaces it rather than appending.
  await page.keyboard.type("the rollback path");
  await expect(composer).toHaveValue(/for the rollback path\./);
});

// D153: the preflight is a range from recorded bills, shows only once the sample is big enough,
// and never takes Send away.
test("the cost preflight shows a range and leaves Send enabled", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session" }).click();

  const composer = page.getByPlaceholder(/Ask the agent/);
  await composer.fill("Add the same retry to the upload path");

  const preflight = page.locator(".composer-preflight");
  await expect(preflight).toBeVisible();
  await expect(preflight).toContainText(/≈ \$[\d.]+–\$[\d.]+ a turn/);
  await expect(preflight).toContainText("from 3 turns");

  await expect(page.getByRole("button", { name: "Send", exact: true })).toBeEnabled();
  await expect(page.locator(".composer-quote")).toBeVisible();
});

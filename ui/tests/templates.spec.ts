import { expect, test } from "@playwright/test";

// D152: "+ session" opens the project's templates when there are any; a blank session is still
// one click, and a template can be deleted from the list it appears in.
test("the create button offers the project's templates", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session" }).click();
  await page.getByRole("button", { name: "sessions" }).click();

  await page.getByRole("button", { name: "+ session" }).click();
  const menu = page.getByRole("menu", { name: "Session templates" });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: /Fix the flaky test/ })).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: /Blank session/ })).toBeVisible();

  await menu.getByRole("button", { name: "Delete template Fix the flaky test" }).click();
  await expect(menu.getByRole("menuitem", { name: /Fix the flaky test/ })).toHaveCount(0);
});

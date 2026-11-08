import { expect, test } from "@playwright/test";

// Q270: the Session's Skills menu searches the local index, pins the current body hash, and
// removes a pin. Saving a template keeps the pins, so the next session can start on the same
// versions. The browser preview carries one sample pin and the same in-memory template file.
test("skills pin from the composer and travel into a template", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();

  await page.getByRole("button", { name: "Session skills" }).click();
  const menu = page.getByRole("menu", { name: "Session skills" });
  await expect(menu).toBeVisible();
  await expect(menu.getByText("pdf", { exact: true })).toBeVisible();

  await menu.getByLabel("Search skills to pin").fill("browser");
  await menu.getByRole("menuitem", { name: /browser-qa/ }).click();
  await expect(
    page.getByRole("list", { name: "Skills pinned to this session" }),
  ).toContainText("browser-qa");

  await page.getByRole("button", { name: "save template" }).click();
  await page.getByRole("button", { name: "sessions" }).click();
  await page.getByRole("button", { name: "+ session" }).click();
  await expect(
    page
      .getByRole("menu", { name: "Session templates" })
      .getByRole("menuitem", { name: /Fix the flaky resume test/ }),
  ).toContainText("2 skills");
});

test("the shelf pins a skill to the open session", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();
  await page.getByRole("button", { name: "Shelf" }).click();

  await page.getByRole("option").filter({ hasText: "browser-qa" }).click();
  await page.getByRole("button", { name: "Pin to session" }).click();
  await expect(page.getByRole("button", { name: "✓ Pinned" })).toBeVisible();

  await page.getByRole("button", { name: "Session", exact: true }).click();
  await page.getByRole("button", { name: "Session skills" }).click();
  await expect(
    page.getByRole("menu", { name: "Session skills" }).getByText("browser-qa"),
  ).toBeVisible();
});

// D141: the preview carries one pin whose shelf body moved. The changed badge opens both
// bodies; Keep leaves the hash, Update moves it, and either closes the review card.
test("a changed pin opens a body diff and can be updated", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();

  await page.getByRole("button", { name: "Session skills" }).click();
  const menu = page.getByRole("menu", { name: "Session skills" });
  await menu.getByRole("button", { name: "Review changed skill pdf" }).click();

  const review = page.getByRole("region", { name: "Review changed skill pin" });
  await expect(review).toBeVisible();
  await expect(review.getByLabel("Skill body changes")).toContainText("wait for the scan");
  await review.getByLabel("Why this pin decision").fill("This session should use the new wait.");
  await review.getByRole("button", { name: "Update to current" }).click();

  await expect(review).toBeHidden();
  await expect(
    page.getByRole("button", { name: "Review changed skill pdf" }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("list", { name: "Skills pinned to this session" }),
  ).toContainText("pdf");
});

test("keeping a changed pin leaves the recorded version in place", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();

  await page.getByRole("button", { name: "Review changed skill pdf" }).click();
  const review = page.getByRole("region", { name: "Review changed skill pin" });
  await review.getByRole("button", { name: "Keep pin" }).click();

  await expect(review).toBeHidden();
  await expect(
    page.getByRole("button", { name: "Review changed skill pdf" }),
  ).toBeVisible();
});

// D141: a long pinned list can hide the one moved body. The filter narrows the menu, and the
// batch action updates every changed pin while recording one reason for the group.
test("the skills menu filters changed pins and updates them together", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();
  const skills = page.getByRole("button", { name: "Session skills" });
  await expect(skills.getByText("1 changed", { exact: true })).toBeVisible();
  await skills.click();
  const menu = page.getByRole("menu", { name: "Session skills" });

  await menu.getByLabel("Search skills to pin").fill("browser");
  await menu.getByRole("menuitem", { name: /browser-qa/ }).click();
  await expect(menu.getByText("browser-qa", { exact: true })).toBeVisible();

  await menu.getByRole("button", { name: /changed only/ }).click();
  await expect(menu.getByText("pdf", { exact: true })).toBeVisible();
  await expect(menu.getByText("browser-qa", { exact: true })).toHaveCount(0);

  await menu
    .getByLabel("Reason for the batch pin decision")
    .fill("The shelf wording is now the one this session should read.");
  await menu.getByRole("button", { name: "Update all changed" }).click();

  await expect(menu).toBeHidden();
  await expect(skills.getByText("1 changed", { exact: true })).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Review changed skill pdf" }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("list", { name: "Skills pinned to this session" }),
  ).toContainText("browser-qa");
});

// D141: Keep is a decision, not a re-pin. The batch form records one reason while every moved
// pin stays on its pinned hash, and the composer button keeps showing the changed count.
test("the skills menu keeps every changed pin with one batch reason", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();
  const skills = page.getByRole("button", { name: "Session skills" });
  await expect(skills.getByText("1 changed", { exact: true })).toBeVisible();
  await skills.click();
  const menu = page.getByRole("menu", { name: "Session skills" });

  await menu
    .getByLabel("Reason for the batch pin decision")
    .fill("The reviewed wording is still the one this session should use.");
  await menu.getByRole("button", { name: "Keep all changed" }).click();

  await expect(menu).toBeHidden();
  await expect(
    page.getByRole("button", { name: "Review changed skill pdf" }),
  ).toBeVisible();
  await expect(skills.getByText("1 changed", { exact: true })).toBeVisible();
});
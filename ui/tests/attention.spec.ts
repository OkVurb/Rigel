import { expect, test } from "@playwright/test";

// D154: the rail count is the number of unresolved rows, the view groups them by session, an
// approval names no session, and a claim or review row opens Diff rather than the transcript.
test("the badge counts unresolved rows and the view groups them by session", async ({ page }) => {
  await page.goto("/");

  const nav = page.locator("nav.rail");
  const rail = nav.getByRole("button", { name: /^Attention/ });
  await expect(rail).toHaveAccessibleName("Attention, 3 waiting");
  await expect(page.locator(".rail-count")).toHaveText("3");

  await rail.click();
  await expect(page.getByRole("heading", { name: "3 unresolved" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Waiting on you" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Fix the flaky resume test" })).toBeVisible();

  // No list pane here: the view takes the list's column as well as the transcript's.
  const box = await page.locator("#main").boundingBox();
  expect(box?.width ?? 0).toBeGreaterThan(600);

  // The question records the turn that asked, so the row targets it rather than the card.
  const approval = page.getByRole("button", { name: /cargo test --workspace/ });
  await expect(approval).toHaveAttribute("title", "Open the turn that asked");
  await approval.click();
  await expect(nav.getByRole("button", { name: "Session" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(
    page.getByRole("heading", { name: "Fix the flaky resume test" }),
  ).toBeVisible();

  // A blocked merge is resolved in Diff.
  await rail.click();
  await page.getByRole("button", { name: /Blocked merge from writer/ }).click();
  await expect(nav.getByRole("button", { name: "Diff" })).toHaveAttribute("aria-pressed", "true");
});

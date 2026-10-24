import { expect, test } from "@playwright/test";

// P7.3's five surfaces, exercised the way a person reaches them: the rail on the left.
test("every surface renders from the rail", async ({ page }) => {
  await page.goto("/");

  // Shelf: the sample rows, the search field and the evidence table's heading.
  await expect(page.getByRole("heading", { name: "Shelf", exact: true }).first()).toBeVisible();
  await expect(page.getByRole("listbox", { name: "Skills" })).toBeVisible();
  await expect(page.getByPlaceholder("Search skills…")).toBeVisible();

  await page.getByRole("button", { name: "Session" }).click();
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible();
  await expect(page.getByText("Browser preview: sample transcript.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Swarm" })).toBeVisible();

  await page.getByRole("button", { name: "Chat" }).click();
  await expect(page.getByRole("heading", { name: "# general" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "# review" })).toBeVisible();
  await expect(page.getByText("THE ROOM")).toBeVisible();

  await page.getByRole("button", { name: "Diff" }).click();
  await expect(page.getByRole("heading", { name: "One tree, several agents" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Conflicts" })).toBeVisible();
  await expect(page.getByText("SUBAGENT WORKTREES")).toBeVisible();

  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByRole("heading", { name: "Providers and keys" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "MODEL AND EFFORT" })).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "Approval waiting" })).toBeChecked();
});

test("the shelf search filters without leaving the surface", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("listbox", { name: "Skills" })).toBeVisible();
  await page.keyboard.press("/");
  const search = page.getByPlaceholder("Search skills…");
  await expect(search).toBeFocused();
  await search.fill("pdf");
  await expect(page.getByRole("option", { name: /pdf/ })).toHaveCount(1);
  await expect(page.getByText("No skill matches")).toHaveCount(0);
});

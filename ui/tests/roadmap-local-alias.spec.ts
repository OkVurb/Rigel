import { expect, test } from "@playwright/test";

// Roadmap 27: a keyless local door is a row like any other, and the picker says where it runs.
test("a local model is tagged local in the picker", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();
  await page.locator(".model-button").click();

  const row = page.locator(".model-row", { hasText: "llama3.2" }).first();
  await expect(row).toBeVisible();
  await expect(row.locator(".tag.local")).toHaveText("local");
});

// Roadmap 29: the inspector reads the resolver's own fields, so an alias shows the word that
// was typed and the concrete pair that will be sent.
test("an alias resolves to its concrete pair in the route inspector", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Settings", exact: true }).click();

  const inspector = page.getByLabel("Route inspector");
  await inspector.getByLabel("Requested model").fill("fast");

  await expect(inspector.getByTestId("route-requested")).toHaveText("openrouter / fast");
  await expect(inspector.getByTestId("route-resolved")).toHaveText(
    "openrouter / openai/gpt-5-nano",
  );
  await expect(inspector.getByTestId("route-reason")).toHaveText("alias");
  await expect(inspector.getByTestId("route-mismatch")).toContainText("openai/gpt-5-nano");
});

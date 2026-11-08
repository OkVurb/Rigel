import { expect, test } from "@playwright/test";

// Q268: Run tests is one explicit local action. The receipt parses pass/fail counts and the
// full output stays behind a disclosure instead of filling the transcript.
test("Run tests reports parsed counts and opens the captured output", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();

  await page.getByRole("button", { name: "Run tests" }).click();

  const result = page.getByRole("region", { name: "Test results" });
  await expect(result).toBeVisible();
  await expect(
    result.locator(".test-run-head").getByText("418 passed", { exact: true }),
  ).toBeVisible();
  await expect(
    result.locator(".test-run-head").getByText("1 failed", { exact: true }),
  ).toBeVisible();
  await expect(result.getByText("exit 101")).toBeVisible();
  await expect(result.getByText("cargo test --workspace")).toBeVisible();

  await result.getByText("full output").click();
  await expect(result.locator("pre")).toContainText(
    "test result: FAILED. 418 passed; 1 failed",
  );
});

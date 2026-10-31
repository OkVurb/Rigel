import { expect, test } from "@playwright/test";

// D136/D149: the filter chips and the outline are derived from the same rows, so the preview's
// sample transcript is enough to check the classification and the per-session memory.
test("the transcript filters, remembers, and draws an outline", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session" }).click();

  const bar = page.getByRole("group", { name: "Transcript filter" });
  const filter = (name: string) => bar.getByRole("button", { name: new RegExp(`^${name}`) });
  const transcript = page.getByRole("region", { name: "Transcript" });
  await expect(filter("all")).toHaveAttribute("aria-pressed", "true");

  await filter("diffs").click();
  await expect(transcript.getByText("crates/rigel-core/tests/resume.rs").first()).toBeVisible();
  await expect(transcript.getByText("read_file")).toHaveCount(0);

  await filter("yours").click();
  await expect(transcript.getByText(/The resume test fails on Windows/)).toBeVisible();
  await expect(transcript.getByText("read_file")).toHaveCount(0);

  const outline = page.getByRole("navigation", { name: "Turn outline" });
  await expect(outline).toBeVisible();
  expect(await outline.getByRole("button").count()).toBeGreaterThan(0);
  await outline.getByRole("button").first().click();

  await page.reload();
  await page.getByRole("button", { name: "Session" }).click();
  await expect(
    page
      .getByRole("group", { name: "Transcript filter" })
      .getByRole("button", { name: /^yours/ }),
  ).toHaveAttribute("aria-pressed", "true");
});

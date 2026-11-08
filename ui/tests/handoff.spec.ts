import { expect, test } from "@playwright/test";

// Q266: a handoff is a draft for another agent. The picker switches to that agent's pane and
// fills the composer with the turn's files and outcome, but the turn only starts on Send.
test("a finished turn prefills a handoff for another agent without sending", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Session", exact: true }).click();

  const turn = page.locator("#turn-turn-preview-2");
  await turn
    .getByRole("combobox", { name: "Hand turn 2 to another agent" })
    .selectOption("agent-preview-reviewer");

  const composer = page.locator(".composer textarea");
  await expect(composer).toBeFocused();
  await expect(composer).toHaveValue(/Handoff from turn 2 \(worker\)\./);
  await expect(composer).toHaveValue(/Outcome: Applied one hunk/);
  await expect(composer).toHaveValue(/- crates\/rigel-core\/tests\/resume\.rs/);
  await expect(composer).toHaveValue(/Take over from here/);

  // The reviewer pane is now the one on screen; its single recorded turn is still the only one,
  // so the handoff has not started a new turn.
  await expect(page.getByRole("region", { name: "reviewer transcript" })).toBeVisible();
  await expect(page.locator(".turn")).toHaveCount(1);
});

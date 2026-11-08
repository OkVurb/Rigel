import { expect, test } from "@playwright/test";

// The Gravity card is opt-in, and every part of the opt-in is visible: the switch, the test-ad
// switch, the shared endpoint and the labelled cards on the paper and in Session. Off means the
// slots are absent while the constellation companion stays.
test("sponsored cards are opt-in, labelled and drawn from the preview ad", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("option", { name: /Sponsors/ }).click();

  await expect(page.getByRole("heading", { name: "Sponsored cards" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Shared endpoint" })).toBeVisible();
  await expect(page.getByText("https://rigel-25128.web.app/api/gravity-ad")).toBeVisible();

  const cardSwitch = page.getByRole("checkbox", { name: "Show sponsored cards in Rigel" });
  await expect(cardSwitch).not.toBeChecked();
  await expect(page.getByRole("checkbox", { name: /Show test ads/ })).toBeDisabled();

  await cardSwitch.check();
  await expect(page.getByRole("checkbox", { name: /Show test ads/ })).toBeChecked();

  await page.getByRole("button", { name: "Home", exact: true }).click();
  const card = page.getByRole("region", { name: "Sponsored" });
  await expect(card).toBeVisible();
  await expect(card.getByText("Advertisement", { exact: true })).toBeVisible();
  await expect(card.getByRole("heading", { name: "Ship a database this afternoon" })).toBeVisible();
  await expect(card.getByRole("button", { name: /Learn more/ })).toBeVisible();

  // Session gets the same opt-in ad, with the default Ursa perched on its top-left edge.
  await page.getByRole("button", { name: "Session", exact: true }).click();
  const companion = page.getByRole("group", { name: "Companion and sponsor" });
  const sessionCard = companion.getByRole("region", { name: "Sponsored" });
  const pet = companion.getByRole("button", { name: /Ursa, the constellation bear/ });
  await expect(pet).toBeVisible();
  await expect(pet.locator("img")).toHaveAttribute("src", "/constellations/ursa.svg");
  await expect(sessionCard).toBeVisible();
  await expect(sessionCard.getByText("Advertisement", { exact: true })).toBeVisible();
  await expect(sessionCard.getByRole("heading", { name: "Ship a database this afternoon" })).toBeVisible();

  // Every animal is in the picker, and every asset resolves before a choice is made.
  await pet.click();
  const picker = page.getByRole("dialog", { name: "Choose a constellation companion" });
  await expect(picker).toBeVisible();
  await expect(picker.getByRole("button")).toHaveCount(6);
  await expect
    .poll(async () =>
      picker.locator("img").evaluateAll((images) =>
        images.every((image) => {
          const candidate = image as HTMLImageElement;
          return candidate.complete && candidate.naturalWidth > 0;
        }),
      ),
    )
    .toBe(true);
  await picker.getByRole("button", { name: /Lepus, the constellation hare/ }).click();
  await expect(companion.getByRole("button", { name: /Lepus, the constellation hare/ })).toBeVisible();

  // Off again: Home drops the slot instead of leaving an empty box; Lepus remains in Session.
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("option", { name: /Sponsors/ }).click();
  await cardSwitch.uncheck();
  await page.getByRole("button", { name: "Session", exact: true }).click();
  await expect(companion.getByRole("button", { name: /Lepus, the constellation hare/ })).toBeVisible();
  await expect(companion.getByRole("region", { name: "Sponsored" })).toHaveCount(0);
  await page.getByRole("button", { name: "Home", exact: true }).click();
  await expect(page.getByRole("region", { name: "Sponsored" })).toHaveCount(0);
});

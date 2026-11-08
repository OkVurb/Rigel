import { expect, test } from "@playwright/test";

// Roadmap 26: the effective policy is read from the project and shown before the turn.
test("the OpenRouter routing policy is shown in the route inspector", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Settings", exact: true }).click();

  const inspector = page.getByLabel("Route inspector");
  await expect(inspector.getByTestId("route-policy")).toContainText(
    "OpenRouter policy: order openai -> anthropic",
  );
  await expect(inspector.getByTestId("route-policy")).toContainText("fallbacks off");
  await expect(inspector.getByTestId("route-policy")).toContainText("quantizations fp16");
});

// Roadmap 28: the probe lists the door's own models, and Save returns only masked header names.
test("a custom door probes its models and never shows the header value again", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("option", { name: /^Providers/ }).click();

  const section = page.getByLabel("Custom OpenAI-compatible door");
  await section.getByLabel("Custom base URL").fill("https://gateway.example/v1");
  await section.getByLabel("Header name").first().fill("x-gateway-token");
  await section.getByLabel("Header value").first().fill("secret-value");
  await section.getByRole("button", { name: "Probe", exact: true }).click();

  await expect(section.getByTestId("custom-models")).toContainText("llama-3.2-8b");
  await section.getByRole("button", { name: "Save door", exact: true }).click();
  await expect(section.getByTestId("custom-saved")).toContainText("x-gateway-token");
  await expect(section.getByTestId("custom-saved")).not.toContainText("secret-value");
});

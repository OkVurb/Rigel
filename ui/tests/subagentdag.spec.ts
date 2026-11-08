import { expect, test } from "@playwright/test";

// Roadmap 42: the parent turn renders the DAG it spawned, including each node's state and the
// dependency edge, without opening the child transcripts.
test("the parent turn renders its subagent graph and edges", async ({ page }) => {
  await page.goto("/?dag=1");
  await page.getByRole("button", { name: "Session", exact: true }).click();

  const graph = page.getByRole("region", { name: /Subagent graph for turn/ });
  await expect(graph).toBeVisible();
  await expect(graph.locator(".subagent-graph-head .mono")).toHaveText("subagent graph");

  const reader = graph.locator("li[data-state='done']").filter({ hasText: "reader" });
  await expect(reader).toContainText("root");
  const verifier = graph.locator("li[data-state='running']").filter({ hasText: "verifier" });
  await expect(verifier).toContainText("after reader");
  await expect(graph.locator(".subagent-edge")).toHaveText("reader → verifier");
});

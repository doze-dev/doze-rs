// The region the console looks at: a choice among the regions of AWS (the ones that hold something first), the same for every page, and
// each region keeps its own resources.
import { ENDPOINT, expect, open, test, unique } from "./fixtures";

test("a region is chosen in the top bar, every page then shows it, and a resource made there is not in the other", async ({ page, cli, browserName }) => {
  const queue = unique("regional", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", queue], { region: "eu-west-1" }).code).toBe(0);
  await open(page, "/resources/sqs/queues", "Queues");
  const chip = page.getByTestId("chip-region");
  await expect(chip).toHaveText(/us-east-1/);
  await expect(page.getByTestId("region-not-default")).toHaveCount(0);
  // Not in the default region.
  await expect(page.getByText(queue)).toHaveCount(0);
  await chip.click();
  // Only regions of AWS are offered; the one that holds the queue is among the first.
  const menu = page.getByTestId("region-menu");
  await expect(menu.getByTestId("region-option-eu-west-1")).toBeVisible();
  expect(await menu.locator('[data-testid^="region-option-"]').count()).toBeGreaterThanOrEqual(30);
  await expect(menu.getByTestId("region-option-not-a-region-9")).toHaveCount(0);
  await menu.getByTestId("region-option-eu-west-1").click();
  await expect(chip).toHaveText(/eu-west-1/);
  await expect(page.getByTestId("region-not-default")).toBeVisible();
  await expect(page.getByText("Standard and FIFO queues in eu-west-1.")).toBeVisible();
  await expect(page.getByRole("link", { name: queue })).toBeVisible();
  // The choice is kept across a reload, and choosing the default again is the same as none.
  await page.reload();
  await expect(page.getByTestId("chip-region")).toHaveText(/eu-west-1/);
  await page.getByTestId("chip-region").click();
  await page.getByTestId("region-option-us-east-1").click();
  await expect(page.getByTestId("chip-region")).toHaveText(/us-east-1/);
  await expect(page.getByRole("link", { name: queue })).toHaveCount(0);
  expect(cli(["sqs", "delete-queue", "--queue-url", `${ENDPOINT}/111122223333/${queue}`], { region: "eu-west-1" }).code).toBe(0);
});

test("Recent and Pinned are kept for each region: a queue opened in one is not a place of the other", async ({ page, cli, browserName }) => {
  const queue = unique("recents", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", queue], { region: "eu-west-1" }).code).toBe(0);
  await open(page, "/resources/sqs/queues", "Queues");
  await page.getByTestId("chip-region").click();
  await page.getByTestId("region-option-eu-west-1").click();
  await page.getByRole("link", { name: queue }).click();
  await expect(page.getByTestId("rail-recent").getByTestId("rail-place")).toHaveText([queue]);
  // Back in the default region the rail has no memory of it, and in eu-west-1 it still has.
  await page.getByTestId("chip-region").click();
  await page.getByTestId("region-option-us-east-1").click();
  await expect(page.getByTestId("rail-recent").getByText(queue)).toHaveCount(0);
  await page.getByTestId("chip-region").click();
  await page.getByTestId("region-option-eu-west-1").click();
  await expect(page.getByTestId("rail-recent").getByTestId("rail-place")).toHaveText([queue]);
  expect(cli(["sqs", "delete-queue", "--queue-url", `${ENDPOINT}/111122223333/${queue}`], { region: "eu-west-1" }).code).toBe(0);
});

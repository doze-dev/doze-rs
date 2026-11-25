// The page header: its title row (the name and the verbs) stays under the top bar while a long page scrolls, drawn once.
import { expect, open, test, unique, ENDPOINT } from "./fixtures";

test("the title row and its verbs stay under the top bar on a long page, and nothing is drawn twice", async ({ page, cli, browserName }) => {
  const queue = unique("sticky", browserName);
  expect(cli(["sqs", "create-queue", "--queue-name", queue], {}).code).toBe(0);
  await open(page, `/resources/sqs/queues/${queue}`, queue);
  const row = page.locator('[data-slot="page-title-row"]');
  await expect(row).toHaveCount(1);
  await expect(row).not.toHaveAttribute("data-stuck", /.*/);
  const heightBefore = await page.evaluate(() => document.documentElement.scrollHeight);
  await page.evaluate(() => window.scrollTo(0, 500));
  await expect(row).toHaveAttribute("data-stuck", "true");
  // Under the 52 px top bar, with the verbs in it.
  const top = await row.evaluate((el) => el.getBoundingClientRect().top);
  expect(top).toBe(52);
  await expect(row.getByRole("button", { name: "Delete" })).toBeInViewport();
  // The layout did not change on the way, and no verb exists twice.
  expect(await page.evaluate(() => document.documentElement.scrollHeight)).toBe(heightBefore);
  await expect(page.getByRole("button", { name: "Delete", exact: true })).toHaveCount(1);
  // Back at the top the line under it goes away again.
  await page.evaluate(() => window.scrollTo(0, 0));
  await expect(row).not.toHaveAttribute("data-stuck", /.*/);
  expect(cli(["sqs", "delete-queue", "--queue-url", `${ENDPOINT}/111122223333/${queue}`], {}).code).toBe(0);
});

// The shared pieces of a list page and of a chart: SelectableTable, FilterChips (kept in the address), Pagination, Chart, and
// CodeBlock. Driven with a real mouse and keyboard.
import { expect, open, overflowX, ringOf, tabKey, tabTo, test } from "./fixtures";

const LOOK = { theme: "aws", mode: "light" } as const;
const DARK = { theme: "aws", mode: "dark" } as const;

test("lists and charts: no sideways scroll at 1024 px, in either look", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 768 });
  for (const look of [LOOK, DARK, { theme: "kafka", mode: "light" } as const]) {
    for (const path of ["/lists", "/charts"]) {
      await open(page, path, look);
      await page.waitForTimeout(400);
      expect(await overflowX(page), `${path} under ${look.theme}-${look.mode}`).toBe(0);
    }
  }
});

test("SelectableTable: choose a row, all rows, some rows; the bar holds its height; Clear and the bulk action work", async ({ page }) => {
  await open(page, "/lists", LOOK);
  const sec = page.getByTestId("section-selectable");
  const table = sec.locator('[data-slot="selectable-table"]').first();
  const bar = table.getByTestId("selection-bar");
  await expect(bar).toContainText("Choose resources to act on them together.");
  const h0 = (await bar.boundingBox())!.height;
  const head = table.getByTestId("select-all");
  await expect(head).toHaveAttribute("aria-checked", "false");
  await table.getByTestId("select-r1").click();
  await expect(table.getByTestId("selected-count")).toHaveText("1 resource selected");
  await expect(head).toHaveAttribute("aria-checked", "mixed");
  expect((await bar.boundingBox())!.height).toBe(h0);
  const ground = (id: string) => table.locator(`tr[data-row=${id}] td`).first().evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(await ground("r1")).not.toBe(await ground("r0"));
  await head.click();
  await expect(table.getByTestId("selected-count")).toHaveText("12 resources selected");
  await expect(head).toHaveAttribute("aria-checked", "true");
  await head.click();
  await expect(bar).toContainText("Choose resources");
  await table.getByTestId("select-r2").click();
  await table.getByTestId("select-r4").click();
  await table.getByTestId("bulk-delete").click();
  await expect(sec.getByTestId("bulk-done").first()).toContainText("2 deleted");
  await expect(bar).toContainText("Choose resources");
  await table.getByTestId("select-r2").click();
  await table.getByTestId("selection-clear").click();
  await expect(bar).toContainText("Choose resources");
});

test("SelectableTable: a thousand rows are virtual, and 'select all' chooses every one of them", async ({ page }) => {
  await open(page, "/lists", LOOK);
  const big = page.getByTestId("section-selectable").locator('[data-slot="selectable-table"]').nth(2);
  expect(await big.locator("tbody tr[data-row]").count()).toBeLessThan(60);
  await big.getByTestId("select-all").click();
  await expect(big.getByTestId("selected-count")).toHaveText("1,000 resources selected");
  await big.locator('[data-slot="data-table"]').evaluate((el) => el.scrollTo(0, el.scrollHeight));
  await expect(big.getByTestId("select-r999")).toBeChecked();
});

test("SelectableTable: the checkboxes are reached and shown by the keyboard, and are named", async ({ page, browserName }) => {
  await open(page, "/lists", LOOK);
  const table = page.getByTestId("section-selectable").locator('[data-slot="selectable-table"]').first();
  const first = table.getByTestId("select-r0");
  await expect(first).toHaveAccessibleName("Select resource r0");
  await tabTo(page, browserName, first, 200);
  expect((await ringOf(first)).visible).toBe(true);
  await page.keyboard.press("Space");
  await expect(table.getByTestId("selected-count")).toHaveText("1 resource selected");
});

test("FilterChips: a chip filters the table, says how many, is kept in the address, and an unknown value falls back", async ({ page, browserName }) => {
  await open(page, "/lists", LOOK);
  const list = page.getByTestId("filtered-list");
  await expect(list.getByTestId("chip-all")).toHaveAttribute("aria-checked", "true");
  await expect(list.getByTestId("chip-Idle")).toHaveAccessibleName("Idle 3");
  await expect(list.getByTestId("page-line")).toHaveText("1 to 12 of 12 resources");
  await list.getByTestId("chip-Idle").click();
  await expect(page).toHaveURL(/[?&]state=Idle\b/);
  await expect(list.getByTestId("page-line")).toHaveText("1 to 3 of 3 resources");
  await expect(list.locator("tbody tr[data-row]")).toHaveCount(3);
  await page.reload();
  await expect(page.getByTestId("filtered-list").getByTestId("chip-Idle")).toHaveAttribute("aria-checked", "true");
  // The Back button is not made to walk through every click.
  await page.getByTestId("filtered-list").getByTestId("chip-Failing").click();
  await expect(page).toHaveURL(/state=Failing/);
  await page.getByTestId("filtered-list").getByTestId("chip-all").click();
  await expect(page).not.toHaveURL(/state=/);
  await page.goto("/lists?state=gone");
  await expect(page.getByTestId("filtered-list").getByTestId("chip-all")).toHaveAttribute("aria-checked", "true");
  // Keyboard.
  const chip = page.getByTestId("filtered-list").getByTestId("chip-Active");
  await page.getByTestId("filtered-list").getByTestId("chip-all").focus();
  await page.keyboard.press(tabKey(browserName));
  await expect(chip).toBeFocused();
  expect((await ringOf(chip)).visible).toBe(true);
  await page.keyboard.press("Enter");
  await expect(chip).toHaveAttribute("aria-checked", "true");
});

test("Pagination: the line, and Load more reads the next page until there is no more", async ({ page }) => {
  await open(page, "/lists", LOOK);
  const sec = page.getByTestId("section-pagination");
  await expect(sec.getByTestId("page-line").first()).toHaveText("1 to 25 of 25 queues");
  const paged = sec.getByTestId("page-line").nth(2);
  await expect(paged).toHaveText("1 to 25, more to load");
  for (const n of [50, 75, 100]) {
    await sec.getByTestId("load-more").first().click();
    await expect(paged).toHaveText(n === 100 ? "1 to 100 of 100 resources" : `1 to ${n}, more to load`);
  }
  await expect(sec.getByTestId("load-more")).toHaveCount(1);
  await expect(sec.getByTestId("load-more")).toBeDisabled();
});

test("Chart: the pointer reads the nearest point, the arrow keys do too, Escape lets go, the legend names each series", async ({ page }) => {
  await open(page, "/charts", LOOK);
  const chart = page.getByTestId("chart").first();
  await expect(chart.getByTestId("chart-legend")).toContainText("OrdersPlaced");
  await expect(chart.locator("svg path[class*=stroke]")).toHaveCount(2);
  const box = (await chart.locator("svg").boundingBox())!;
  await page.mouse.move(box.x + box.width * 0.5, box.y + 60);
  const readout = chart.getByTestId("chart-readout");
  await expect(readout).toBeVisible();
  await expect(readout).toContainText("2026-11-08");
  await expect(readout).toContainText("UTC");
  await expect(readout).toContainText("OrdersPlaced");
  await expect(readout).toContainText("OrdersFailed");
  await page.mouse.move(box.x + box.width * 0.5, box.y - 60);
  await expect(readout).toBeHidden();
  // The keyboard: the chart takes the focus (with a ring), the arrows step through the points.
  await chart.getByRole("group").focus();
  await page.keyboard.press("ArrowRight");
  const first = await readout.locator(".font-mono").textContent();
  await page.keyboard.press("ArrowRight");
  expect(await readout.locator(".font-mono").textContent()).not.toBe(first);
  await page.keyboard.press("Home");
  await expect(readout).toContainText("05:20:00");
  await page.keyboard.press("Escape");
  await expect(readout).toBeHidden();
});

test("Chart: the x axis is on UTC boundaries whatever the range, and a range switch redraws it", async ({ page }) => {
  await open(page, "/charts", LOOK);
  const ranged = page.getByTestId("chart-ranged");
  const labels = async () => (await ranged.locator("svg text").allTextContents()).filter((t) => /^\d\d:\d\d$|^[A-Z][a-z]{2} \d+$/.test(t));
  expect((await labels()).every((t) => /^\d\d:(00|30)$/.test(t))).toBe(true);
  await page.getByTestId("range-1w").click();
  await expect.poll(async () => (await labels()).every((t) => /^[A-Z][a-z]{2} \d+$/.test(t))).toBe(true);
  await page.getByTestId("range-1h").click();
  await expect.poll(async () => (await labels()).every((t) => /^\d\d:\d0$/.test(t))).toBe(true);
});

test("Chart: a threshold is drawn with its words, gaps break the line, one point is a dot, empty and loading say so", async ({ page }) => {
  await open(page, "/charts", LOOK);
  await expect(page.getByTestId("chart-threshold-threshold")).toContainText("Alarm at 12");
  // The line has three runs: a gap is a gap, never a line to zero.
  const d = await page.getByTestId("chart-gaps").locator("svg path[class*=stroke]").getAttribute("d");
  expect((d!.match(/M/g) ?? []).length).toBe(3);
  await expect(page.getByTestId("chart-single").locator("svg circle")).toHaveCount(1);
  await expect(page.getByTestId("chart-empty-empty")).toContainText("No data points");
  await expect(page.getByTestId("chart-loading-loading")).toBeVisible();
  await expect(page.getByTestId("chart-five-legend").locator("li")).toHaveCount(5);
});

test("Chart: with reduced motion it appears at once, without it it fades in", async ({ browser }) => {
  for (const [reduce, expectNone] of [["reduce", true], ["no-preference", false]] as const) {
    const ctx = await browser.newContext({ reducedMotion: reduce, viewport: { width: 1440, height: 900 } });
    const page = await ctx.newPage();
    await open(page, "/charts", LOOK);
    const name = await page.getByTestId("chart").first().locator("svg").evaluate((el) => getComputedStyle(el).animationName);
    expect(name === "none", `animation under ${reduce}: ${name}`).toBe(expectNone);
    await ctx.close();
  }
});

test("CodeBlock and JsonBlock: a block is reached by the keyboard to scroll it, and wrapped text stays inside the page", async ({ page, browserName }) => {
  await open(page, "/charts", LOOK);
  const block = page.getByTestId("code-nowrap");
  await tabTo(page, browserName, block, 300);
  expect((await ringOf(block)).visible).toBe(true);
  expect(await block.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
  await expect(page.getByTestId("json-block")).toContainText('"QueueName": "pick-lists"');
  expect(await overflowX(page)).toBe(0);
});

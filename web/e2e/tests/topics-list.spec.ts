// The Topics list (slice R1a): the filter and the Show internal switch are in the address and work with the mouse and the
// keyboard; the empty filter result says so; a name opens the topic; nothing scrolls sideways.
import { cutHeaders, expect, overflowX, test } from "./fixtures";

test("the list: one h1, the figures as stat tiles, a row per topic, and a name opens the topic", async ({ page }) => {
  await page.goto("/topics");
  await expect(page.locator("h1")).toHaveText("Topics");
  const strip = page.locator("[data-slot=stat-strip]");
  for (const label of ["Topics", "Partitions", "Newest write", "Written to"]) await expect(strip).toContainText(label);
  const table = page.getByRole("table", { name: "Topics" });
  await expect(table.locator("#r-orders")).toBeVisible();
  await expect(table.locator("#r-__consumer_offsets")).toHaveCount(0);
  await table.locator("#r-orders").getByTestId("topic-link").click();
  await expect(page).toHaveURL(/\/topics\/orders\/messages$/);
  await expect(page.locator("h1")).toHaveText("orders");
  await page.goBack();
  await expect(page.locator("h1")).toHaveText("Topics");
});

test("Show internal: the switch works with the mouse and the keyboard, is in the address, and the row joins the list", async ({ page }) => {
  await page.goto("/topics");
  const sw = page.getByTestId("show-internal");
  await expect(sw).toHaveAttribute("aria-checked", "false");
  await sw.click();
  await expect(sw).toHaveAttribute("aria-checked", "true");
  await expect(page).toHaveURL(/internal=true/);
  const table = page.getByRole("table", { name: "Topics" });
  await expect(table.locator("#r-__consumer_offsets")).toBeVisible();
  await expect(table.locator("#r-__consumer_offsets")).toContainText("internal");
  await expect(page.getByTestId("internal-note")).toContainText("The internal topic is shown.");
  // The label toggles it too.
  await page.locator("label[for=show-internal]").click();
  await expect(sw).toHaveAttribute("aria-checked", "false");
  await expect(table.locator("#r-__consumer_offsets")).toHaveCount(0);
  // The keyboard.
  await sw.focus();
  await page.keyboard.press("Space");
  await expect(sw).toHaveAttribute("aria-checked", "true");
  await expect(page).toHaveURL(/internal=true/);
  // A reload keeps it.
  await page.reload();
  await expect(page.getByTestId("show-internal")).toHaveAttribute("aria-checked", "true");
  await expect(page.getByRole("table", { name: "Topics" }).locator("#r-__consumer_offsets")).toBeVisible();
});

test("the filter narrows the list as you type, is in the address, says when nothing matches, and Clear brings all back", async ({ page }) => {
  await page.goto("/topics");
  const table = page.getByRole("table", { name: "Topics" });
  await expect(table.locator("#r-orders")).toBeVisible();
  const filter = page.getByTestId("topics-filter");
  await filter.click();
  await page.keyboard.type("pay");
  await expect(page).toHaveURL(/q=pay/);
  await expect(table.locator("#r-orders")).toHaveCount(0);
  await expect(table.locator("#r-payments\\.settled")).toBeVisible();
  await expect(page.getByTestId("topics-count")).toHaveText(/^\d+ of \d+ topics$/);
  await filter.fill("zzz-nothing-is-called-this");
  await expect(page.getByTestId("topics-page")).toHaveAttribute("data-state", "filtered");
  await expect(page.getByText("No topic name contains zzz-nothing-is-called-this")).toBeVisible();
  await page.getByTestId("clear-filter").click();
  await expect(page).not.toHaveURL(/q=/);
  await expect(page.getByRole("table", { name: "Topics" }).locator("#r-orders")).toBeVisible();
  await expect(filter).toHaveValue("");
  // Esc in the field clears it.
  await filter.fill("ord");
  await expect(page).toHaveURL(/q=ord/);
  await filter.press("Escape");
  await expect(filter).toHaveValue("");
  await expect(page).not.toHaveURL(/q=/);
});

test("the Topics list does not scroll sideways at 1024, 1280, 1440 and 1920, and a refresh moves nothing", async ({ page }) => {
  await page.goto("/topics");
  await expect(page.getByRole("table", { name: "Topics" })).toBeVisible();
  for (const width of [1024, 1280, 1440, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    // Polled: crossing 1280 px the sidebar folds to icons with its own short transition, and the page settles after it.
    await expect.poll(() => overflowX(page), { message: `${width}` }).toBeLessThanOrEqual(0);
    if (width >= 1280) expect(await cutHeaders(page), `headers cut at ${width}`).toEqual([]);
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  const before = await page.getByRole("table", { name: "Topics" }).boundingBox();
  const row = await page.locator("#r-orders").elementHandle();
  await page.getByTestId("refresh-now").click();
  await expect(page.getByTestId("refresh-now").locator("svg.animate-spin")).toHaveCount(0);
  const after = await page.getByRole("table", { name: "Topics" }).boundingBox();
  expect(Math.round(after!.y)).toBe(Math.round(before!.y));
  expect(await row!.evaluate((el) => el.isConnected)).toBe(true);
});

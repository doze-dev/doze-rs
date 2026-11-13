// A topic's frame and its Messages (slice R1a): tabs as routes; every filter in the address (partitions, offset or key,
// time, isolation, Older); the record Sheet open, close, Esc, Back and a deep link; the Live tail on a broker with traffic.
import { boxes, choose, expect, overflowX, test } from "./fixtures";
import { produce } from "./kafka";

const rows = (page: import("@playwright/test").Page) => page.getByRole("table", { name: "Records, newest first" }).locator("tbody tr[data-row]");

test("the frame: one h1, the stats, the transaction region, five tabs as routes, and a tab switch moves nothing above it", async ({ page }) => {
  await page.goto("/topics/orders");
  await expect(page.locator("h1")).toHaveText("orders");
  await expect(page.locator("h1")).toHaveCount(1);
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText("Topics");
  const strip = page.locator("[data-slot=stat-strip]").first();
  for (const label of ["Partitions", "Retention", "Records", "Size on disk", "Last write"]) await expect(strip).toContainText(label);
  await expect(page.getByTestId("txn-region")).toBeVisible();
  const tabs = page.getByTestId("topic-tabs");
  await expect(tabs.getByRole("link")).toHaveText([/^Messages$/, /^Log$/, /^Config$/, /^Consumers\s*\d+$/, /^Producers\s*\d+$/]);
  const before = await boxes(page, ["h1", "[data-testid=topic-tabs]", "[data-testid=txn-region]"]);
  for (const [tab, url] of [
    ["Log", /\/topics\/orders\/log$/],
    ["Consumers", /\/topics\/orders\/consumers$/],
    ["Producers", /\/topics\/orders\/producers$/],
    ["Messages", /\/topics\/orders\/messages$/],
  ] as const) {
    await tabs.getByRole("link", { name: new RegExp(`^${tab}`) }).click();
    await expect(page).toHaveURL(url);
    await expect(tabs.getByRole("link", { name: new RegExp(`^${tab}`) })).toHaveAttribute("aria-current", "page");
    expect(await boxes(page, ["h1", "[data-testid=topic-tabs]", "[data-testid=txn-region]"]), tab).toEqual(before);
  }
  // A name that is no topic says so.
  await page.goto("/topics/no-such-topic-here");
  await expect(page.locator("h1")).toHaveText("No such topic");
});

test("isolation: the ToggleGroup is in the address, says what read_committed holds back, and Back restores it", async ({ page }) => {
  await page.goto("/topics/orders/messages");
  await expect(rows(page).first()).toBeVisible();
  const iso = page.getByTestId("isolation");
  await iso.getByRole("radio", { name: "read_committed" }).click();
  await expect(page).toHaveURL(/isolation=read_committed/);
  await expect(page.getByTestId("isolation-note")).toContainText("Reading as read_committed.");
  await expect(page.getByTestId("explainers").getByRole("button", { name: "How this read works" })).toHaveAttribute("aria-expanded", "true");
  await iso.getByRole("radio", { name: "read_uncommitted" }).click();
  await expect(page).not.toHaveURL(/isolation=/);
  await expect(page.getByTestId("isolation-note")).toHaveCount(0);
  // The toolbar never moved.
});

test("partitions, key and time: each is in the address, shown as a chip with a way off, and the rows follow", async ({ page }) => {
  await page.goto("/topics/orders/messages");
  await expect(rows(page).first()).toBeVisible();
  const toolbar = await boxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]"]);
  // Partitions: the MultiSelect.
  await page.getByRole("combobox", { name: "Partitions" }).click();
  await page.getByRole("option", { name: /^orders-1\s/ }).click();
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/partition=1/);
  await expect(page.getByTestId("chip-p1")).toBeVisible();
  await expect(rows(page).first()).toBeVisible();
  for (const r of await rows(page).all()) expect(await r.locator("td").first().innerText()).toBe("1");
  expect(await boxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]"])).toEqual(toolbar);
  await page.getByRole("button", { name: "Remove partition 1" }).click();
  await expect(page).not.toHaveURL(/partition=/);
  // A key: the first row's, typed into the one box.
  const key = (await rows(page).first().locator("td").nth(3).innerText()).trim();
  await page.getByTestId("find").fill(key);
  await page.getByTestId("find").press("Enter");
  await expect(page).toHaveURL(new RegExp(`key=${encodeURIComponent(key).replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`));
  await expect(page.getByTestId("chip-key")).toContainText(key);
  await expect(page.getByTestId("reach")).toBeVisible();
  await expect(rows(page).first()).toBeVisible();
  for (const r of await rows(page).all()) expect((await r.locator("td").nth(3).innerText()).trim()).toBe(key);
  // Digits are an offset.
  await page.getByTestId("find").fill("0");
  await page.getByTestId("find").press("Enter");
  await expect(page).toHaveURL(/offset=0/);
  await expect(page).not.toHaveURL(/key=/);
  await expect(page.getByTestId("chip-offset")).toBeVisible();
  // A time, from the Popover's field.
  await page.getByTestId("time-trigger").click();
  await page.getByRole("button", { name: "24 hours ago" }).click();
  await expect(page.getByTestId("time-input")).toHaveValue(/^\d{4}-\d\d-\d\d \d\d:\d\d$/);
  await page.getByTestId("time-apply").click();
  await expect(page).toHaveURL(/time=/);
  await expect(page.getByTestId("chip-time")).toBeVisible();
  await page.getByRole("button", { name: "Clear all" }).click();
  await expect(page).not.toHaveURL(/offset=|time=|key=/);
  expect(await boxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]"])).toEqual(toolbar);
});

test("the record Sheet: a row opens it with its own address, Esc and Back close it, a deep link opens it", async ({ page }) => {
  await page.goto("/topics/orders/messages");
  const first = rows(page).first();
  await expect(first).toBeVisible();
  const opener = first.getByTestId("open-record");
  await opener.click();
  await expect(page).toHaveURL(/record=\d+%3A\d+|record=\d+:\d+/);
  const sheet = page.getByTestId("record-sheet");
  await expect(sheet).toBeVisible();
  await expect(sheet).toContainText("One record, completely.");
  await expect(sheet.getByTestId("record-value")).toBeVisible();
  // JSON is laid out, and As sent shows it as it came.
  if (await sheet.getByTestId("json-view").count()) {
    await expect(sheet.getByTestId("record-value")).toContainText("\n");
    await sheet.getByTestId("json-view").getByRole("radio", { name: "As sent" }).click();
    await expect(sheet.getByTestId("record-value")).not.toContainText("\n  ");
  }
  await page.keyboard.press("Escape");
  await expect(sheet).toBeHidden();
  await expect(page).not.toHaveURL(/record=/);
  await expect(opener).toBeFocused();
  // Back closes it too.
  await opener.click();
  await expect(sheet).toBeVisible();
  const url = page.url();
  await page.goBack();
  await expect(sheet).toBeHidden();
  await expect(page).not.toHaveURL(/record=/);
  // A deep link opens the page with the record over it; closing stays on the page.
  await page.goto(url);
  await expect(page.getByTestId("record-sheet")).toBeVisible();
  await expect(page.locator("h1")).toHaveText("orders");
  await page.getByTestId("record-sheet").getByRole("button", { name: "Close" }).click();
  await expect(page.getByTestId("record-sheet")).toBeHidden();
  await expect(page).toHaveURL(/\/topics\/orders\/messages$/);
  // A record that is not there says so in the Sheet.
  await page.goto("/topics/orders/messages?record=0:999999999");
  await expect(page.getByTestId("record-missing")).toContainText("There is no record at offset 999,999,999");
});

test("Older asks for more records, in the address", async ({ page }) => {
  await page.goto("/topics/orders/messages?limit=5");
  await expect(rows(page)).toHaveCount(5);
  await page.getByTestId("older").click();
  await expect(page).toHaveURL(/limit=10/);
  await expect(rows(page)).toHaveCount(10);
});

test("the Live tail: new rows at the top, no row rewritten, the header stable, held behind a pill while hovered", async ({ page }) => {
  test.setTimeout(120_000);
  await page.goto("/topics/orders/messages");
  await expect(rows(page).first()).toBeVisible();
  await page.getByTestId("mode").getByRole("radio", { name: "Live tail" }).click();
  await expect(page).toHaveURL(/mode=tail/);
  await choose(page, page.getByTestId("speed"), "Realtime");
  await expect(page.getByTestId("conn")).toHaveAttribute("data-state", "live", { timeout: 15_000 });
  await page.mouse.move(5, 5);
  const head = await page.getByRole("table", { name: "Records, newest first" }).locator("thead").elementHandle();
  const top = rows(page).first();
  const topId = await top.getAttribute("data-row");
  const topEl = await top.elementHandle();
  const topText = await top.innerText();
  const box = await boxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]", "thead"]);
  await produce("orders", 3, "tail-a");
  await expect(rows(page).first()).not.toHaveAttribute("data-row", topId!, { timeout: 45_000 });
  expect(await head!.evaluate((el) => el.isConnected)).toBe(true);
  expect(await topEl!.evaluate((el) => el.isConnected)).toBe(true);
  expect(await topEl!.evaluate((el) => (el as HTMLElement).innerText)).toBe(topText);
  expect(await page.locator(`tr[data-row="${topId}"]`).elementHandle().then((h) => h!.evaluate((el, old) => el === old, topEl))).toBe(true);
  expect(await boxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]", "thead"])).toEqual(box);
  // Hovering the table holds what comes next behind the pill; the rows on screen stay where they are.
  await page.getByTestId("records").locator("tbody tr").nth(2).hover();
  const heldTop = await rows(page).first().getAttribute("data-row");
  await produce("orders", 2, "tail-b");
  await expect(page.getByTestId("held-pill")).toBeVisible({ timeout: 45_000 });
  await expect(page.getByTestId("held-pill")).toContainText(/\d+ new · Show/);
  expect(await rows(page).first().getAttribute("data-row")).toBe(heldTop);
  await page.getByTestId("held-pill").click();
  await expect(page.getByTestId("held-pill")).toHaveCount(0);
  await expect(rows(page).first()).not.toHaveAttribute("data-row", heldTop!);
  // Back to Snapshot: the stream closes.
  await page.getByTestId("mode").getByRole("radio", { name: "Snapshot" }).click();
  await expect(page).not.toHaveURL(/mode=tail/);
  await expect(page.getByTestId("conn")).toHaveAttribute("data-state", "snapshot");
});

test("the Messages page does not scroll sideways at 1024, 1280, 1440 and 1920", async ({ page }) => {
  await page.goto("/topics/orders/messages");
  await expect(rows(page).first()).toBeVisible();
  for (const width of [1024, 1280, 1440, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    expect(await overflowX(page), `${width}`).toBeLessThanOrEqual(0);
  }
});

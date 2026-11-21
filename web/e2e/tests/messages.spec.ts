// A topic's frame and its Messages (slice R1a): tabs as routes; every filter in the address (partitions, offset or key,
// time, isolation, Older); the record Sheet open, close, Esc, Back and a deep link; the Live tail on a broker with traffic.
import { choose, cutHeaders, expect, overflowX, test } from "./fixtures";
import { Kafka, logLevel } from "kafkajs";
import { SEEDED } from "../playwright.config";
import { produce, seeded } from "./kafka";

/** Boxes in page coordinates (the viewport's plus the scroll), so a scroll is not a shift. */
async function pageBoxes(page: import("@playwright/test").Page, selectors: string[]) {
  return page.evaluate(
    (sels) =>
      sels.map((s) => {
        const r = document.querySelector(s)?.getBoundingClientRect();
        return r ? [Math.round(r.x + window.scrollX), Math.round(r.y + window.scrollY), Math.round(r.width), Math.round(r.height)] : null;
      }),
    selectors,
  );
}

const rows = (page: import("@playwright/test").Page) => page.getByRole("table", { name: "Records, newest first" }).locator("tbody tr[data-row]");

test("the frame: one h1, the stats, the transaction region, the tabs as routes (Schema while there is a registry), and a tab switch moves nothing above it", async ({ page }) => {
  await page.goto("/topics/orders");
  await expect(page.locator("h1")).toHaveText("orders");
  await expect(page.locator("h1")).toHaveCount(1);
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText("Topics");
  const strip = page.locator("[data-slot=stat-strip]").first();
  for (const label of ["Partitions", "Retention", "Records", "Size on disk", "Last write"]) await expect(strip).toContainText(label);
  await expect(page.getByTestId("txn-region")).toBeVisible();
  const tabs = page.getByTestId("topic-tabs");
  // The Schema tab is there while the broker has a registry (the seeded one has).
  const registry = (await (await page.request.get("/api/v1/shell", { headers: { "X-Console": "1" } })).json()).schemas !== null;
  await expect(tabs.getByRole("link")).toHaveText([/^Messages$/, /^Log$/, /^Config$/, /^Consumers\s*\d+$/, /^Producers\s*\d+$/, ...(registry ? [/^Schema\s*\d+$/] : [])]);
  const before = await pageBoxes(page, ["h1", "[data-testid=topic-tabs]", "[data-testid=txn-region]"]);
  for (const [tab, url] of [
    ["Log", /\/topics\/orders\/log$/],
    ["Consumers", /\/topics\/orders\/consumers$/],
    ["Producers", /\/topics\/orders\/producers$/],
    ...(registry ? ([["Schema", /\/topics\/orders\/schema$/]] as const) : []),
    ["Messages", /\/topics\/orders\/messages$/],
  ] as const) {
    await tabs.getByRole("link", { name: new RegExp(`^${tab}`) }).click();
    await expect(page).toHaveURL(url);
    await expect(tabs.getByRole("link", { name: new RegExp(`^${tab}`) })).toHaveAttribute("aria-current", "page");
    expect(await pageBoxes(page, ["h1", "[data-testid=topic-tabs]", "[data-testid=txn-region]"]), tab).toEqual(before);
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
  await expect(page.getByTestId("isolation-note")).toContainText("Reading as read_committed:");
  await expect(page.getByTestId("isolation-held")).toHaveText(/^\d[\d,]*$/);
  await expect(page.getByTestId("explainers").getByRole("button", { name: "How this read works" })).toHaveAttribute("aria-expanded", "true");
  await iso.getByRole("radio", { name: "read_uncommitted" }).click();
  await expect(page).not.toHaveURL(/isolation=/);
  await expect(page.getByTestId("isolation-note")).toHaveCount(0);
  // The toolbar never moved.
});

test("partitions, key and time: each is in the address, shown as a chip with a way off, and the rows follow", async ({ page }) => {
  await page.goto("/topics/orders/messages");
  await expect(rows(page).first()).toBeVisible();
  const toolbar = await pageBoxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]"]);
  // Partitions: the MultiSelect.
  await page.getByRole("combobox", { name: "Partitions" }).click();
  await page.getByRole("option", { name: /^orders-1\s/ }).click();
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/partition=1/);
  await expect(page.getByTestId("chip-p1")).toBeVisible();
  await expect(rows(page).first()).toBeVisible();
  // The rows follow (polled: the rows of the read before stay on screen until the new answer replaces them).
  await expect.poll(async () => (await rows(page).locator("td:first-child").allInnerTexts()).every((t) => t.trim() === "1")).toBe(true);
  expect(await pageBoxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]"])).toEqual(toolbar);
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
  await expect.poll(async () => (await rows(page).locator("td:nth-child(4)").allInnerTexts()).every((t) => t.trim() === key)).toBe(true);
  // Digits are an offset.
  await page.getByTestId("find").fill("0");
  await page.getByTestId("find").press("Enter");
  await expect(page).toHaveURL(/offset=0/);
  await expect(page).not.toHaveURL(/key=/);
  await expect(page.getByTestId("chip-offset")).toBeVisible();
  // With no partition chosen, an offset is read from on every partition (kcat -o N without -p): the chip says so and the
  // table holds only what it says (CON-1341).
  await page.getByTestId("find").fill("2");
  await page.getByTestId("find").press("Enter");
  await expect(page.getByTestId("chip-offset")).toHaveText(/from offset 2 in each partition/);
  await expect
    .poll(async () => {
      const cells = await rows(page).evaluateAll((trs) => trs.map((tr) => [tr.querySelector("td:nth-child(1)")?.textContent?.trim(), tr.querySelector("td:nth-child(2)")?.textContent?.trim()]));
      return cells.length > 0 && cells.every(([, o]) => Number(o?.replace(/\D/g, "")) >= 2) && new Set(cells.map(([p]) => p)).size > 1;
    })
    .toBe(true);
  // A time, from the Popover's field.
  await page.getByTestId("time-trigger").click();
  await page.getByRole("button", { name: "24 hours ago" }).click();
  await expect(page.getByTestId("time-input")).toHaveValue(/^\d{4}-\d\d-\d\d \d\d:\d\d$/);
  await page.getByTestId("time-apply").click();
  await expect(page).toHaveURL(/time=/);
  await expect(page.getByTestId("chip-time")).toBeVisible();
  await page.getByRole("button", { name: "Clear all" }).click();
  await expect(page).not.toHaveURL(/offset=|time=|key=/);
  expect(await pageBoxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]"])).toEqual(toolbar);
});

test("From time has a UTC calendar: typing moves it, a day and a time chosen on it write the field (J387)", async ({ page }) => {
  await page.goto("/topics/orders/messages");
  await page.getByTestId("time-trigger").click();
  await page.getByTestId("time-input").fill("2026-11-13 14:30");
  const cal = page.getByTestId("datetime-calendar");
  await expect(cal).toContainText("November 2026");
  await expect(cal.getByRole("button", { name: "13 November 2026", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(cal.getByTestId("datetime-clock")).toHaveValue("14:30:00");
  // A day with the mouse, then the time with the keyboard.
  await cal.getByRole("button", { name: "Previous month" }).click();
  await expect(cal).toContainText("October 2026");
  await cal.getByRole("button", { name: "2 October 2026", exact: true }).click();
  await expect(page.getByTestId("time-input")).toHaveValue("2026-10-02 14:30:00");
  await cal.getByTestId("datetime-clock").fill("09:05");
  await expect(page.getByTestId("time-input")).toHaveValue("2026-10-02 09:05:00");
  await page.getByTestId("time-apply").click();
  await expect(page).toHaveURL(/time=2026-10-02(\+|%20| )09(%3A|:)05(%3A|:)00/);
  await expect(page.getByTestId("chip-time")).toContainText("from 2026-10-02 09:05:00");
  // Opened again, the calendar shows what is applied.
  await page.getByTestId("time-trigger").click();
  await expect(page.getByTestId("datetime-calendar").getByRole("button", { name: "2 October 2026", exact: true })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
});

test("a value the table cut short says so on hover, and the record Sheet has all of it (J385)", async ({ page, browserName }) => {
  test.skip(!seeded, "writes its own record");
  const topic = `long-${browserName}-${Date.now()}`;
  const kafka = new Kafka({ clientId: "e2e-long", brokers: [SEEDED.kafka], logLevel: logLevel.NOTHING });
  const admin = kafka.admin();
  await admin.connect();
  await admin.createTopics({ topics: [{ topic, numPartitions: 1 }], waitForLeaders: true });
  await admin.disconnect();
  const p = kafka.producer({ allowAutoTopicCreation: false });
  await p.connect();
  const value = `start-${"x".repeat(3000)}-end`;
  await p.send({ topic, messages: [{ key: "k", value }] });
  await p.disconnect();
  await page.goto(`/topics/${topic}/messages`);
  const cell = rows(page).first().locator("td").nth(4);
  await expect(cell).toHaveAttribute("title", /^start-x{400,} … \(2\.9 KiB in all: open the record for the whole value\)$/);
  await rows(page).first().click();
  await expect(page.getByRole("dialog")).toContainText("-end");
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
    expect(await sheet.getByTestId("record-value").textContent()).toContain("\n  ");
    await sheet.getByTestId("json-view").getByRole("radio", { name: "As sent" }).click();
    await expect(sheet.getByTestId("json-view").getByRole("radio", { name: "As sent" })).toHaveAttribute("aria-checked", "true");
    expect(await sheet.getByTestId("record-value").textContent()).not.toContain("\n  ");
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

test("Older past 200 records reads before where the page stopped, in the address, down to the first record (J27)", async ({ page, browserName }) => {
  test.skip(!seeded, "makes its own topic");
  const topic = `older-${browserName}-${Date.now()}`;
  const kafka = new Kafka({ clientId: "e2e-older", brokers: [SEEDED.kafka], logLevel: logLevel.NOTHING });
  const admin = kafka.admin();
  await admin.connect();
  await admin.createTopics({ topics: [{ topic, numPartitions: 1 }], waitForLeaders: true });
  await admin.disconnect();
  await produce(topic, 260, "older");
  await page.goto(`/topics/${topic}/messages?limit=200`);
  await expect(rows(page)).toHaveCount(200);
  await page.getByTestId("older").click();
  await expect(page).toHaveURL(/before=0(%3A|:)60/);
  await expect(rows(page)).toHaveCount(60);
  await expect(page.getByTestId("chip-before")).toContainText("before 0:60");
  await expect(page.getByTestId("older")).toHaveCount(0);
  await expect(page.getByTestId("pager")).toContainText("Older records, newest first · 60 records");
  // The way off the chip: the newest again.
  await page.getByTestId("chip-before").getByRole("button").click();
  await expect(page).not.toHaveURL(/before=/);
  await expect(rows(page)).toHaveCount(200);
});

test("the Live tail: new rows at the top, no row rewritten, the header stable, held behind a pill while hovered", async ({ page, browserName }) => {
  test.setTimeout(120_000);
  await page.goto("/topics/orders/messages");
  await expect(rows(page).first()).toBeVisible();
  await page.getByTestId("mode").getByRole("radio", { name: "Live tail" }).click();
  await expect(page).toHaveURL(/mode=tail/);
  await choose(page, page.getByTestId("speed"), "Realtime · 1 s");
  await expect(page.getByTestId("conn")).toHaveAttribute("data-state", "live", { timeout: 15_000 });
  await page.mouse.move(5, 5);
  const head = await page.getByRole("table", { name: "Records, newest first" }).locator("thead").elementHandle();
  const top = rows(page).first();
  const topId = await top.getAttribute("data-row");
  const topEl = await top.elementHandle();
  const topText = await top.innerText();
  const box = await pageBoxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]", "thead"]);
  // Layout shift (Chromium): the prepended rows shift nothing on screen (CON-1350).
  await page.evaluate(() => {
    const w = window as unknown as { __ls: number };
    w.__ls = 0;
    try {
      new PerformanceObserver((l) => {
        for (const e of l.getEntries() as unknown as { value: number; hadRecentInput: boolean }[]) if (!e.hadRecentInput) w.__ls += e.value;
      }).observe({ type: "layout-shift", buffered: false });
    } catch {
      w.__ls = -1;
    }
  });
  const recordsTile = page.locator("[data-slot=stat]", { hasText: "Records" }).locator("[data-slot=stat-value]");
  const recordsBefore = Number((await recordsTile.innerText()).replace(/\D/g, ""));
  await produce("orders", 3, "tail-a");
  await expect(rows(page).first()).not.toHaveAttribute("data-row", topId!, { timeout: 45_000 });
  // The figures above follow the tail's batches (CON-1346): Records counts the new rows, in the same box.
  await expect.poll(async () => Number((await recordsTile.innerText()).replace(/\D/g, "")), { timeout: 15_000 }).toBeGreaterThanOrEqual(recordsBefore + 3);
  expect(await head!.evaluate((el) => el.isConnected)).toBe(true);
  expect(await topEl!.evaluate((el) => el.isConnected)).toBe(true);
  expect(await topEl!.evaluate((el) => (el as HTMLElement).innerText)).toBe(topText);
  expect(await page.locator(`tr[data-row="${topId}"]`).elementHandle().then((h) => h!.evaluate((el, old) => el === old, topEl))).toBe(true);
  expect(await pageBoxes(page, ["[data-testid=toolbar]", "[data-testid=filter-line]", "thead"])).toEqual(box);
  if (browserName === "chromium") expect(await page.evaluate(() => (window as unknown as { __ls: number }).__ls), "layout shift while tailing at the top").toBeLessThan(0.01);
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

test("the Messages page does not scroll sideways at 1024, 1280, 1440 and 1920, and no header is cut from 1280", async ({ page }) => {
  await page.goto("/topics/orders/messages");
  await expect(rows(page).first()).toBeVisible();
  for (const width of [1024, 1280, 1440, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    await page.waitForTimeout(150);
    expect(await overflowX(page), `${width}`).toBeLessThanOrEqual(0);
    if (width >= 1280) expect(await cutHeaders(page), `headers cut at ${width}`).toEqual([]);
  }
  for (const path of ["/topics/orders/partitions", "/topics/orders/log", "/topics/orders/config", "/topics/orders/consumers", "/topics/orders/producers"]) {
    for (const width of [1280, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(path);
      await expect(page.locator("h1")).toBeVisible();
      await page.waitForLoadState("networkidle");
      expect(await cutHeaders(page), `headers cut on ${path} at ${width}`).toEqual([]);
    }
  }
});

test("Messages has one way to keep up: no page-wide interval, a Refresh beside the switch in Snapshot, none in Live tail; the wire dock is under the records", async ({ page }) => {
  await page.goto("/topics/orders/messages");
  await expect(rows(page).first()).toBeVisible();
  await expect(page.getByTestId("refresh-interval")).toHaveCount(0);
  const line = page.getByTestId("filter-line");
  await expect(line.getByTestId("refresh-now")).toBeVisible();
  const stamp = line.getByTestId("refresh-stamp");
  const first = await stamp.textContent();
  await page.waitForTimeout(1100);
  await line.getByTestId("refresh-now").click();
  await expect(stamp).not.toHaveText(first ?? "");
  // The board's "On the wire": the tape filtered to this topic.
  const dock = page.getByTestId("wire-dock");
  await expect(dock).toHaveAttribute("data-topic", "orders");
  await expect(dock.getByRole("heading", { name: "On the wire" })).toBeVisible();
  await page.getByTestId("mode").getByRole("radio", { name: "Live tail" }).click();
  await expect(line.getByTestId("refresh-now")).toHaveCount(0);
  // Another tab of the same topic keeps the page-wide control.
  await page.goto("/topics/orders/partitions");
  await expect(page.getByTestId("refresh-interval")).toBeVisible();
});

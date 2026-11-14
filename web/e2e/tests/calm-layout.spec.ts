// The layout rules the UX audit of 2026-11-13 checked by eye, as assertions (CON-1355 to CON-1357): a dialog's actions are
// always in view; the transaction region has one height whether a transaction is open or not; a strip of figures is
// balanced with its values at one height; the topic's head leaves its records in view at 1024 x 900; the Live tape keeps an
// API name whole at 1024 with the sidebar open.
import type { Page } from "@playwright/test";
import { Kafka, logLevel } from "kafkajs";
import { expect, test } from "./fixtures";
import { kafkaAddress } from "./r4";

const DIALOGS = ["/lab?new=slow", "/topics/orders?dialog=produce", "/access?acl=new", "/groups/fulfilment?dialog=reset", "/settings?edit=log.segment.bytes", "/topics/orders/partitions?dialog=delete-records&partition=0"];

test("every dialog keeps its actions in view at 1024 x 900 and 1440 x 900 (a sticky footer)", async ({ page }) => {
  for (const width of [1024, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    for (const url of DIALOGS) {
      await page.goto(url);
      const footer = page.locator("[role=dialog] [data-slot=dialog-footer]").last();
      await expect(footer, url).toBeVisible();
      await page.waitForTimeout(300);
      const box = (await footer.boundingBox())!;
      expect(box.y, `${url} at ${width}`).toBeGreaterThanOrEqual(0);
      expect(box.y + box.height, `${url} at ${width}`).toBeLessThanOrEqual(900);
      for (const b of await footer.getByRole("button").all()) await expect(b).toBeInViewport();
    }
  }
});

async function regionHeight(page: Page) {
  return page.getByTestId("txn-region").evaluate((el) => Math.round(el.getBoundingClientRect().height));
}

test("the transaction region is one height, open or calm, at 1024 and 1440 (CON-807)", async ({ page, browserName }) => {
  const topic = `calm-txn-${browserName}`;
  const kafka = new Kafka({ clientId: `calm-${browserName}`, brokers: [await kafkaAddress(page.request)], logLevel: logLevel.NOTHING });
  const admin = kafka.admin();
  await admin.connect();
  await admin.createTopics({ topics: [{ topic, numPartitions: 2 }] }).catch(() => false);
  await admin.disconnect();
  const producer = kafka.producer({ transactionalId: `calm-${browserName}-${Date.now()}`, maxInFlightRequests: 1, idempotent: true });
  await producer.connect();
  const heights: Record<string, number[]> = { calm: [], open: [] };
  const tx = await producer.transaction();
  try {
    await tx.send({ topic, messages: [{ key: "a", value: "1" }, { key: "b", value: "2" }] });
    for (const width of [1024, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(`/topics/${topic}/messages`);
      await expect(page.getByTestId("txn-region")).toHaveAttribute("data-state", "open");
      heights.open!.push(await regionHeight(page));
      await expect(page.getByTestId("txn-region")).toContainText("Open the transaction");
    }
  } finally {
    await tx.abort();
    await producer.disconnect();
  }
  for (const width of [1024, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`/topics/${topic}/messages`);
    await expect(page.getByTestId("txn-region")).toHaveAttribute("data-state", "calm");
    heights.calm!.push(await regionHeight(page));
  }
  expect(heights.open).toEqual(heights.calm);
});

test("stat strips are balanced, their values at one height, and the topic's records show at 1024 x 900", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 900 });
  // The sidebar open (the person's choice) leaves the least room: the hardest case.
  await page.addInitScript(() => localStorage.setItem("doze:sidebar", "open"));
  for (const url of ["/topics", "/groups/fulfilment", "/topics/orders/messages", "/live", "/"]) {
    await page.goto(url);
    const strip = page.locator("[data-slot=stat-strip]").first();
    await expect(strip, url).toBeVisible();
    const rows = (await strip.getAttribute("data-rows"))!.split("+").map(Number);
    // No lone tile under a fuller row.
    if (rows.length > 1) expect(Math.min(...rows), `${url}: ${rows.join("+")}`).toBeGreaterThanOrEqual(Math.max(...rows) - 1);
    if (rows.length > 1) expect(rows[rows.length - 1], `${url}: ${rows.join("+")}`).toBeGreaterThan(1);
    // The values of the first row sit at one height, whatever their labels' lines.
    const tops = await strip.locator("[data-slot=stat-value]").evaluateAll((els, n) => els.slice(0, n).map((e) => Math.round(e.getBoundingClientRect().top)), rows[0]!);
    expect(new Set(tops).size, `${url}: ${tops.join(",")}`).toBe(1);
  }
  await page.evaluate(() => localStorage.removeItem("doze:sidebar"));
  await page.goto("/topics/orders/messages");
  const first = page.getByRole("table", { name: "Records, newest first" }).locator("tbody tr[data-row]").first();
  await expect(first).toBeVisible();
  const top = await first.evaluate((el) => el.getBoundingClientRect().top);
  expect(top, "the first record is on screen without scrolling").toBeLessThan(900);
});

test("the Live tape keeps an API name whole at 1024 with the sidebar open", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 900 });
  await page.addInitScript(() => localStorage.setItem("doze:sidebar", "open"));
  await page.goto("/live");
  const names = page.getByTestId("tape").locator("tbody tr[data-row] td:nth-child(3) button");
  await expect(names.first()).toBeVisible();
  const cut = await names.evaluateAll((els) => els.filter((e) => e.scrollWidth > e.clientWidth + 1).map((e) => e.textContent));
  expect(cut).toEqual([]);
});

test("a tab's count has a width of its own: a count that grows moves no tab", async ({ page }) => {
  await page.goto("/topics/orders/messages");
  const badge = page.getByTestId("topic-tabs").locator("a[data-tab=consumers] span").first();
  await expect(badge).toBeVisible();
  const min = await badge.evaluate((el) => parseFloat(getComputedStyle(el).minWidth));
  const five = await badge.evaluate((el) => {
    const probe = el.cloneNode() as HTMLElement;
    probe.textContent = "99,999";
    probe.style.minWidth = "0";
    el.parentElement!.appendChild(probe);
    const w = probe.getBoundingClientRect().width;
    probe.remove();
    return w;
  });
  expect(min).toBeGreaterThan(0);
  expect(min + 0.5).toBeGreaterThanOrEqual(five - 12);
});

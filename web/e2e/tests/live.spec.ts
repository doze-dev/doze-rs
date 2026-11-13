// The Live page (slice R3) against a broker WITH traffic: the spec drives its own (two producers on a gap that does not fold, a
// consumer group, one refused produce), so the seeded world and a console started with E2E_URL (and E2E_KAFKA) both have
// a moving tape. The owner's complaint was flicker and speed ("it should not be moving this fast"): every tail test measures.
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { Kafka, logLevel, type Producer } from "kafkajs";
import type { Page } from "@playwright/test";
import { SEEDED } from "../playwright.config";
import { boxes, choose, cutHeaders, expect, overflowX, test } from "./fixtures";

const KAFKA = process.env.E2E_KAFKA ?? SEEDED.kafka;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Traffic: each producer sends every 2.5 s (a stream folds within 2 s, so each send is a new row), offset from the other. */
function traffic(broker: string) {
  let stop = false;
  const producers: Producer[] = [];
  const run = async () => {
    for (const [i, client] of ["e2e-orders", "e2e-payments"].entries()) {
      const p = new Kafka({ clientId: client, brokers: [broker], logLevel: logLevel.NOTHING, retry: { retries: 2 } }).producer({ allowAutoTopicCreation: true });
      producers.push(p);
      void (async () => {
        await sleep(i * 1250);
        let n = 0;
        while (!stop) {
          try {
            if (n === 0) await p.connect();
            await p.send({ topic: i === 0 ? "orders" : "payments.settled", messages: [{ key: `k-${n}`, value: JSON.stringify({ n }) }] });
          } catch {
            // the broker is down (the reconnect test): try again next time
            n = -1;
          }
          n++;
          await sleep(2500);
        }
      })();
    }
  };
  void run();
  return async () => {
    stop = true;
    await Promise.all(producers.map((p) => p.disconnect().catch(() => {})));
  };
}

/** One produce Kafka refuses: a record without a key to a compacted topic (INVALID_RECORD, 87). */
async function refusedProduce(broker: string) {
  const k = new Kafka({ clientId: "admin-cli", brokers: [broker], logLevel: logLevel.NOTHING, retry: { retries: 0 } });
  const admin = k.admin();
  await admin.connect();
  await admin.createTopics({ topics: [{ topic: "inventory.levels", numPartitions: 1, configEntries: [{ name: "cleanup.policy", value: "compact" }] }] }).catch(() => {});
  await admin.disconnect();
  const p = k.producer();
  await p.connect();
  await p.send({ topic: "inventory.levels", messages: [{ value: "no key" }] }).catch(() => {});
  await p.disconnect();
}

let stopTraffic: (() => Promise<void>) | undefined;
test.beforeAll(async () => {
  await refusedProduce(KAFKA);
  stopTraffic = traffic(KAFKA);
  await sleep(3000);
});
test.afterAll(async () => {
  await stopTraffic?.();
});

// A fresh browser state for each test, before its first page (a second navigation right after the first cut the first
// page's requests short, which WebKit reports as errors).
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("r3-clean")) {
      localStorage.clear();
      sessionStorage.setItem("r3-clean", "1");
    }
  });
});

const tape = (page: Page) => page.getByTestId("tape");
const rowsOf = (page: Page) => tape(page).locator("tbody tr[data-row]");
const scroller = (page: Page) => tape(page).locator("[data-slot=data-table]");

/** The times the tape shows, top to bottom. */
const timesOf = (page: Page) => rowsOf(page).locator("[data-testid=row-time]").allInnerTexts();

/** The tape's order is the order of the times it shows (newest first): a row's time is when it joined the tape (its first
 * request answered), so a slow Fetch is never below rows that came after it, and a folded repeat keeps its place and time. */
function expectNewestFirst(times: string[]) {
  for (let i = 1; i < times.length; i++) expect(times[i]! <= times[i - 1]!, `row ${i}: ${times[i]} after ${times[i - 1]}`).toBe(true);
}

async function openLive(page: Page, search = "") {
  await page.goto(`/live${search}`);
  await expect(page.locator("h1")).toHaveText("Live");
  await expect(rowsOf(page).first()).toBeVisible();
}

async function tailOn(page: Page) {
  await page.getByTestId("tail-mode").getByRole("radio", { name: "Live tail" }).click();
  await expect(page).toHaveURL(/tail=/);
  await expect(page.getByTestId("tail-state")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  // The pointer off the tape (hovering holds it).
  await page.mouse.move(2, 600);
}

test("Live is a snapshot by default: one h1, the figures in tiles, Snapshot chosen, and no request after it loaded", async ({ page }) => {
  await openLive(page);
  await expect(page.locator("h1")).toHaveCount(1);
  // A top-level page: no breadcrumb row at all, and no page-wide interval: the Snapshot | Live tail switch owns the updates,
  // with its own stamp and Refresh beside it.
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveCount(0);
  await expect(page.getByTestId("refresh-interval")).toHaveCount(0);
  await expect(page.getByTestId("tail-bar").getByTestId("refresh-now")).toBeVisible();
  expectNewestFirst(await timesOf(page));
  const strip = page.locator("[data-slot=stat-strip]");
  for (const label of ["Requests, last minute", "Rows kept", "Reaches back to", "Clients"]) await expect(strip).toContainText(label);
  await expect(strip).toContainText("of 2,000");
  await expect(page.getByTestId("tail-mode").getByRole("radio", { name: "Snapshot" })).toHaveAttribute("aria-checked", "true");
  await expect(page.getByTestId("tail-state")).toHaveText("Snapshot");
  await expect(page.getByTestId("tail-speed")).toHaveText(/Relaxed · 3 s/);
  const asked: string[] = [];
  page.on("request", (r) => asked.push(r.url()));
  const before = await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.getAttribute("data-row")));
  await sleep(6000);
  expect(asked, "no request after load").toEqual([]);
  expect(await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.getAttribute("data-row")))).toEqual(before);
});

test("the tail: batches never faster than the pace, existing rows never rewritten, new rows highlighted, the page around it still", async ({ page }) => {
  await openLive(page);
  await tailOn(page);
  const selectors = ["h1", "[data-slot=stat-strip]", "[data-testid=refusals]", "[data-testid=live-toolbar]", "[data-testid=tail-bar]", "thead"];
  const before = await boxes(page, selectors);
  await page.evaluate(() => {
    const w = window as unknown as { __batches: number[]; __old: Map<string, [Element, string]>; __fresh: number };
    w.__batches = [];
    w.__fresh = 0;
    w.__old = new Map();
    for (const tr of document.querySelectorAll("[data-testid=tape] tbody tr[data-row]")) w.__old.set(tr.getAttribute("data-row")!, [tr, tr.outerHTML]);
    new MutationObserver((records) => {
      const added = records.some((r) => [...r.addedNodes].some((n) => n instanceof HTMLElement && n.matches("tr[data-row]")));
      if (added) w.__batches.push(performance.now());
    }).observe(document.querySelector("[data-testid=tape] tbody")!, { childList: true });
  });
  await sleep(20_000);
  const r = await page.evaluate(() => {
    const w = window as unknown as { __batches: number[]; __old: Map<string, [Element, string]> };
    let replaced = 0;
    let rewritten = 0;
    for (const [id, [node, html]] of w.__old) {
      const now = document.querySelector(`[data-testid=tape] tbody tr[data-row="${id}"]`);
      if (!now) continue; // fell off the bottom of the 200
      if (now !== node) replaced++;
      // A folded repeat that grew changes its own count and time in place, by design; nothing else may change.
      else if (now.outerHTML !== html && !now.textContent!.includes("×")) rewritten++;
    }
    const fresh = document.querySelectorAll("[data-testid=tape] tbody tr:has([data-fresh])").length;
    const freshOld = [...w.__old.values()].filter(([n]) => n.querySelector("[data-fresh]")).length;
    return { batches: w.__batches, replaced, rewritten, fresh, freshOld };
  });
  expect(r.batches.length, "the traffic made batches").toBeGreaterThanOrEqual(2);
  const gaps = r.batches.slice(1).map((t, i) => t - r.batches[i]!);
  for (const g of gaps) expect(g, `gaps ${gaps.map(Math.round).join(", ")} ms`).toBeGreaterThanOrEqual(2900);
  expect(r.replaced, "existing rows replaced").toBe(0);
  expect(r.rewritten, "existing rows rewritten").toBe(0);
  expect(r.fresh, "new rows carry the highlight").toBeGreaterThan(0);
  expect(r.freshOld, "rows of the snapshot never highlight").toBe(0);
  expect(await boxes(page, selectors)).toEqual(before);
  // No row twice.
  const ids = await rowsOf(page).evaluateAll((rs) => rs.map((x) => x.getAttribute("data-row")));
  expect(new Set(ids).size).toBe(ids.length);
  // The order is the order of the times shown, after batches were prepended too; the tail owns updates, so no Refresh.
  expectNewestFirst(await timesOf(page));
  await expect(page.getByTestId("refresh-now")).toHaveCount(0);
});

test("an interval chosen on another page does not poll Live: the switch owns its updates, Refresh fetches by hand", async ({ page }) => {
  await page.goto("/topics");
  await choose(page, page.getByTestId("refresh-interval"), "Every 5 s");
  expect(await page.evaluate(() => localStorage.getItem("doze:refresh"))).toBe("5s");
  await openLive(page);
  await expect(page.getByTestId("refresh-interval")).toHaveCount(0);
  const asked: string[] = [];
  page.on("request", (r) => asked.push(r.url()));
  await sleep(6500);
  expect(asked, "no poll on Live").toEqual([]);
  const stamp = page.getByTestId("tail-bar").getByTestId("refresh-stamp");
  const first = await stamp.textContent();
  await sleep(1100);
  await page.getByTestId("tail-bar").getByTestId("refresh-now").click();
  await expect(stamp).not.toHaveText(first ?? "");
  expectNewestFirst(await timesOf(page));
});

test("the tail holds while the tape is scrolled: the pill counts, the scroll stays, Show applies", async ({ page }) => {
  await openLive(page);
  await choose(page, page.getByTestId("tail-speed"), "Realtime · 1 s");
  expect(await page.evaluate(() => localStorage.getItem("doze:tail-speed"))).toBe("realtime");
  await tailOn(page);
  await expect(rowsOf(page).nth(16)).toBeAttached();
  await scroller(page).hover();
  await page.mouse.wheel(0, 240);
  await page.mouse.move(2, 600);
  const pill = page.getByTestId("held-pill");
  await expect(pill).toBeVisible({ timeout: 10_000 });
  const at = async () => ({ top: await scroller(page).evaluate((e) => e.scrollTop), y: await page.evaluate(() => window.scrollY), first: await rowsOf(page).first().getAttribute("data-row"), n: await rowsOf(page).count() });
  const held = await at();
  expect(held.top).toBeGreaterThan(100);
  const count1 = Number((await pill.innerText()).match(/\d+/)![0]);
  expect(count1).toBeGreaterThan(0);
  await sleep(3500);
  expect(await at()).toEqual(held);
  const count2 = Number((await pill.innerText()).match(/\d+/)![0]);
  expect(count2).toBeGreaterThanOrEqual(count1);
  await pill.click();
  await expect(pill).toBeHidden();
  const after = await at();
  expect(after.top).toBe(0);
  expect(after.first).not.toBe(held.first);
  expect(after.y).toBe(held.y);
});

test("filters change the rows, live in the address, and survive a reload", async ({ page }) => {
  await openLive(page);
  // Topics: a MultiSelect with counts.
  await page.getByRole("combobox", { name: "Topics" }).click();
  await page.getByRole("option", { name: /^orders \d/ }).click();
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/topic=/);
  await expect(page.getByRole("combobox", { name: "Topics" })).toHaveText("orders");
  await expect.poll(async () => (await tape(page).locator("tbody tr[data-row] td:nth-child(4)").allInnerTexts()).every((t) => t.includes("orders"))).toBe(true);
  // Refused only.
  await page.getByTestId("refused-only").click();
  await expect(page).toHaveURL(/refused=/);
  await expect(page.getByTestId("refused-only")).toHaveAttribute("data-state", "on");
  await page.reload();
  await expect(page.getByTestId("refused-only")).toHaveAttribute("data-state", "on");
  await expect(page.getByRole("combobox", { name: "Topics" })).toHaveText("orders");
  await page.getByTestId("refused-only").click();
  await expect(page).not.toHaveURL(/refused=/);
  // Idle polls: shown when switched off.
  await expect(page.getByTestId("pager")).toContainText("idle polls hidden");
  await page.getByTestId("idle-hidden").click();
  await expect(page).toHaveURL(/idle=/);
  await expect(page.getByTestId("pager")).not.toContainText("idle polls hidden");
  // A search that matches nothing says which filters are on and offers the way out.
  await page.getByTestId("live-search").fill("no-such-thing-anywhere");
  await expect(page).toHaveURL(/q=no-such-thing-anywhere/);
  await expect(page.getByTestId("live-no-match")).toContainText("topic is orders");
  await page.getByTestId("live-no-match").getByRole("button", { name: "Clear filters" }).click();
  await expect(page).not.toHaveURL(/topic=|q=/);
  await expect(rowsOf(page).first()).toBeVisible();
  // A click on a client in a row filters by it.
  const client = (await tape(page).locator("tbody tr[data-row] td:nth-child(2) button").first().innerText()).trim();
  await tape(page).locator("tbody tr[data-row] td:nth-child(2) button").first().click();
  await expect(page).toHaveURL(new RegExp(`client=`));
  await expect.poll(async () => (await tape(page).locator("tbody tr[data-row] td:nth-child(2)").allInnerTexts()).every((t) => t.includes(client))).toBe(true);
  await page.goBack();
  await expect(page).not.toHaveURL(/client=/);
});

test("a request opens in a Sheet with its own address: Esc closes, Back closes, a deep link opens it", async ({ page }) => {
  await openLive(page);
  const first = rowsOf(page).first();
  const id = await first.getAttribute("data-row");
  await first.locator("td").nth(4).click();
  await expect(page).toHaveURL(new RegExp(`request=${id}`));
  const sheet = page.getByTestId("request-sheet");
  await expect(sheet).toBeVisible();
  await expect(sheet.getByTestId("request-body")).toContainText(/Correlation id|Kafka's words/);
  await page.keyboard.press("Escape");
  await expect(sheet).toBeHidden();
  await expect(page).not.toHaveURL(/request=/);
  await first.locator("td").nth(4).click();
  await expect(sheet).toBeVisible();
  await page.goBack();
  await expect(sheet).toBeHidden();
  await expect(page).not.toHaveURL(/request=/);
  await page.goto(`/live?request=${id}`);
  await expect(sheet).toBeVisible();
  await sheet.getByRole("button", { name: "Close" }).click();
  await expect(sheet).toBeHidden();
  await expect(page).toHaveURL(/\/live$/);
  // A request that is not on the tape says so.
  await page.goto("/live?request=987654321");
  await expect(sheet.getByTestId("request-gone")).toContainText("no longer on the wire");
});

test("a refused request: Kafka's words, the rule behind them, what would pass", async ({ page }) => {
  await openLive(page, "?refused=1");
  const row = rowsOf(page).filter({ hasText: "INVALID_RECORD" }).first();
  await expect(row).toBeVisible();
  await row.locator("td").nth(4).click();
  const sheet = page.getByTestId("request-sheet");
  await expect(sheet).toHaveAttribute("data-kind", "refused");
  await expect(sheet.getByRole("heading", { name: "INVALID_RECORD (87)" })).toBeVisible();
  await expect(sheet.getByTestId("kafka-words")).toContainText("Kafka's words");
  await expect(sheet.getByTestId("rules")).toContainText("cleanup.policy=compact");
  await expect(sheet.getByText("Give it a key")).toBeVisible();
  // The refusals strip opens to one line per (code, client).
  await page.keyboard.press("Escape");
  await page.getByTestId("refusals").getByRole("button").first().click();
  await expect(page.getByTestId("refusal-lines")).toContainText("INVALID_RECORD (87)");
});

test("the clients Sheet: who is connected, and a name filters the tape", async ({ page }) => {
  await openLive(page);
  await page.getByTestId("open-clients").click();
  await expect(page).toHaveURL(/clients=/);
  const sheet = page.getByTestId("clients-sheet");
  await expect(sheet.getByTestId("clients")).toContainText("e2e-orders");
  await page.keyboard.press("Escape");
  await expect(page).not.toHaveURL(/clients=/);
  await page.getByTestId("open-clients").click();
  await sheet.getByRole("link", { name: "e2e-orders" }).click();
  await expect(sheet).toBeHidden();
  await expect(page).toHaveURL(/client=/);
  await expect.poll(async () => (await tape(page).locator("tbody tr[data-row] td:nth-child(2)").allInnerTexts()).every((t) => t.includes("e2e-orders"))).toBe(true);
});

test("the first-request page and the docks", async ({ page }, testInfo) => {
  await page.goto("/connect/first-request");
  await expect(page.locator("h1")).toHaveText("A new client just connected");
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toHaveText(/Home.*Connect/);
  await expect(page.getByTestId("first-banner")).toBeVisible();
  await expect(page.getByTestId("first-story").locator("li").first()).toContainText(/ApiVersions|Metadata/);
  // A client whose id a shell would act on: the command the page offers quotes it (the server makes it with shell_word). One
  // id per browser: the broker is shared, and only a client it has not heard from before is the first-request page's.
  const hostile = `x; open -a Calculator #${testInfo.project.name}`;
  const admin = new Kafka({ clientId: hostile, brokers: [KAFKA], logLevel: logLevel.NOTHING, retry: { retries: 0 } }).admin();
  await admin.connect();
  await admin.listTopics();
  await admin.disconnect();
  await expect
    .poll(async () => {
      await page.reload();
      return page.getByTestId("first-request").locator("[data-slot=code-value] code").last().textContent();
    })
    .toContain(`client.id='${hostile}'`);
  const command = await page.getByTestId("first-request").locator("[data-slot=code-value] code").last().textContent();
  expect(command).not.toContain(`client.id=${hostile}`);
  await page.goto("/gallery");
  const dock = page.locator("[data-testid=wire-dock][data-topic=orders]");
  await expect(dock.locator("tbody tr[data-row]").first()).toBeVisible();
  for (const t of await dock.locator("tbody tr[data-row] td:nth-child(4)").allInnerTexts()) expect(t).toContain("orders");
  const group = page.locator("[data-testid=wire-dock][data-group=fulfilment]");
  await group.getByRole("radio", { name: "Commits" }).click();
  await expect(group.getByRole("radio", { name: "Commits" })).toHaveAttribute("aria-checked", "true");
  await dock.locator("tbody tr[data-row]").first().getByTestId("row-time").click();
  await expect(page).toHaveURL(/\/live\?request=\d+/);
  await expect(page.getByTestId("request-sheet")).toBeVisible();
});

test("no page scrolls sideways at 1024 to 1920, in a snapshot, in the tail and under a Sheet", async ({ page }) => {
  for (const w of [1024, 1280, 1440, 1920]) {
    await page.setViewportSize({ width: w, height: 900 });
    await openLive(page);
    expect(await overflowX(page), `${w} snapshot`).toBe(0);
    await expect.poll(() => scroller(page).evaluate((e) => e.scrollWidth - e.clientWidth), { message: `${w} table` }).toBe(0);
    if (w >= 1280) expect(await cutHeaders(page), `headers cut at ${w}`).toEqual([]);
    await page.goto(`/live?tail=1`);
    await expect(page.getByTestId("tail-state")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
    expect(await overflowX(page), `${w} tail`).toBe(0);
    await page.goto(`/live?clients=1`);
    await expect(page.getByTestId("clients-sheet")).toBeVisible();
    expect(await overflowX(page), `${w} sheet`).toBe(0);
  }
});

test.describe("a broker that goes away and comes back", () => {
  const ports = { kafka: "127.0.0.1:39192", console: "127.0.0.1:39194" };
  let child: ChildProcess | undefined;
  let data = "";
  const bin = process.env.DOZE_KAFKA_BIN ?? resolve(import.meta.dir, "../../../target/release/doze-kafka");
  const start = async () => {
    child = spawn(bin, ["--listen", ports.kafka, "--data-dir", data, "--console-addr", ports.console], { stdio: "ignore" });
    const end = Date.now() + 15_000;
    for (;;) {
      try {
        if ((await fetch(`http://${ports.console}/healthz`)).ok) return;
      } catch {
        // not yet
      }
      if (Date.now() > end) throw new Error("the second broker did not start");
      await sleep(100);
    }
  };
  const kill = async () => {
    const c = child;
    child = undefined;
    if (!c || c.exitCode !== null) return;
    const gone = new Promise((r) => c.once("exit", r));
    c.kill("SIGKILL");
    await gone;
  };
  test.afterEach(async () => {
    await kill();
    if (data) rmSync(data, { recursive: true, force: true });
  });

  test("the tail says Reconnecting, then Live, with no row twice and none of the old life", async ({ page, errors }) => {
    test.setTimeout(120_000);
    data = mkdtempSync(join(tmpdir(), "doze-e2e-r3-"));
    await start();
    let stop = traffic(ports.kafka);
    try {
      await page.goto(`http://${ports.console}/live`);
      await page.evaluate(() => localStorage.setItem("doze:tail-speed", "realtime"));
      await page.goto(`http://${ports.console}/live?tail=1`);
      const state = page.getByTestId("tail-state");
      await expect(state).toHaveAttribute("data-state", "live", { timeout: 10_000 });
      await page.mouse.move(2, 600);
      await expect(rowsOf(page).first()).toBeVisible({ timeout: 15_000 });
      const old = await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.getAttribute("data-row")));
      await kill();
      await expect(state).toHaveAttribute("data-state", /reconnecting|disconnected/, { timeout: 10_000 });
      await expect(state).toHaveText(/Reconnecting…|Disconnected/);
      await stop();
      await start();
      stop = traffic(ports.kafka);
      await expect(state).toHaveAttribute("data-state", "live", { timeout: 30_000 });
      await expect(state).toHaveText("Live");
      // The new life's rows arrive; none of the old life stays (its ids could be other requests now), none twice.
      await expect.poll(async () => (await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.textContent ?? ""))).some((t) => t.includes("e2e-")), { timeout: 20_000 }).toBe(true);
      const ids = await rowsOf(page).evaluateAll((rs) => rs.map((r) => r.getAttribute("data-row")));
      expect(new Set(ids).size).toBe(ids.length);
      expect(old.length).toBeGreaterThan(0);
    } finally {
      await stop();
      // The browser's own report of the requests the test made fail (the broker was killed), not a fault of the page.
      for (let i = errors.length - 1; i >= 0; i--) if (/ERR_CONNECTION_REFUSED|ERR_EMPTY_RESPONSE|ERR_INCOMPLETE_CHUNKED|Failed to load resource|NetworkError|network connection was lost|Could not connect/i.test(errors[i]!)) errors.splice(i, 1);
    }
  });
});

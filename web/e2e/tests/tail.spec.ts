// The Live tail page (`/tail`, CON-1580 to CON-1585) against the real binary with real traffic written through the Kafka
// protocol: chosen from a topic or a group or by a pattern, several topics merged, filters applied by the server, held while
// the person reads, paused and resumed from its cursor, a record opened in the Sheet, copied as a command. The numbers the
// live design asks of a tail (CON-809: nothing before a person asks, at most a batch a pace, no row rewritten, nothing on
// screen moving, a reconnect with no row twice and none missed) are measured here, in all three browsers.
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { Kafka, logLevel } from "kafkajs";
import type { Page } from "@playwright/test";
import { SECOND, SEEDED } from "../playwright.config";
import { boxes, choose, expect, overflowX, test } from "./fixtures";

const KAFKA = process.env.E2E_KAFKA ?? SEEDED.kafka;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Writes records to `topic`: keys `<tag>-<i>`, JSON values, a trace header on each. */
async function write(topic: string, n: number, tag: string, broker = KAFKA, extra: (i: number) => Record<string, unknown> = () => ({})) {
  const k = new Kafka({ clientId: "orders-svc", brokers: [broker], logLevel: logLevel.NOTHING, retry: { retries: 3 } });
  const p = k.producer({ allowAutoTopicCreation: false });
  await p.connect();
  await p.send({ topic, messages: Array.from({ length: n }, (_, i) => ({ key: `${tag}-${i}`, value: JSON.stringify({ tag, i, state: i % 2 ? "paid" : "open", ...extra(i) }), headers: { "trace-id": `${tag}-t${i}` } })) });
  await p.disconnect();
}

async function create(topic: string, partitions = 1, broker = KAFKA) {
  const a = new Kafka({ clientId: "e2e-admin", brokers: [broker], logLevel: logLevel.NOTHING }).admin();
  await a.connect();
  await a.createTopics({ topics: [{ topic, numPartitions: partitions }] }).catch(() => {});
  await a.disconnect();
}

const rows = (page: Page) => page.getByTestId("tail-wrap").locator("tbody tr[data-row]");
const keys = (page: Page) => rows(page).evaluateAll((rs) => rs.map((r) => r.querySelectorAll("td")[3]?.textContent ?? ""));

async function realtime(page: Page) {
  await page.goto("/tail");
  await page.evaluate(() => localStorage.setItem("doze:tail-speed", "realtime"));
}

test("nothing is opened before a person chooses: the page is a calm chooser that sends nothing", async ({ page }) => {
  const asked: string[] = [];
  page.on("request", (r) => asked.push(r.url()));
  await page.goto("/tail");
  await expect(page.getByRole("heading", { level: 1, name: "Live tail" })).toBeVisible();
  await expect(page.getByTestId("tail-choose")).toBeVisible();
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "off");
  await expect(page.getByTestId("tail-pause")).toBeDisabled();
  await page.waitForLoadState("networkidle");
  const settled = asked.length;
  const mutations = await page.evaluate(async () => {
    let n = 0;
    new MutationObserver((r) => (n += r.length)).observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    await new Promise((r) => setTimeout(r, 4000));
    return n;
  });
  expect(asked.slice(settled), "requests after load").toEqual([]);
  expect(mutations, "DOM mutations after load").toBe(0);
  expect(asked.some((u) => u.includes("/api/v1/tail/")), "no stream").toBe(false);
  // A recent topic is one click away.
  await page.getByTestId("tail-recent").getByRole("button", { name: "orders", exact: true }).click();
  await expect(page).toHaveURL(/\/tail\?topic=orders$/);
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
});

test("from a topic and from a group: Live tail follows it from now, and a record written then is on the page", async ({ page }) => {
  await realtime(page);
  await page.goto("/topics/orders");
  await page.getByTestId("act-tail").click();
  await expect(page).toHaveURL(/\/tail\?topic=orders$/);
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  await expect(page.getByTestId("topic-chip").filter({ hasText: "orders" })).toBeVisible();
  await expect(page.getByTestId("tail-waiting")).toBeVisible();
  // The seeded records are older than the tail: none is shown.
  expect(await rows(page).count()).toBe(0);
  await page.mouse.move(2, 2);
  await write("orders", 3, "from-topic");
  await expect(rows(page)).toHaveCount(3, { timeout: 15_000 });
  expect((await keys(page)).sort()).toEqual(["from-topic-0", "from-topic-1", "from-topic-2"]);
  // Open a record: the Sheet of the Messages page, with its headers; Escape closes it and gives the address back.
  await rows(page).first().getByTestId("open-record").click();
  const sheet = page.getByTestId("record-sheet");
  await expect(sheet).toBeVisible();
  await expect(page).toHaveURL(/record=orders%3A\d+%3A\d+|record=orders:\d+:\d+/);
  await expect(sheet).toContainText("trace-id");
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
  await expect(page).not.toHaveURL(/record=/);
  // The group that reads orders: "Tail this group's input".
  await page.goto("/groups/fulfilment");
  await page.getByTestId("tail-input").click();
  await expect(page).toHaveURL(/\/tail\?topic=orders/);
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
});

test("several topics and a pattern: one stream newest first, each topic its colour and rate, a topic created later joins", async ({ page }) => {
  test.setTimeout(90_000);
  const suffix = Date.now().toString(36);
  await create(`e2e-${suffix}.eu`, 2);
  await realtime(page);
  await page.goto(`/tail?topic=payments.settled&pattern=e2e-${suffix}.*`);
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  await expect(page.getByTestId("pattern-chip")).toContainText(`e2e-${suffix}.*`);
  await expect(page.getByTestId("topic-chip").filter({ hasText: `e2e-${suffix}.eu` })).toBeVisible();
  await page.mouse.move(2, 2);
  await create(`e2e-${suffix}.us`, 1);
  await write(`e2e-${suffix}.us`, 2, "joined");
  await write(`e2e-${suffix}.eu`, 2, "eu");
  await write("payments.settled", 2, "pay");
  await expect(rows(page)).toHaveCount(6, { timeout: 20_000 });
  await expect(page.getByTestId("topic-chip").filter({ hasText: `e2e-${suffix}.us` })).toBeVisible();
  const topics = await rows(page).evaluateAll((rs) => rs.map((r) => r.querySelector("td")?.textContent ?? ""));
  expect(new Set(topics)).toEqual(new Set([`e2e-${suffix}.us`, `e2e-${suffix}.eu`, "payments.settled"]));
  // Each row carries its topic's hue; two topics of the tail never share one.
  const hues = await rows(page).evaluateAll((rs) => rs.map((r) => [r.querySelector("td")?.textContent ?? "", r.getAttribute("data-hue")] as const));
  const byTopic = new Map(hues);
  expect(new Set(byTopic.values()).size).toBe(byTopic.size);
  // The newest first by time.
  const times = await rows(page).evaluateAll((rs) => rs.map((r) => r.querySelectorAll("td")[2]?.textContent ?? ""));
  expect([...times].sort().reverse()).toEqual(times);
  await expect(page.getByTestId("topic-rate").first()).toHaveText(/\/s$/);
  await expect(page.locator("[data-slot=stat]", { hasText: /^Read/ }).locator("[data-slot=stat-value]")).toHaveText("6");
});

test("filters run on the server: only the matching records, matched of read counted, each filter a chip that turns it off", async ({ page }) => {
  test.setTimeout(90_000);
  await realtime(page);
  await page.goto("/tail?topic=orders");
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  // "/" opens the filters.
  await page.locator("body").click({ position: { x: 2, y: 300 } });
  await page.keyboard.press("/");
  await page.getByLabel("Key contains").fill("FLT-B");
  await page.getByLabel("JSON field").fill("state");
  await page.getByLabel("Equals").fill("paid");
  await page.getByTestId("filters-apply").click();
  await expect(page).toHaveURL(/key=FLT-B/);
  await expect(page).toHaveURL(/field=state/);
  await expect(page.getByTestId("filter-chip-key")).toContainText("key contains FLT-B");
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  await page.mouse.move(2, 2);
  await write("orders", 4, "flt-a");
  await write("orders", 4, "flt-b");
  // flt-b-1 and flt-b-3 are "paid": the two of eight.
  await expect(rows(page)).toHaveCount(2, { timeout: 15_000 });
  expect((await keys(page)).sort()).toEqual(["flt-b-1", "flt-b-3"]);
  const fig = (label: string) => page.locator("[data-slot=stat]", { hasText: new RegExp(`^${label}`) }).locator("[data-slot=stat-value]");
  await expect(fig("Matched")).toHaveText("2");
  await expect(fig("Read")).toHaveText("8");
  // The chip of the field turns that filter off (a new tail, from now).
  await page.getByTestId("filter-chip-field").getByRole("button").click();
  await expect(page).not.toHaveURL(/field=/);
  await expect(page.getByTestId("filter-chip-key")).toBeVisible();
});

test("held while hovered and while scrolled: a pill counts what came, the rows on screen and the bar do not move", async ({ page, browserName }) => {
  test.setTimeout(90_000);
  await realtime(page);
  await page.goto("/tail?topic=orders");
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  await page.mouse.move(2, 2);
  await write("orders", 30, "fill");
  await expect(rows(page)).toHaveCount(30, { timeout: 15_000 });
  const bar = ["h1", "[data-slot=stat-strip]", "[data-testid=tail-bar]", "[data-testid=tail-chips]", "thead"];
  const before = await boxes(page, bar);
  // Hovered: held.
  await rows(page).nth(3).hover();
  const top = await rows(page).first().getAttribute("data-row");
  await write("orders", 2, "held");
  await expect(page.getByTestId("held-pill")).toContainText("2 new · Show", { timeout: 15_000 });
  expect(await rows(page).first().getAttribute("data-row")).toBe(top);
  expect(await boxes(page, bar)).toEqual(before);
  // `n` shows them.
  await page.mouse.move(2, 2);
  await page.keyboard.press("n");
  await expect(page.getByTestId("held-pill")).toHaveCount(0);
  await expect(rows(page)).toHaveCount(32);
  // Scrolled in the table: held, and the scroll does not change.
  const sc = page.getByTestId("tail-wrap").locator("[data-slot=data-table]");
  await sc.evaluate((el) => el.scrollTo({ top: 400, behavior: "instant" }));
  await page.mouse.move(2, 2);
  const y = await sc.evaluate((el) => el.scrollTop);
  const yPage = await page.evaluate(() => window.scrollY);
  await write("orders", 3, "scrolled");
  await expect(page.getByTestId("held-pill")).toContainText("3 new · Show", { timeout: 15_000 });
  expect(await sc.evaluate((el) => el.scrollTop)).toBe(y);
  expect(await page.evaluate(() => window.scrollY)).toBe(yPage);
  await page.getByTestId("held-pill").click();
  await expect(rows(page)).toHaveCount(35);
  expect(await sc.evaluate((el) => el.scrollTop)).toBeLessThanOrEqual(2);
  if (browserName === "chromium") expect(await overflowX(page)).toBeLessThanOrEqual(0);
});

test("at the top: a batch a pace at most, no row rewritten or replaced, nothing above the table moved", async ({ page, browserName }) => {
  test.setTimeout(90_000);
  await page.goto("/tail");
  await page.evaluate(() => localStorage.setItem("doze:tail-speed", "relaxed"));
  await page.goto("/tail?topic=orders");
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  await page.mouse.move(2, 2);
  await write("orders", 3, "base");
  await expect(rows(page)).toHaveCount(3, { timeout: 15_000 });
  const bar = ["h1", "[data-slot=stat-strip]", "[data-testid=tail-bar]", "[data-testid=tail-chips]", "thead"];
  const before = await boxes(page, bar);
  await page.evaluate(() => {
    const w = window as unknown as { __b: number[]; __old: Map<string, [Element, string]>; __ls: number };
    w.__b = [];
    w.__ls = 0;
    try {
      new PerformanceObserver((l) => {
        for (const e of l.getEntries() as unknown as { value: number; hadRecentInput: boolean }[]) if (!e.hadRecentInput) w.__ls += e.value;
      }).observe({ type: "layout-shift", buffered: false });
    } catch {
      w.__ls = -1;
    }
    w.__old = new Map();
    for (const tr of document.querySelectorAll("[data-testid=tail-wrap] tbody tr[data-row]")) w.__old.set(tr.getAttribute("data-row")!, [tr, tr.textContent ?? ""]);
    new MutationObserver((records) => {
      if (records.some((r) => [...r.addedNodes].some((n) => n instanceof HTMLElement && n.matches("tr[data-row]")))) w.__b.push(performance.now());
    }).observe(document.querySelector("[data-testid=tail-wrap] tbody")!, { childList: true });
  });
  // Traffic faster than the pace: a record every 400 ms for 10 s.
  for (let i = 0; i < 25; i++) {
    await write("orders", 1, `fast-${i}`);
    await sleep(400);
  }
  await sleep(3500);
  const r = await page.evaluate(() => {
    const w = window as unknown as { __b: number[]; __old: Map<string, [Element, string]>; __ls: number };
    let replaced = 0;
    let rewritten = 0;
    for (const [id, [node, text]] of w.__old) {
      const now = document.querySelector(`[data-testid=tail-wrap] tbody tr[data-row="${CSS.escape(id)}"]`);
      if (now !== node) replaced++;
      else if (now.textContent !== text) rewritten++;
    }
    return { b: w.__b, replaced, rewritten, ls: w.__ls };
  });
  const gaps = r.b.slice(1).map((t, i) => t - r.b[i]!);
  expect(r.b.length, "batches").toBeGreaterThanOrEqual(2);
  for (const g of gaps) expect(g, `gaps ${gaps.map(Math.round).join(", ")} ms`).toBeGreaterThanOrEqual(2900);
  expect(r.replaced).toBe(0);
  expect(r.rewritten).toBe(0);
  if (browserName === "chromium") expect(r.ls, "layout shift").toBeLessThan(0.01);
  expect(await boxes(page, bar)).toEqual(before);
  await expect(rows(page)).toHaveCount(28);
  const ids = await rows(page).evaluateAll((rs) => rs.map((x) => x.getAttribute("data-row")));
  expect(new Set(ids).size).toBe(ids.length);
});

test("Pause reads nothing; Resume goes on from where it paused, with what was written meanwhile and nothing twice", async ({ page }) => {
  test.setTimeout(90_000);
  await realtime(page);
  await page.goto("/tail?topic=orders");
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  await page.mouse.move(2, 2);
  await page.keyboard.press("p");
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "paused");
  await expect(page.getByTestId("tail-pause")).toHaveText(/Resume/);
  const asked: string[] = [];
  page.on("request", (r) => asked.push(r.url()));
  await write("orders", 4, "while-paused");
  await sleep(2500);
  expect(await rows(page).count()).toBe(0);
  expect(asked.filter((u) => u.includes("/api/v1/tail/")), "nothing fetched while paused").toEqual([]);
  await page.getByTestId("tail-pause").click();
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  await expect(rows(page)).toHaveCount(4, { timeout: 15_000 });
  expect((await keys(page)).sort()).toEqual(["while-paused-0", "while-paused-1", "while-paused-2", "while-paused-3"]);
  // The speed changes the pace and keeps the rows and the cursor.
  await choose(page, page.getByTestId("tail-speed"), "Slow · 10 s");
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  await expect(rows(page)).toHaveCount(4);
});

test("a row's menu copies it as a kcat command that writes it again", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "reading the clipboard needs Chromium's permission");
  await realtime(page);
  await page.goto("/tail?topic=orders");
  await expect(page.getByTestId("tail-conn")).toHaveAttribute("data-state", "live", { timeout: 10_000 });
  await page.mouse.move(2, 2);
  await write("orders", 1, "copy-me");
  await expect(rows(page)).toHaveCount(1, { timeout: 15_000 });
  await rows(page).first().getByTestId("row-menu").click();
  await page.getByTestId("copy-kcat").click();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toContain("kcat -P -b");
  const text = await page.evaluate(() => navigator.clipboard.readText());
  expect(text).toContain("-t orders");
  expect(text).toContain("-k copy-me-0");
  expect(text).toContain("-H trace-id=copy-me-t0");
});

test.describe("a broker that goes away and comes back", () => {
  const ports = SECOND;
  let child: ChildProcess | undefined;
  let data = "";
  const bin = process.env.DOZE_KAFKA_BIN ?? resolve(import.meta.dir, "../../../target/release/doze-kafka");
  const start = async () => {
    child = spawn(bin, ["--listen", ports.kafka, "--data-dir", data, "--console-addr", ports.console, "--schema-registry-addr", ports.registry], { stdio: "ignore" });
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

  test("the tail reconnects from its cursor: what was written while it was down comes once, nothing twice", async ({ page, errors }) => {
    test.setTimeout(120_000);
    data = mkdtempSync(join(tmpdir(), "doze-e2e-tail-"));
    await start();
    try {
      await create("events", 2, ports.kafka);
      await page.goto(`http://${ports.console}/tail`);
      await page.evaluate(() => localStorage.setItem("doze:tail-speed", "realtime"));
      await page.goto(`http://${ports.console}/tail?topic=events`);
      const state = page.getByTestId("tail-conn");
      await expect(state).toHaveAttribute("data-state", "live", { timeout: 10_000 });
      await page.mouse.move(2, 2);
      await write("events", 3, "before", ports.kafka);
      await expect(rows(page)).toHaveCount(3, { timeout: 15_000 });
      await kill();
      await expect(state).toHaveAttribute("data-state", /reconnecting|disconnected/, { timeout: 10_000 });
      await start();
      // Written before the tail is back: the cursor brings them.
      await write("events", 2, "gap", ports.kafka);
      await expect(state).toHaveAttribute("data-state", "live", { timeout: 30_000 });
      await expect(rows(page)).toHaveCount(5, { timeout: 15_000 });
      expect((await keys(page)).sort()).toEqual(["before-0", "before-1", "before-2", "gap-0", "gap-1"]);
      const ids = await rows(page).evaluateAll((rs) => rs.map((r) => r.getAttribute("data-row")));
      expect(new Set(ids).size).toBe(ids.length);
    } finally {
      await page.close();
      for (let i = errors.length - 1; i >= 0; i--) if (errors[i]!.startsWith("console: ") && /ERR_CONNECTION_REFUSED|ERR_EMPTY_RESPONSE|ERR_INCOMPLETE_CHUNKED|Failed to load resource|NetworkError|network connection was lost|Could not connect/i.test(errors[i]!)) errors.splice(i, 1);
    }
  });
});

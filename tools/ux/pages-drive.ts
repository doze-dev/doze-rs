// Page scenarios for the console, driven in real Chrome over CDP (UX agent 2, "pages"). Plain bun, small named scenarios that
// assert numbers. Agent 1's `tools/ux/drive.ts` may import or merge these (`export const scenarios`).
//
//   bun tools/ux/pages-drive.ts http://127.0.0.1:19412 [scenario ...] [--json out.json] [--shots dir]
//
// With no scenario name every scenario runs, one Chrome for all of them (the lock is held for at most three minutes).
// Exit status 1 when a scenario fails. The numbers are printed either way: they are the before/after of the report.

import { Browser, type Box, type Page, moved } from "./pages-cdp";

export interface Outcome {
  name: string;
  ok: boolean;
  notes: string[];
  numbers: Record<string, unknown>;
}
type Scenario = (p: Page, base: string, shots?: string) => Promise<Outcome>;

/** The parts above the data of a topic page, by name: what must not move when a control is used. */
const ABOVE = `(() => {
  const box = (e) => { if (!e) return null; const r = e.getBoundingClientRect(); return { x: Math.round((r.left + scrollX) * 10) / 10, y: Math.round((r.top + scrollY) * 10) / 10, w: Math.round(r.width * 10) / 10, h: Math.round(r.height * 10) / 10 }; };
  const byText = (sel, t) => [...document.querySelectorAll(sel)].find((e) => e.textContent.trim() === t);
  const reading = byText('span', 'Reading as');
  return {
    crumbs: box(document.querySelector('nav.crumbs')),
    header: box(document.querySelector('.page-h')),
    tabs: box(document.querySelector('.tabs')),
    callout: box(document.querySelector('main .callout')),
    toolbar: box(document.querySelector('form.toolbar')),
    seg: box(document.querySelector('form.toolbar .seg')),
    reading: box(reading),
    readingSeg: box(reading && reading.nextElementSibling && reading.nextElementSibling.matches('.seg') ? reading.nextElementSibling : null),
    firstTable: box(document.querySelector('main table.t')),
  };
})()`;

async function above(p: Page): Promise<Record<string, Box | null>> {
  return p.eval(ABOVE);
}

/** Click the first element of `sel` whose text is `text`, by a real mouse event. */
async function clickText(p: Page, sel: string, text: string, navigates = true, scroll = true): Promise<boolean | null> {
  const c = await p.eval<{ x: number; y: number } | null>(`(() => { const e = [...document.querySelectorAll(${JSON.stringify(sel)})].find((e) => e.textContent.trim() === ${JSON.stringify(text)}); if (!e) return null; if (${scroll}) e.scrollIntoView({ block: 'center' }); const r = e.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
  if (!c) return null;
  return p.clickAt(c.x, c.y, { navigates });
}

const outcome = (name: string, notes: string[], numbers: Record<string, unknown> = {}): Outcome => ({ name, ok: notes.length === 0, notes, numbers });

/** Findings 8 and 9: choosing a partition or an isolation level changes the data, never what is above it. */
export const messagesStable: Scenario = async (p, base) => {
  const notes: string[] = [];
  const url = `${base}/topics/orders/messages`;
  await p.goto(url);
  const start = await above(p);
  const click = (t: string) => () => clickText(p, "form.toolbar button, form.toolbar a", t);
  const steps: [string, () => Promise<boolean | null>][] = [
    ["partition 0", click("0")],
    ["read_committed", click("read_committed")],
    ["partition 1", click("1")],
    ["read_uncommitted", click("read_uncommitted")],
    ["all partitions", click("All partitions")],
  ];
  const numbers: Record<string, unknown> = { start };
  let clsMax = 0;
  for (const [name, act] of steps) {
    await p.scrollTo(0);
    const had = await act();
    if (had === null) {
      numbers[name] = "control absent on this page";
      notes.push(`${name}: the control is not on the page`);
      continue;
    }
    const now = await above(p);
    const shifted = moved(start, now).filter((s) => !s.startsWith("firstTable"));
    for (const s of shifted) notes.push(`after ${name}: ${s}`);
    clsMax = Math.max(clsMax, await p.cls());
    numbers[name] = { url: await p.url(), shifted };
  }
  numbers.cls = clsMax;
  // The reading control stays at the right edge of the toolbar, in every state.
  const rights = await p.eval<number[]>(`[...document.querySelectorAll('form.toolbar .seg')].map((e) => Math.round(e.getBoundingClientRect().right))`);
  numbers.segRights = rights;
  return outcome("messages-stable", notes, numbers);
};

/** Finding 7 and the shared header: the tabs of a topic share one header and one tab row, to the pixel. */
export const topicTabsStable: Scenario = async (p, base) => {
  const notes: string[] = [];
  const pages = ["", "/config", "/consumers", "/log", "/producers", "/transactions/orders-eos"];
  const seen: Record<string, Record<string, Box | null>> = {};
  for (const tail of pages) {
    await p.goto(`${base}/topics/orders${tail}`);
    seen[tail || "/"] = await p.boxes([".page-h", ".tabs", ".page-h h1", ".page-h .acts", ".tabs a:nth-child(1)", ".tabs a:nth-child(2)", ".tabs a:nth-child(3)", ".tabs a:nth-child(4)", ".tabs a:nth-child(5)"]);
  }
  const first = seen["/"];
  for (const [tail, boxes] of Object.entries(seen)) for (const s of moved(first, boxes)) notes.push(`${tail}: ${s}`);
  return outcome("topic-tabs-stable", notes, { seen });
};

/** Finding 7: the Consumers tab is inside the topic and lists the groups that read it. */
export const consumersTab: Scenario = async (p, base, shots) => {
  const notes: string[] = [];
  await p.goto(`${base}/topics/orders`);
  const nav = await p.click(".tabs a:nth-child(4)");
  const at = await p.url();
  if (at !== "/topics/orders/consumers") notes.push(`the tab went to ${at}`);
  const crumbs = await p.crumbs();
  const groups = await p.eval<string[]>(`[...document.querySelectorAll('table.consumers td.grp a')].map((a) => a.textContent.trim())`);
  if (groups.length < 1) notes.push("no group listed");
  const rows = await p.eval<number>(`document.querySelectorAll('table.consumers tbody tr').length`);
  const count = await p.eval<string>(`document.querySelector('.tabs a.on .n')?.textContent ?? ''`);
  if (String(groups.length) !== count) notes.push(`the tab says ${count} and the page lists ${groups.length}`);
  if (shots) await p.screenshot(`${shots}/consumers.png`);
  return outcome("consumers-tab", notes, { nav, at, crumbs, groups, rows, count });
};

/** Finding 4: the breadcrumb is the entity path in every state of the pages that used to put state in it. */
export const breadcrumbs: Scenario = async (p, base) => {
  const notes: string[] = [];
  const cases: [string, string[]][] = [
    ["/topics/orders", ["Resources", "Topics", "orders"]],
    ["/topics/orders/messages?partition=0", ["Resources", "Topics", "orders"]],
    ["/topics/orders/messages?partition=0&offset=16098", ["Resources", "Topics", "orders"]],
    ["/topics/orders/messages?isolation=read_committed", ["Resources", "Topics", "orders"]],
    ["/topics/notifications.email", ["Resources", "Topics", "notifications.email"]],
    ["/topics/orders/partitions", ["Resources", "Topics", "orders", "Partitions"]],
    ["/topics/orders/config", ["Resources", "Topics", "orders", "Config"]],
    ["/topics/orders/consumers", ["Resources", "Topics", "orders", "Consumers"]],
    ["/topics/orders/log", ["Resources", "Topics", "orders", "Log"]],
    ["/topics/orders/log?partition=1", ["Resources", "Topics", "orders", "Log"]],
    ["/topics/orders/producers", ["Resources", "Topics", "orders", "Producers"]],
    ["/topics/orders/producers?open=4023", ["Resources", "Topics", "orders", "Producers"]],
    ["/topics/orders/transactions/orders-eos", ["Resources", "Topics", "orders", "Producers", "orders-eos"]],
    ["/topics/orders/transactions/orders-eos/held", ["Resources", "Topics", "orders", "Producers", "orders-eos", "What it holds back"]],
    ["/settings", ["Settings"]],
    ["/access", ["Access"]],
    ["/access?state=secured", ["Access"]],
    ["/groups/fulfilment", ["Resources", "Groups", "fulfilment"]],
    ["/groups/fulfilment/lag", ["Resources", "Groups", "fulfilment", "Lag"]],
    ["/live", ["Live"]],
    ["/live?state=paused", ["Live"]],
    ["/lab", ["Lab"]],
  ];
  const got: Record<string, string[]> = {};
  for (const [path, want] of cases) {
    await p.goto(base + path);
    const c = await p.crumbs();
    got[path] = c;
    if (JSON.stringify(c) !== JSON.stringify(want)) notes.push(`${path}: ${c.join(" > ")} (wanted ${want.join(" > ")})`);
    const title = await p.eval<string>("document.title");
    if (!title.startsWith(want[want.length - 1])) notes.push(`${path}: title ${JSON.stringify(title)}`);
  }
  return outcome("breadcrumbs", notes, got);
};

/** Finding 6: an unbuilt screen is one designed page, 200, its own crumb, a way back, never Home's content. */
export const unbuilt: Scenario = async (p, base, shots) => {
  const notes: string[] = [];
  await p.goto(`${base}/topics/orders/messages?partition=1`);
  await p.eval(`document.body.insertAdjacentHTML('beforeend', '<a id="__lab" href="/lab">lab</a>')`);
  await p.click("#__lab");
  const at = await p.url();
  const crumbs = await p.crumbs();
  const h2 = await p.eval<string>("document.querySelector('main h2')?.textContent ?? ''");
  const back = await p.eval<string>("[...document.querySelectorAll('main a.btn')].map((a) => a.getAttribute('href')).join(',')");
  const robots = await p.eval<string>("document.querySelector('meta[name=robots]')?.content ?? ''");
  const homeLandmarks = await p.eval<number>("document.querySelectorAll('[data-extra=live-sentence], .hero').length");
  if (at !== "/lab") notes.push("went to " + at);
  if (back !== "/topics/orders/messages?partition=1") notes.push("the way back is " + back);
  const rail = await p.eval<string>("[...document.querySelectorAll('.rail .ni .soon')].map((e) => e.parentElement.textContent.trim()).join(',')");
  if (!rail.includes("Lab")) notes.push("the rail does not mark Lab as Soon: " + rail);
  if (crumbs.join(">") !== "Lab") notes.push("crumbs " + crumbs.join(">"));
  if (h2 !== "Not available in this edition") notes.push("heading " + h2);
  if (robots !== "noindex") notes.push("no noindex");
  if (homeLandmarks !== 0) notes.push("Home landmarks on the page");
  if (shots) await p.screenshot(`${shots}/unbuilt.png`);
  return outcome("unbuilt", notes, { at, crumbs, h2, back, robots });
};

/** Finding 10: an expander opens in place: same address, same scroll, nothing above it moves. */
export const expanders: Scenario = async (p, base, shots) => {
  const notes: string[] = [];
  const numbers: Record<string, unknown> = {};
  await p.goto(`${base}/topics/inventory.levels/config`);
  for (const [label, scroll] of [["Where from", 700], ["Kafka's text", 400]] as const) {
    // Put the control in the middle of the window first, then measure: the click itself must not scroll.
    const top = await p.eval<number>(`(() => { const e = [...document.querySelectorAll('a.btn, summary')].find((e) => e.textContent.trim() === ${JSON.stringify(label)}); return e ? Math.round(e.getBoundingClientRect().top + scrollY) : -1; })()`);
    await p.scrollTo(Math.max(0, top - 300) + (scroll > 0 ? 0 : 0));
    const y0 = await p.scrollY;
    const at0 = await p.url();
    const above0 = await p.eval<Record<string, number>>(`(() => { const r = (s) => Math.round(document.querySelector(s).getBoundingClientRect().top + scrollY); return { header: r('.page-h'), tabs: r('.tabs'), toolbar: r('form.toolbar') }; })()`);
    const navigated = await clickText(p, "a.btn, summary", label, true, false);
    const y1 = await p.scrollY;
    const at1 = await p.url();
    const above1 = await p.eval<Record<string, number>>(`(() => { const r = (s) => Math.round(document.querySelector(s).getBoundingClientRect().top + scrollY); return { header: r('.page-h'), tabs: r('.tabs'), toolbar: r('form.toolbar') }; })()`);
    if (navigated === null) notes.push(`${label}: no such control`);
    if (navigated) notes.push(`${label}: the click navigated (${at0} -> ${at1})`);
    if (y1 !== y0) notes.push(`${label}: scrollY ${y0} -> ${y1}`);
    for (const k of Object.keys(above0)) if (above0[k] !== above1[k]) notes.push(`${label}: ${k} moved ${above0[k]} -> ${above1[k]}`);
    numbers[label] = { navigated, scrollBefore: y0, scrollAfter: y1, url: at1, opened: await p.eval("document.querySelectorAll('details[open]').length") };
  }
  if (shots) await p.screenshot(`${shots}/expanders.png`);
  return outcome("expanders", notes, numbers);
};

export const scenarios: Record<string, Scenario> = {
  expanders,
  "messages-stable": messagesStable,
  "topic-tabs-stable": topicTabsStable,
  "consumers-tab": consumersTab,
  breadcrumbs,
  unbuilt,
};

if (import.meta.main) {
  const args = process.argv.slice(2);
  const base = args.find((a) => a.startsWith("http"))!.replace(/\/$/, "");
  const json = args.indexOf("--json");
  const shotsAt = args.indexOf("--shots");
  const shots = shotsAt >= 0 ? args[shotsAt + 1] : undefined;
  if (shots) await Bun.$`mkdir -p ${shots}`;
  const skip = new Set([json >= 0 ? args[json + 1] : "", shots ?? ""]);
  const wanted = args.filter((a) => !a.startsWith("http") && !a.startsWith("--") && !skip.has(a));
  const names = wanted.length ? wanted : Object.keys(scenarios);
  const browser = await Browser.launch();
  const results: Outcome[] = [];
  try {
    for (const name of names) {
      const page = await browser.page();
      try {
        const r = await Promise.race([
          scenarios[name](page, base, shots),
          new Promise<Outcome>((_, no) => setTimeout(() => no(new Error("scenario took more than 40 s")), 40_000)),
        ]);
        const cls = await page.cls();
        if (cls > 0.02) r.notes.push(`layout shift ${cls} > 0.02`);
        for (const pr of page.problems) r.notes.push("console: " + pr);
        r.ok = r.notes.length === 0;
        results.push(r);
      } catch (e) {
        results.push({ name, ok: false, notes: [String(e)], numbers: {} });
      }
    }
  } finally {
    await browser.close();
  }
  for (const r of results) {
    console.log(`${r.ok ? "PASS" : "FAIL"} ${r.name}`);
    for (const n of r.notes) console.log("   - " + n);
  }
  if (json >= 0) await Bun.write(args[json + 1], JSON.stringify(results, null, 1));
  process.exit(results.every((r) => r.ok) ? 0 : 1);
}

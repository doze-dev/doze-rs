// The live design in real Chrome (docs/port/console/live-design.md, CON-800 to CON-812): run against a console on a REAL broker with
// traffic (`--url http://127.0.0.1:<console port>`); against the fixtures they pass what they can and skip what needs a moving tape.
//
//   stream-rate          every page, default settings: requests after load = 0, DOM mutations after load = 0, no EventSource
//   refresh-interval     5 s on Topics: conditional requests, nothing mutates that did not change, no layout shift; Off stops; it persists
//   text-stable          the strips, toolbars and table headers of the data pages keep their boxes while the broker's numbers move
//   tail-live            the Live tail: batches, rows added at the top only, the header and toolbar pixel-identical, scrollY unchanged,
//                        the pill when scrolled, the speed setting
//   tail-reconnect       (needs BROKER_CTL) the broker is stopped and started: amber, then green, no row twice
//   table-stable         the columns of the tape are the same for 20 s of new rows; header and body cells line up; no sideways scroll
//   live-above-the-fold  at 1440x900 and 1024x768 the filters and the table's first row are on screen without scrolling
import { join } from "node:path";
import type { T } from "../drive.ts";

const sleep = (ms: number) => Bun.sleep(ms);
const PAGES = [
  "/", "/topics", "/topics/orders", "/topics/orders/messages", "/topics/orders/log", "/topics/orders/producers", "/topics/orders/partitions",
  "/groups", "/groups/fulfilment", "/groups/fulfilment/lag", "/groups/fulfilment/story", "/live", "/access", "/settings", "/connect",
];

const WATCH = `(() => {
  window.__muts = 0; window.__es = 0;
  new MutationObserver((l) => { window.__muts += l.length; }).observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
  const E = window.EventSource; if (E) window.EventSource = function (...a) { window.__es++; return new E(...a); };
  return 1;
})()`;

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  async "stream-rate"(t) {
    const p = t.page;
    await p.goto("/");
    await p.eval("localStorage.removeItem('doze:refresh'); localStorage.removeItem('doze:tail-speed')");
    const per = Number(process.env.RATE_SECONDS ?? 6);
    let total = 0, mutations = 0;
    for (const path of PAGES) {
      await p.goto(path);
      await sleep(900);
      await p.eval(WATCH);
      const before = p.requests.length;
      await sleep(per * 1000);
      const made = p.requests.slice(before).filter((r) => !r.url.includes("/static/"));
      const muts = await p.eval<number>("window.__muts");
      const es = await p.eval<number>("window.__es");
      total += made.length;
      mutations += muts;
      t.ok(made.length === 0, `${path}: ${made.length} requests after load (${made.map((r) => r.url).join(", ")})`);
      t.ok(muts === 0, `${path}: ${muts} DOM mutations after load`);
      t.ok(es === 0, `${path}: ${es} EventSource opened by itself`);
    }
    t.num("seconds-per-page", per);
    t.num("pages", PAGES.length);
    t.num("requests-after-load", total);
    t.num("mutations-after-load", mutations);
  },

  async "refresh-interval"(t) {
    const p = t.page;
    await p.goto("/topics");
    await p.eval("localStorage.removeItem('doze:refresh')");
    await p.goto("/topics");
    await p.waitFor("!!document.querySelector('.refresh .dz-trigger')");
    t.ok(await p.eval("!!document.querySelector('.refresh time[data-stamp]') && /^\\d\\d:\\d\\d:\\d\\d$/.test(document.querySelector('.refresh time[data-stamp]').textContent)"), "the stamp is an absolute time");
    await p.shot(join(t.shots, "refresh-bar-off.png"));
    // Choose 5 s through the control, as a person does.
    await p.click(".refresh .dz-trigger");
    await sleep(150);
    await p.shot(join(t.shots, "refresh-bar-open.png"));
    await p.click(".refresh .dz-opt[data-value='5']");
    await sleep(300);
    t.ok((await p.eval<string>("localStorage.getItem('doze:refresh')")) === "5", "5 s is remembered");
    await p.eval(`(() => { window.__muts = 0; window.__bad = 0; window.__cells = new Set();
      new MutationObserver((l) => { for (const x of l) { window.__muts++; const el = x.target.nodeType === 1 ? x.target : x.target.parentElement; if (!el) continue; if (el.closest('thead, .toolbar, .page-h, .tabs, .crumbs, .refresh, nav')) { if (!el.closest('.refresh')) window.__bad++; continue; } const c = el.closest('td, .stat'); if (c) window.__cells.add(c); } }).observe(document.getElementById('main'), { subtree: true, childList: true, attributes: true, characterData: true });
      return 1; })()`);
    await p.resetShift();
    const mark = p.requests.length;
    await sleep(16000);
    const polls = p.requests.slice(mark).filter((r) => r.url.includes("/fragments/"));
    const conditional = polls.filter((r) => Object.keys(r.headers).some((h) => h.toLowerCase() === "if-none-match"));
    t.num("polls", polls.length);
    t.num("conditional", conditional.length);
    t.ok(polls.length >= 4, `polls happen on the interval (${polls.length})`);
    t.ok(conditional.length >= polls.length - 2, "after the first, every poll is conditional (If-None-Match)");
    t.num("mutations", await p.eval<number>("window.__muts"));
    t.num("cells-touched", await p.eval<number>("window.__cells.size"));
    t.num("cells-in-page", await p.eval<number>("document.querySelectorAll('main td, main .stat').length"));
    t.ok((await p.eval<number>("window.__bad")) === 0, "no header, toolbar or title was touched by a poll");
    t.num("shift", await p.shift());
    t.ok((await p.shift()) < 0.01, "no layout shift");
    // Off stops them.
    await p.click(".refresh .dz-trigger");
    await sleep(150);
    await p.click(".refresh .dz-opt[data-value='0']");
    await sleep(500);
    const off = p.requests.length;
    await sleep(7000);
    t.ok(p.requests.length === off, "Off stops the requests");
    // 5 s again, then a reload keeps it.
    await p.click(".refresh .dz-trigger");
    await sleep(150);
    await p.click(".refresh .dz-opt[data-value='5']");
    await sleep(300);
    await p.goto("/topics");
    await p.waitFor("!!document.querySelector('.refresh .dz-value')");
    t.ok((await p.text(".refresh .dz-value")).trim() === "5 s", "a reload keeps the choice");
    await p.eval("localStorage.removeItem('doze:refresh')");
  },

  async "text-stable"(t) {
    const p = t.page;
    await p.goto("/");
    await p.eval("localStorage.setItem('doze:refresh', '5')");
    const pages = ["/", "/topics/orders", "/live", "/groups/fulfilment", "/topics/orders/log", "/topics/orders/producers"];
    const SEL = ".stats, .stat, .toolbar, .tailbar, .tabs, .txn-region, .attention-region, .home-callout, details.refusals, thead th, .dt-empty";
    const boxes = `(() => [...document.querySelectorAll(${JSON.stringify(SEL)})].map((e, i) => { const r = e.getBoundingClientRect(); return [i, e.className || e.tagName, Math.round(r.x * 10) / 10, Math.round(r.y * 10) / 10, Math.round(r.width * 10) / 10, Math.round(r.height * 10) / 10]; }))()`;
    for (const path of pages) {
      await p.goto(path);
      await sleep(1200);
      const a = await p.eval<Array<[number, string, number, number, number, number]>>(boxes);
      await sleep(Number(process.env.STABLE_SECONDS ?? 14) * 1000);
      const b = await p.eval<Array<[number, string, number, number, number, number]>>(boxes);
      t.ok(a.length > 0, `${path}: the page has strips or headers to compare`);
      const moved = a.filter((x, i) => b[i] && (x[2] !== b[i][2] || x[3] !== b[i][3] || x[4] !== b[i][4] || x[5] !== b[i][5]));
      t.ok(a.length === b.length && moved.length === 0, `${path}: ${moved.length} of ${a.length} boxes changed (${moved.slice(0, 3).map((m) => m[1]).join(", ")})`);
      t.num(path + " boxes", a.length);
    }
    await p.eval("localStorage.removeItem('doze:refresh')");
  },

  async "tail-live"(t) {
    const p = t.page;
    await p.goto("/live");
    await p.eval("localStorage.removeItem('doze:tail-speed')");
    await p.goto("/live");
    await p.waitFor("!!document.querySelector('[data-tail-mode=live]') && !document.querySelector('[data-tail-mode=live]').disabled");
    t.ok((await p.eval<string>("document.querySelector('[data-conn]').getAttribute('data-conn')")) === "snapshot", "it opens as a snapshot");
    await p.shot(join(t.shots, "live-snapshot.png"));
    // Realtime, so that a batch comes each second.
    await p.click(".tailbar .speed .dz-trigger");
    await sleep(150);
    await p.click(".tailbar .speed .dz-opt[data-value='1']");
    await sleep(200);
    t.ok((await p.eval<string>("document.querySelector('[data-tail-speed]').title")).startsWith("Realtime"), "the tooltip states the rate");
    await p.click("[data-tail-mode=live]");
    await p.waitFor("document.querySelector('[data-conn]').getAttribute('data-conn') === 'live'", 5000);
    await p.eval(`(() => {
      const body = document.querySelector('tbody[data-tail]');
      window.__first = [...body.children].slice(0, 3).map((r) => r);
      window.__batches = []; window.__removedOld = 0; window.__thead = document.querySelector('thead').outerHTML;
      new MutationObserver((l) => { let adds = 0; for (const x of l) { adds += [...x.addedNodes].filter((n) => n.nodeType === 1 && n.tagName === 'TR').length; for (const n of x.removedNodes) if (window.__first.includes(n)) window.__removedOld++; } if (adds) window.__batches.push([Math.round(performance.now()), adds]); }).observe(body, { childList: true });
      new MutationObserver(() => { window.__headerTouched = (window.__headerTouched || 0) + 1; }).observe(document.querySelector('thead'), { subtree: true, childList: true, attributes: true, characterData: true });
      new MutationObserver(() => { window.__toolbarTouched = (window.__toolbarTouched || 0) + 1; }).observe(document.querySelector('.toolbar'), { subtree: true, childList: true, attributes: true, characterData: true });
      return 1; })()`);
    await p.resetShift();
    const clipOf = async (sel: string) => { const r = await p.rect(sel); return { x: Math.floor(r.x), y: Math.floor(r.y), width: Math.ceil(r.width), height: Math.ceil(r.height) }; };
    const shotA = await p.shot(join(t.shots, "tail-header-a.png"), await clipOf(".page-h"));
    const barA = await p.shot(join(t.shots, "tail-bar-a.png"), await clipOf(".toolbar"));
    const y0 = await p.scrollY();
    await sleep(3200);
    const shotB = await p.shot(join(t.shots, "tail-header-b.png"), await clipOf(".page-h"));
    const barB = await p.shot(join(t.shots, "tail-bar-b.png"), await clipOf(".toolbar"));
    t.ok(Bun.file(shotA).size === Bun.file(shotB).size && (await Bun.file(shotA).arrayBuffer()).byteLength === (await Bun.file(shotB).arrayBuffer()).byteLength, "the page header is pixel-identical 3 s apart");
    t.ok(Buffer.compare(Buffer.from(await Bun.file(shotA).arrayBuffer()), Buffer.from(await Bun.file(shotB).arrayBuffer())) === 0, "the page header bytes are identical");
    t.ok(Buffer.compare(Buffer.from(await Bun.file(barA).arrayBuffer()), Buffer.from(await Bun.file(barB).arrayBuffer())) === 0, "the toolbar bytes are identical");
    await sleep(8000);
    const batches = await p.eval<Array<[number, number]>>("window.__batches");
    t.num("batches-in-11s", batches.length);
    const gaps = batches.slice(1).map((b, i) => b[0] - batches[i][0]);
    t.num("least-gap-ms", gaps.length ? Math.min(...gaps) : -1);
    t.ok(batches.length > 0, "rows arrived");
    t.ok(gaps.every((g) => g >= 900), `at most one batch a second (gaps ${gaps.join(",")})`);
    t.ok((await p.eval<number>("window.__removedOld")) === 0, "no row that was there was replaced or removed");
    t.ok(!(await p.eval<number>("window.__headerTouched || 0")) && !(await p.eval<number>("window.__toolbarTouched || 0")), "the table header and the toolbar are never part of an update");
    t.ok((await p.scrollY()) === y0, "scrollY did not change");
    const rows = await p.eval<number>("document.querySelector('tbody[data-tail]').children.length");
    t.ok(rows <= 200, `the DOM keeps at most 200 rows (${rows})`);
    await p.shot(join(t.shots, "live-tail.png"));
    // Scrolled: rows are held behind the pill, and the page does not move.
    await p.scrollTo(300);
    const held0 = await p.eval<number>("document.querySelectorAll('tbody[data-tail] tr').length");
    await sleep(4500);
    const held1 = await p.eval<number>("document.querySelectorAll('tbody[data-tail] tr').length");
    t.ok(held1 === held0, `scrolled down, no row is put in (${held0} to ${held1})`);
    t.ok(await p.eval("!document.querySelector('[data-held-pill]').hidden"), "the pill appears");
    t.num("pill", await p.text("[data-held-n]"));
    t.ok((await p.scrollY()) === 300, "the page did not move while rows were held");
    await p.shot(join(t.shots, "live-held.png"));
    await p.click("[data-held-pill]");
    await sleep(400);
    t.ok(await p.eval("document.querySelector('[data-held-pill]').hidden"), "Show puts them in and the pill goes");
    // Paused through the speed control.
    await p.scrollTo(0);
    await p.click(".tailbar .speed .dz-trigger");
    await sleep(150);
    await p.click(".tailbar .speed .dz-opt[data-value='0']");
    await sleep(300);
    t.ok((await p.text("[data-conn-text]")) === "Paused", "Paused says so");
    const n0 = await p.eval<number>("document.querySelectorAll('tbody[data-tail] tr').length");
    await sleep(3500);
    t.ok((await p.eval<number>("document.querySelectorAll('tbody[data-tail] tr').length")) === n0, "Paused takes no rows");
    await p.click("[data-tail-mode=snapshot]");
    t.ok((await p.eval<string>("document.querySelector('[data-conn]').getAttribute('data-conn')")) === "snapshot", "Snapshot closes it");
    await p.eval("localStorage.removeItem('doze:tail-speed')");
  },

  async "tail-reconnect"(t) {
    const ctl = process.env.BROKER_CTL ?? "";
    if (!ctl) return;
    const p = t.page;
    await p.goto("/live");
    await p.eval("localStorage.setItem('doze:tail-speed', '1')");
    await p.goto("/live");
    await p.waitFor("!document.querySelector('[data-tail-mode=live]').disabled");
    await p.click("[data-tail-mode=live]");
    await p.waitFor("document.querySelector('[data-conn]').getAttribute('data-conn') === 'live'", 5000);
    await sleep(2500);
    // The console is the broker's, so stopping the broker ends the stream; the page must say so and come back.
    Bun.spawn([ctl, "stop"]);
    await p.waitFor("document.querySelector('[data-conn]').getAttribute('data-conn') !== 'live'", 15000);
    const first = await p.eval<string>("document.querySelector('[data-conn]').getAttribute('data-conn')");
    t.ok(first === "reconnecting" || first === "disconnected", `after the loss: ${first}`);
    await p.shot(join(t.shots, "live-reconnecting.png"));
    await sleep(3000);
    await p.shot(join(t.shots, "live-disconnected.png"));
    Bun.spawn([ctl, "start"]);
    await p.waitFor("document.querySelector('[data-conn]').getAttribute('data-conn') === 'live'", 60000);
    t.ok(true, "green again");
    await sleep(3000);
    const dup = await p.eval<number>("(() => { const ids = [...document.querySelectorAll('tbody[data-tail] tr')].map((r) => r.id); return ids.length - new Set(ids).size; })()");
    t.ok(dup === 0, `no row twice (${dup})`);
    // The browser logs each refused connection: that is the loss this scenario causes, not a defect of the page.
    p.clearLogs();
    await p.eval("localStorage.removeItem('doze:tail-speed')");
  },

  async "table-stable"(t) {
    const p = t.page;
    await p.goto("/live");
    await p.eval("localStorage.setItem('doze:tail-speed', '1')");
    for (const width of [1024, 1280, 1440, 1920]) {
      await p.viewport(width, 900);
      await p.goto("/live");
      await p.waitFor("!!document.querySelector('thead th')");
      const sideways = await p.eval<boolean>("document.documentElement.scrollWidth > document.documentElement.clientWidth + 1");
      t.ok(!sideways, `${width}: no sideways scrollbar`);
      const align = await p.eval<number>(`(() => { const th = [...document.querySelectorAll('.dt-t thead th')]; const td = [...document.querySelector('.dt-t tbody tr').children]; return Math.max(...th.map((h, i) => Math.abs(h.getBoundingClientRect().left - td[i].getBoundingClientRect().left))); })()`);
      t.num(`${width}-align-px`, align);
      t.ok(align <= 0.5, `${width}: header and body cells line up (${align})`);
    }
    await p.viewport(1280, 900);
    await p.goto("/live");
    await p.waitFor("!document.querySelector('[data-tail-mode=live]').disabled");
    await p.click("[data-tail-mode=live]");
    await p.waitFor("document.querySelector('[data-conn]').getAttribute('data-conn') === 'live'", 5000);
    const widths = `(() => [...document.querySelectorAll('.dt-t thead th')].map((h) => h.getBoundingClientRect().width))()`;
    const w0 = await p.eval<number[]>(widths);
    let worst = 0;
    const end = Date.now() + Number(process.env.TABLE_SECONDS ?? 20) * 1000;
    while (Date.now() < end) {
      await sleep(1000);
      const w = await p.eval<number[]>(widths);
      worst = Math.max(worst, ...w.map((x, i) => Math.abs(x - w0[i])));
    }
    t.num("column-width-change-px", worst);
    t.ok(worst === 0, `the header cells keep their widths while rows arrive (${worst} px)`);
    await p.shot(join(t.shots, "live-table.png"));
    await p.click("[data-tail-mode=snapshot]");
    await p.eval("localStorage.removeItem('doze:tail-speed')");
  },

  async "live-above-the-fold"(t) {
    const p = t.page;
    for (const [w, h] of [[1440, 900], [1024, 768]] as const) {
      await p.viewport(w, h);
      await p.goto("/live");
      await p.waitFor("!!document.querySelector('.dt-t tbody tr')");
      await sleep(300);
      const r = await p.eval<{ toolbar: number; row: number; ih: number }>("({ toolbar: document.querySelector('.toolbar').getBoundingClientRect().bottom, row: document.querySelector('.dt-t tbody tr').getBoundingClientRect().bottom, ih: innerHeight })");
      t.num(`${w}x${h}-toolbar-bottom`, r.toolbar);
      t.num(`${w}x${h}-first-row-bottom`, r.row);
      t.ok(r.toolbar < r.ih, `${w}x${h}: the filters are on screen`);
      t.ok(r.row < r.ih, `${w}x${h}: the first row of the table is on screen without scrolling (${r.row} < ${r.ih})`);
      await p.shot(join(t.shots, `live-${w}x${h}.png`));
    }
    await p.viewport(1280, 800);
  },
};

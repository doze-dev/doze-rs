// Nothing overflows or is clipped: Home at 1024, 1280, 1440 and 1920 px wide, and every other page at 1024 (cards with a thin side
// column, long values). "Overflow": the document is wider than the window, or an element's content is wider than its box and it
// neither scrolls nor ends in an ellipsis on purpose.
import { join } from "node:path";
import type { T } from "../drive.ts";

const CHECK = `(() => {
  const doc = document.documentElement.scrollWidth - document.documentElement.clientWidth;
  const skip = (e) => e.closest('pre,.code,.dz-pop,[role=listbox],[role=menu],.mid,td.val,.dz-opt-label,.sr,.skip,svg,[hidden],.ovl,.t.wire .mid-a,[data-scaffold]');
  const bad = [];
  for (const e of document.querySelectorAll('main *')) {
    if (skip(e) || e.closest('[hidden]')) continue;
    const r = e.getBoundingClientRect();
    if (!r.width || !r.height) continue;
    const s = getComputedStyle(e);
    if (s.display === 'inline' || s.display === 'contents') continue;
    if (e.scrollWidth > e.clientWidth + 1 && !/auto|scroll/.test(s.overflowX)) bad.push(e.tagName + '.' + String(e.className).slice(0, 30) + ' ' + e.scrollWidth + '>' + e.clientWidth + ' "' + (e.textContent || '').trim().slice(0, 24) + '"');
  }
  return { doc, bad: bad.slice(0, 6), count: bad.length };
})()`;

const PAGES = [
  "/", "/live", "/live/clients", "/connect", "/connect/first-request", "/connect/advertised", "/topics", "/topics/new", "/topics/orders", "/topics/orders/partitions",
  "/topics/orders/config", "/topics/orders/consumers", "/topics/orders/log", "/topics/orders/producers", "/topics/orders/wire", "/groups", "/groups/fulfilment", "/groups/fulfilment/members",
  "/groups/fulfilment/lag", "/groups/analytics/lag?expiring=1", "/groups/fulfilment/story", "/jump", "/access", "/lab", "/settings", "/topics/orders.dlq", "/topics/inventory.levels",
];

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  async "home-fits-at-four-widths"(t: T) {
    const p = t.page;
    try {
      for (const [w, h] of [[1024, 768], [1280, 800], [1440, 900], [1920, 1080]]) {
        await p.viewport(w, h);
        for (const path of ["/", "/?hint=quiet"]) {
          await p.goto(path);
          await Bun.sleep(300);
          const r = await p.eval<{ doc: number; bad: string[]; count: number }>(CHECK);
          t.ok(r.doc <= 0, `${path} @${w}: the page is not wider than the window (${r.doc} px)`);
          t.ok(r.count === 0, `${path} @${w}: nothing overflows (${r.bad.join(" | ")})`);
          if (path === "/") {
            const g = await p.eval<{ cols: string[]; second: number; first: number }>(`(() => { const g = document.querySelector('.home-grid'); const c = [...g.children].map((e) => e.getBoundingClientRect()); return { cols: c.map((r) => Math.round(r.left) + ':' + Math.round(r.width)), first: c[0].width, second: c[1].left === c[0].left ? c[0].width : c[1].width }; })()`);
            if (w >= 1280) t.ok(g.second >= 380 && g.first > g.second, `@${w}: two columns, the second at least 380 px and the first the wider (${g.cols.join(" / ")})`);
            if (w === 1024) t.ok(g.cols[0].split(":")[0] === g.cols[1].split(":")[0], `@${w}: stacked under 1180 px`);
            await p.shot(join(t.shots, `home-${w}.png`));
          }
        }
      }
    } finally {
      await p.viewport(1280, 800);
    }
  },

  async "every-page-fits-at-1024"(t: T) {
    const p = t.page;
    let bad = 0;
    try {
      await p.viewport(1024, 768);
      for (const path of PAGES) {
        await p.goto(path);
        await Bun.sleep(250);
        const r = await p.eval<{ doc: number; bad: string[]; count: number }>(CHECK);
        if (r.doc > 0 || r.count > 0) bad++;
        t.ok(r.doc <= 0 && r.count === 0, `${path}: ${r.doc > 0 ? `page ${r.doc} px too wide; ` : ""}${r.bad.join(" | ")}`);
      }
    } finally {
      await p.viewport(1280, 800);
    }
    t.num("pages", PAGES.length);
    t.num("with overflow", bad);
  },
};

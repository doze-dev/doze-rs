// Calm pages stay still, and live pages move only as designed (the owner: "an overdose of flickering all over"). Against a REAL broker
// with traffic (LIVE_URL, default http://127.0.0.1:9094):
//   calm-pages        every calm page, 10 s: ZERO DOM mutations in `main` except the text of a relative time ("4 s ago") that the
//                     browser keeps, and zero animations; the topics list as pixels (table and header) 300 ms apart.
//   live-text-stable  the part of Home, Live, a topic and a group that is below the header, sampled every 250 ms: the boxes of every
//                     element that is not a tape row never change by a pixel while the values change.
import { join } from "node:path";
import type { T } from "../drive.ts";

// Opt-in: set LIVE_URL to a real broker's console (with traffic); without it the scenario passes quietly.
const LIVE = process.env.LIVE_URL ?? "";
const up = async () => { if (!LIVE) return false; try { return (await fetch(LIVE + "/live", { signal: AbortSignal.timeout(2000) })).ok; } catch { return false; } };

const WATCH = `(() => {
  const main = document.getElementById('main');
  const m = window.__calm = { mutations: [], anims: 0 };
  const rel = /\\d+(\\.\\d)? [smhd]( ago)?$/;
  new MutationObserver((list) => {
    for (const x of list) {
      if (x.type === 'characterData' && /\\d+(\\.\\d)? [smhd]( ago)?/.test(x.oldValue || '') && /\\d+(\\.\\d)? [smhd]( ago)?/.test(x.target.data)) continue; // a relative time, kept by the browser
      m.mutations.push(x.type + ' ' + (x.target.nodeName || '') + ' ' + String(x.target.id || x.target.className || x.target.parentElement && x.target.parentElement.className).slice(0, 30) + (x.type === 'attributes' ? ' @' + x.attributeName : ''));
    }
  }).observe(main, { subtree: true, childList: true, characterData: true, characterDataOldValue: true, attributes: true });
  const seen = new WeakSet();
  setInterval(() => { for (const a of document.getAnimations({ subtree: true })) if (!seen.has(a)) { seen.add(a); const t = a.effect && a.effect.target; if (t && t.closest && t.closest('#main')) m.anims++; } }, 25);
  return 1;
})()`;

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  async "calm-pages"(t: T) {
    const p = t.page;
    if (!(await up())) { t.num("skipped", "no real broker at " + LIVE); return; }
    for (const path of ["/topics", "/topics/orders", "/topics/orders/config", "/topics/orders/partitions", "/topics/orders/consumers", "/groups/fulfilment/lag", "/groups/fulfilment/members"]) {
      await p.goto(LIVE + path);
      await Bun.sleep(600);
      await p.eval(WATCH);
      await Bun.sleep(10000);
      const r = await p.eval<{ mutations: string[]; anims: number }>("window.__calm");
      t.num(`${path}: mutations / animations in 10 s`, `${r.mutations.length} / ${r.anims}`);
      t.ok(r.mutations.length === 0 && r.anims === 0, `${path}: nothing moved except relative times (${r.mutations.slice(0, 4).join(" | ")})`);
    }
    // The topics list as pixels, 300 ms apart: the table and the header.
    await p.goto(LIVE + "/topics");
    await Bun.sleep(600);
    const regions = await p.eval<{ x: number; y: number; width: number; height: number }[]>(`(() => { const r = (e) => { const b = e.getBoundingClientRect(); return { x: Math.floor(b.x), y: Math.round(b.y + scrollY), width: Math.ceil(b.width), height: Math.ceil(b.height) }; }; return [r(document.querySelector('.page-h')), r(document.querySelector('table.t'))]; })()`);
    const shots: Buffer[][] = [];
    for (let k = 0; k < 4; k++) {
      const row: Buffer[] = [];
      for (const [i, r] of regions.entries()) { const f = join(t.shots, `calm-topics-${k}-${i}.png`); await p.shot(f, r); row.push(Buffer.from(await Bun.file(f).arrayBuffer())); }
      shots.push(row);
      await Bun.sleep(300);
    }
    const same = shots.every((row) => row.every((b, i) => b.equals(shots[0][i])));
    t.ok(same, "the topics list's header and table are pixel-identical in 4 frames 300 ms apart");
  },

  async "live-text-stable"(t: T) {
    const p = t.page;
    if (!(await up())) { t.num("skipped", "no real broker at " + LIVE); return; }
    const pages: [string, string, string][] = [
      ["Home", "/", "#home-live"], ["Live (above the tape)", "/live", "#live-wire"], ["a topic", "/topics/orders", "#main"], ["a group", "/groups/fulfilment", "#main"],
    ];
    for (const [name, path, scope] of pages) {
      await p.goto(LIVE + path);
      await Bun.sleep(800);
      await p.eval(`(() => { window.__boxes = []; window.__samples = 0; window.__sel = ${JSON.stringify(scope)};
        const els = () => [...document.querySelector(window.__sel).querySelectorAll('*')].filter((e) => !e.closest('table.wire tbody,.dz-pop,[hidden]') && !e.closest('.page-h h1') && e.getBoundingClientRect().height > 0 && getComputedStyle(e).display !== 'inline');
        const snap = () => els().map((e) => { const r = e.getBoundingClientRect(); return [e.tagName + '.' + String(e.className).slice(0, 18) + '#' + (e.id || ''), r.top + scrollY, r.left, r.width, r.height]; });
        window.__first = snap();
        window.__changed = {};
        window.__iv = setInterval(() => { window.__samples++; const now = snap(); const byKey = new Map(); window.__first.forEach((f, i) => byKey.set(f[0] + ':' + i, f)); now.forEach((n, i) => { const f = window.__first[i]; if (!f || f[0] !== n[0]) { window.__changed['structure'] = (window.__changed['structure'] || 0) + 1; return; } if (Math.abs(f[1] - n[1]) > 0 || Math.abs(f[2] - n[2]) > 0 || Math.abs(f[3] - n[3]) > 0 || Math.abs(f[4] - n[4]) > 0) { window.__changed[n[0]] = (window.__changed[n[0]] || 0) + 1; } }); }, 250); return 1; })()`);
      await Bun.sleep(15000);
      const r = await p.eval<{ samples: number; changed: Record<string, number> }>("(() => { clearInterval(window.__iv); return { samples: window.__samples, changed: window.__changed }; })()");
      const n = Object.keys(r.changed).length;
      t.num(`${name}: samples / elements whose box changed`, `${r.samples} / ${n}${n ? " " + JSON.stringify(r.changed).slice(0, 200) : ""}`);
      t.ok(n === 0, `${name}: no element below the header moved or resized in ${r.samples} samples 250 ms apart (${JSON.stringify(r.changed).slice(0, 160)})`);
    }
  },
};

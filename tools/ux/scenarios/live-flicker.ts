// The Live page against a REAL broker with traffic must not flicker (the owner: "flickering like crazy"). Over 20 seconds of updates
// the scenario records, in Chrome, the things that flicker is made of:
//   (a) Web/CSS animations started on anything that is not a brand-new row (a block that dips in opacity on every update),
//   (b) elements of rows already on screen that were replaced or removed (rows morphed by position instead of by id), and text of
//       a surviving row that changed,
//   (c) layout shift of everything that is not a tape row (the header, the toolbar), and of the rows (new rows push the older ones
//       down: inherent in a newest-first tape; reported, and zero when the reader has scrolled away: the new rows are held back),
//   (d) the header, the toolbar and the table head as pixels, 3 s apart (identical),
//   (e) a row the reader hovers or focuses is the same element for as long as it is on the tape,
//   (f) scrollY, and the row under the reader's eye, do not move while updates arrive.
// It needs a real broker whose console is at LIVE_URL (default http://127.0.0.1:9094) with traffic; with none reachable it passes
// quietly (the fixtures tape does not update).
import { join } from "node:path";
import type { T } from "../drive.ts";

const LIVE = process.env.LIVE_URL ?? "http://127.0.0.1:9094";

const INSTALL = `(() => {
  const wire = document.getElementById('live-wire');
  const init = new WeakSet(); wire.querySelectorAll('*').forEach((e) => init.add(e));
  const m = window.__m = { anims: {}, removedMiddle: 0, replacedCells: 0, textInSurvivors: 0, newRows: 0, ids: new Map(), identityBroken: 0, shiftRows: 0, shiftOther: 0, samples: 0, focusLost: 0, notes: [] };
  const rowId = (r) => Number((r.getAttribute('data-row') || r.id.replace(/^wire-/, '')) || -1);
  const minId = () => Math.min(...[...wire.querySelectorAll('tbody tr[data-row]')].map(rowId));
  new MutationObserver((list) => {
    for (const x of list) {
      if (x.type === 'childList') {
        for (const n of x.removedNodes) {
          if (n.nodeType !== 1 || !init.has(n)) continue;
          if (n.tagName === 'TR') { if (n.hasAttribute('data-row') && rowId(n) > minId()) m.removedMiddle++; }
          else if (x.target.closest && x.target.closest('tbody tr') && x.target.closest('tbody tr').isConnected) m.replacedCells++;
        }
        for (const n of x.addedNodes) if (n.nodeType === 1 && n.tagName === 'TR' && !init.has(n)) m.newRows++;
      } else if (x.type === 'characterData') {
        const tr = x.target.parentElement && x.target.parentElement.closest('tbody tr');
        if (tr && init.has(tr) && tr.isConnected && !x.target.parentElement.closest('[data-vary],.fold')) m.textInSurvivors++;
      }
    }
  }).observe(wire, { subtree: true, childList: true, characterData: true });
  const seen = new WeakSet();
  setInterval(() => {
    m.samples++;
    for (const a of document.getAnimations({ subtree: true })) {
      if (seen.has(a)) continue; seen.add(a);
      const t = a.effect && a.effect.target; if (!t) continue;
      const tr = t.closest && t.closest('tbody tr');
      if (tr && !init.has(tr)) continue; // a brand-new row may highlight itself
      if (a.transitionProperty === 'background-color' && t.tagName === 'TD') continue; // the tint of a row that has arrived, fading out
      const k = (t.tagName || '?') + '.' + String(t.className).slice(0, 24) + ':' + (a.animationName || a.transitionProperty || 'web');
      m.anims[k] = (m.anims[k] || 0) + 1;
    }
    for (const r of wire.querySelectorAll('tbody tr[data-row]')) {
      const id = r.getAttribute('data-row'); const was = m.ids.get(id);
      if (was && was.deref() !== r) m.identityBroken++; else if (!was) m.ids.set(id, new WeakRef(r));
    }
  }, 16);
  new PerformanceObserver((l) => { for (const e of l.getEntries()) { if (e.hadRecentInput) continue; const rows = (e.sources || []).every((s) => s.node && ((s.node.closest && s.node.closest('table.wire tbody')) || !s.node.isConnected)); if (rows) m.shiftRows += e.value; else { m.shiftOther += e.value; m.notes.push(Math.round(e.value * 1000) / 1000 + ' ' + (e.sources || []).map((x) => x.node && ((x.node.tagName || '#t') + '.' + String(x.node.className || x.node.parentElement && x.node.parentElement.className).slice(0, 20) + ' ' + Math.round(x.previousRect.y) + '>' + Math.round(x.currentRect.y))).join(',')); } } }).observe({ type: 'layout-shift' });
  return 1;
})()`;

async function reachable(): Promise<boolean> {
  try { return (await fetch(LIVE + "/live", { signal: AbortSignal.timeout(2000) })).ok; } catch { return false; }
}

export const scenarios: Record<string, (t: T) => Promise<void>> = {
  async "live-no-flicker"(t: T) {
    const p = t.page;
    if (!(await reachable())) { t.num("skipped", "no real broker at " + LIVE); return; }
    await p.goto(LIVE + "/live");
    await p.waitFor("!!document.querySelector('#live-wire table.wire tbody tr[data-row]')", 8000);
    await Bun.sleep(800);
    await p.eval(INSTALL);
    // (e) hover and focus a row near the top and keep them.
    await p.eval("window.__f = document.querySelectorAll('#live-wire tbody tr[data-row]')[1].querySelector('a'); window.__f.focus(); window.__frow = window.__f.closest('tr'); 1");
    const box = await p.eval<{ x: number; y: number }>("(() => { const r = window.__frow.getBoundingClientRect(); return { x: r.left + 300, y: r.top + r.height / 2 }; })()");
    await p.move(box.x, box.y);
    // (d) pixels of what must not change: the title, the actions, the toolbar and the table head.
    const regions = async () => p.eval<{ x: number; y: number; width: number; height: number }[]>(`(() => { const r = (e) => { const b = e.getBoundingClientRect(); return { x: Math.floor(b.x), y: Math.round(b.y + scrollY), width: Math.ceil(b.width), height: Math.ceil(b.height) }; }; const w = document.getElementById('live-wire'); return [r(w.querySelector('.page-h h1')), r(w.querySelector('form.toolbar')), r(w.querySelector('table.wire thead'))]; })()`);
    const shotOf = async (name: string) => { const rs = await regions(); const out: Buffer[] = []; for (const [i, r] of rs.entries()) { const f = join(t.shots, `flicker-${name}-${i}.png`); await p.shot(f, r); out.push(Buffer.from(await Bun.file(f).arrayBuffer())); } return out; };
    const blockH0 = await p.eval<number>("document.querySelector('#live-wire .wire-dock').getBoundingClientRect().top + scrollY");
    const a = await shotOf("a");
    const rows0 = await p.eval<number>("document.querySelectorAll('#live-wire tbody tr[data-row]').length");
    await Bun.sleep(3000);
    const b = await shotOf("b");
    const same = a.map((x, i) => x.equals(b[i]));
    t.num("(d) regions (title, toolbar, head) at a / b", JSON.stringify([await regions()]));
    // A new refusal makes the callout above the tape longer (J-row: reserve its slot): the table then moves, which is data, not flicker.
    const blockH1 = await p.eval<number>("document.querySelector('#live-wire .wire-dock').getBoundingClientRect().top + scrollY");
    const moved = Math.abs(blockH1 - blockH0) > 0.5;
    if (moved) t.num("(d) note", "a refusal changed the callout above the table: the head moved, pixels not compared");
    t.ok(moved || same.every(Boolean), `(d) the title, the toolbar and the table head are identical 3 s apart (${same.join(", ")})`);
    // 20 seconds of updates.
    const y0 = await p.scrollY();
    await Bun.sleep(17000);
    const m = await p.eval<Record<string, unknown>>("(() => { const m = window.__m; return { anims: m.anims, removedMiddle: m.removedMiddle, replacedCells: m.replacedCells, notes: m.notes.slice(0, 8), textInSurvivors: m.textInSurvivors, newRows: m.newRows, identityBroken: m.identityBroken, shiftRows: m.shiftRows, shiftOther: m.shiftOther, samples: m.samples }; })()");
    const anims = Object.values(m.anims as Record<string, number>).reduce((x, y) => x + y, 0);
    t.num("(a) animations on anything but new rows", anims);
    if (anims) t.num("(a) which", JSON.stringify(m.anims));
    t.num("new rows in 20 s", m.newRows as number);
    t.num("(b) row cells replaced", m.replacedCells as number);
    t.num("(b) rows removed from the middle", m.removedMiddle as number);
    t.num("(b) text changed in a surviving row", m.textInSurvivors as number);
    t.num("(b) rows whose element changed for the same id", m.identityBroken as number);
    t.num("(c) layout shift, not rows", m.shiftOther as number);
    if (m.shiftOther) t.num("(c) which", JSON.stringify(m.notes));
    t.num("(c) layout shift, rows pushed down by new rows", m.shiftRows as number);
    t.ok(rows0 > 3 && (m.newRows as number) > 3, `real traffic arrived (${m.newRows} new rows)`);
    t.ok(anims === 0, "(a) no animation on a container, a cell, a text parent or an old row");
    t.ok(m.replacedCells === 0 && m.removedMiddle === 0 && m.identityBroken === 0, "(b) no row on screen was replaced or morphed into another");
    t.ok((m.textInSurvivors as number) === 0, "(b) no text of a surviving row changed except times and counters");
    const blockEnd = await p.eval<number>("document.querySelector('#live-wire .wire-dock').getBoundingClientRect().top + scrollY");
    t.ok((m.shiftOther as number) <= 0.01 || Math.abs(blockEnd - blockH0) > 0.5, `(c) nothing but the rows moved (${m.shiftOther}; the callout above grew: ${Math.abs(blockEnd - blockH0) > 0.5})`);
    t.ok(await p.eval(`window.__frow.isConnected ? document.activeElement === window.__f : true`), "(e) the focused row link kept the focus while its row was on the tape");
    t.ok(await p.eval(`window.__frow.isConnected ? !!document.querySelector('#live-wire tbody tr[data-row="' + window.__frow.getAttribute('data-row') + '"]') && document.querySelector('#live-wire tbody tr[data-row="' + window.__frow.getAttribute('data-row') + '"]') === window.__frow : true`), "(e) a row on the tape is the same element after the updates");
    t.near(await p.scrollY(), y0, 0.5, "(f) scrollY did not change while updates arrived");
    await p.shot(join(t.shots, "flicker-end.png"));
  },

  // The reader has scrolled down the tape: new rows are held back (a pill says how many), nothing under their eye moves, and the pill
  // (or scrolling back to the top) brings them in.
  async "live-scrolled-reader-is-not-pushed"(t: T) {
    const p = t.page;
    if (!(await reachable())) { t.num("skipped", "no real broker at " + LIVE); return; }
    await p.viewport(1280, 500);
    try {
      await p.goto(LIVE + "/live");
      await p.waitFor("!!document.querySelector('#live-wire table.wire tbody tr[data-row]')", 8000);
      await Bun.sleep(800);
      await p.scrollTo(260);
      const eye = await p.eval<{ id: string; top: number }>("(() => { const r = [...document.querySelectorAll('#live-wire tbody tr[data-row]')].find((r) => r.getBoundingClientRect().top > 120); window.__eye = r; return { id: r.getAttribute('data-row'), top: r.getBoundingClientRect().top }; })()");
      const y0 = await p.scrollY();
      await Bun.sleep(7000);
      const now = await p.eval<{ top: number; same: boolean }>("(() => ({ top: window.__eye.getBoundingClientRect().top, same: document.querySelector('#live-wire tbody tr[data-row=\"" + " ' + window.__eye.getAttribute('data-row') + '\"]') === window.__eye }))()").catch(() => ({ top: -1, same: false }));
      t.near(await p.scrollY(), y0, 0.5, "scrollY is the same after 7 s of traffic");
      t.near(now.top, eye.top, 1, "the row the reader is looking at has not moved");
      const pill = await p.eval<string>("(document.querySelector('.new-pill') || {}).textContent || ''");
      t.ok(/new/.test(pill), `a pill says there are new rows (“${pill}”)`);
      await p.click(".new-pill", { scroll: false });
      await Bun.sleep(500);
      t.ok((await p.scrollY()) < 40, "the pill takes the reader to the new rows");
      t.ok(!(await p.exists(".new-pill:not([hidden])")), "and goes");
    } finally {
      await p.viewport(1280, 800);
    }
  },
};

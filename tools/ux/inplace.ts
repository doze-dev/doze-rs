// The shared check of an in-place control (UX finding 10): a control inside a page changes its own region and nothing else.
//
//   await inPlace(t, { name: "topics: Show internal", act: () => p.click("..."), done: "...", above: [".page-h", "form.toolbar"], url: /internal=1/ });
//
// It records the document, the `main` element, scrollY, the boxes of what is above the data, the history length, the focus and the
// layout-shift total, runs the act, waits for `done`, and asserts: same document, same `main` (a boosted navigation would swap it),
// scrollY unchanged, nothing above moved, layout shift under 0.02, the address (when given) and the history (pushed or replaced).
// Every number is recorded for the report.

import type { T } from "./drive.ts";
import type { Page } from "./page.ts";

export interface Spec {
  name: string;
  act: () => Promise<void>;
  /** A JS expression that becomes true when the swap has landed. */
  done: string;
  /** Selectors whose boxes must not move (the header, the tabs, the toolbar). */
  above: string[];
  /** The address after the act, as a pattern of path+query. */
  url?: RegExp;
  /** The history grows by one (a pushed filter, the default) or stays (typing replaces). */
  history?: "push" | "replace" | "none";
  /** The selector (or "same") that holds the focus after: the control that was used. */
  focus?: string;
  /** Wait this long after `done` for fades and late shifts. */
  settle?: number;
}

/** Makes the page long enough to scroll without changing anything the region swaps: a spacer at the end of `main`. */
export async function pad(p: Page, px = 1800): Promise<void> {
  await p.eval(`(() => { if (!document.getElementById('__pad')) { const d = document.createElement('div'); d.id = '__pad'; d.style.height = '${px}px'; document.getElementById('main').appendChild(d); } })()`);
}

const BOXES = (sels: string[]) => `(() => { const o = {}; for (const s of ${JSON.stringify(sels)}) { const e = document.querySelector(s); if (!e) { o[s] = null; continue; } const r = e.getBoundingClientRect(); o[s] = [Math.round((r.left + scrollX) * 10) / 10, Math.round((r.top + scrollY) * 10) / 10, Math.round(r.width * 10) / 10, Math.round(r.height * 10) / 10]; } return o; })()`;

export async function inPlace(t: T, p: Page, s: Spec): Promise<void> {
  await p.eval("window.__doc = 'same'; window.__main = document.getElementById('main')");
  await p.resetShift();
  const y0 = await p.scrollY();
  const box0 = await p.eval<Record<string, number[] | null>>(BOXES(s.above));
  const hist0 = await p.eval<number>("history.length");
  const focus0 = s.focus === "same" ? await p.eval<string>("(document.activeElement && (document.activeElement.id || document.activeElement.getAttribute('aria-label') || document.activeElement.textContent.trim().slice(0, 30))) || ''") : "";
  await s.act();
  await p.waitFor(s.done, 4000);
  await Bun.sleep(s.settle ?? 380);
  const y1 = await p.scrollY();
  const box1 = await p.eval<Record<string, number[] | null>>(BOXES(s.above));
  const n = s.name;
  t.near(y1, y0, 0.5, `${n}: scrollY is unchanged`);
  t.ok(await p.eval("window.__doc === 'same'"), `${n}: the document is the same (no reload)`);
  t.ok(await p.eval("window.__main === document.getElementById('main')"), `${n}: \`main\` was not swapped (no page transition)`);
  for (const sel of s.above) {
    const a = box0[sel], b = box1[sel];
    if (!a || !b) { t.ok(!!a === !!b, `${n}: ${sel} ${a ? "vanished" : "appeared"}`); continue; }
    t.ok(a.every((v, i) => Math.abs(v - b[i]) <= 0.5), `${n}: ${sel} did not move (${a.join(",")} -> ${b.join(",")})`);
  }
  const cls = await p.shift();
  t.num(`${n}: cls`, cls);
  t.ok(cls < 0.02, `${n}: layout shift ${cls} under 0.02`);
  if (s.url) { const u = await p.eval<string>("location.pathname + location.search"); t.ok(s.url.test(u), `${n}: the address is ${u}`); }
  const hist1 = await p.eval<number>("history.length");
  const want = s.history ?? "push";
  if (want === "push") t.ok(hist1 === hist0 + 1, `${n}: the history grew by one (${hist0} -> ${hist1})`);
  else t.ok(hist1 === hist0, `${n}: the history did not grow (${hist0} -> ${hist1})`);
  if (s.focus) {
    const ok = s.focus === "same"
      ? await p.eval<boolean>(`(document.activeElement && (document.activeElement.id || document.activeElement.getAttribute('aria-label') || document.activeElement.textContent.trim().slice(0, 30))) === ${JSON.stringify(focus0)}`)
      : await p.eval<boolean>(`document.activeElement === document.querySelector(${JSON.stringify(s.focus)})`);
    t.ok(ok, `${n}: focus stays on the control`);
  }
  t.num(`${n}: scrollY`, y1);
}

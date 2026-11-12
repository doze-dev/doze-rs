// The scroll-and-jump hunt (UX finding 10, "it must be happening to other stuff too"): drive every same-page control of
// every built page of the fixtures server in real Chrome and record, for each one, what the click did.
//
//   bun tools/ux/pages-audit.ts http://127.0.0.1:19412 --from 0 --to 12 --json out.json     (a slice of the page list, so the
//   Chrome lock is held for under three minutes; run the slices one after another and join them with --md)
//   bun tools/ux/pages-audit.ts --md docs/port/console/ux-audit.md out1.json out2.json ...  (write the table)
//
// A "same-page control" is a link whose address differs from the page's only in its query or hash, a button of a GET form,
// or a <summary>. For each: scrollY and the boxes of the parts above the data (crumbs, header, tabs, callout, toolbar) before
// and after a real mouse click, whether the click navigated, the layout-shift total, and a verdict:
//   in place   no navigation, scroll kept, nothing above moved             (what every control should be)
//   reload     the click reloaded the page; nothing above moved, scroll was already at the top  (needs agent 1's swap to feel instant)
//   JUMP       the click reloaded the page AND the scroll was lost, or something above moved, or the page scrolled itself

import { Browser, type Box, type Page, moved } from "./pages-cdp";

export interface Row {
  page: string;
  control: string;
  kind: string;
  target: string;
  navigated: boolean;
  scrollBefore: number;
  scrollAfter: number;
  moved: string[];
  cls: number;
  /** The click replaced `main` (a boosted navigation): a cross-fade and a scroll to the top, not an in-place update. */
  mainSwapped: boolean;
  verdict: "in place" | "main swap" | "reload" | "JUMP";
}

export const PAGES = [
  "/topics",
  "/topics?q=orders",
  "/topics/orders",
  "/topics/orders/messages?partition=0&offset=16098",
  "/topics/orders/messages?isolation=read_committed",
  "/topics/orders/partitions",
  "/topics/orders/config",
  "/topics/orders/consumers",
  "/topics/orders.dlq",
  "/topics/notifications.email",
  "/groups",
  "/groups/fulfilment",
  "/groups/fulfilment/members",
  "/groups/fulfilment/lag",
  "/groups/analytics/lag",
  "/groups/search-indexer/lag",
  "/groups/payments-workers",
  "/live",
  "/live?client=orders",
  "/live/clients",
  "/connect",
  "/connect/advertised",
  "/",
  "/jump",
  "/topics/new",
];

/** The parts above the data: the callout counts only when it is above the toolbar. */
const PARTS_JS = `(() => {
  const box = (e) => { if (!e) return null; const r = e.getBoundingClientRect(); return { x: Math.round((r.left + scrollX) * 10) / 10, y: Math.round((r.top + scrollY) * 10) / 10, w: Math.round(r.width * 10) / 10, h: Math.round(r.height * 10) / 10 }; };
  const bar = document.querySelector('form.toolbar');
  const callout = document.querySelector('main .callout');
  const above = callout && bar && (callout.compareDocumentPosition(bar) & Node.DOCUMENT_POSITION_FOLLOWING) ? callout : null;
  return { 'nav.crumbs': box(document.querySelector('nav.crumbs')), '.page-h': box(document.querySelector('.page-h')), '.tabs': box(document.querySelector('.tabs')), 'callout': box(above), 'form.toolbar': box(bar && { getBoundingClientRect: () => { const r = bar.getBoundingClientRect(); return { left: r.left, top: r.top, width: r.width, height: 0 }; } }) };
})()`;

const ENUMERATE = `(() => {
  const here = location.pathname;
  const out = [];
  let i = 0;
  const label = (e) => (e.getAttribute('aria-label') || e.textContent || e.value || '').replace(/\\s+/g, ' ').trim().slice(0, 48);
  const visible = (e) => { const r = e.getBoundingClientRect(); const s = getComputedStyle(e); return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && !e.hidden; };
  for (const e of document.querySelectorAll('main a[href], main summary, main form button, main form input[type=submit]')) {
    if (!visible(e)) continue;
    let kind, target = '';
    if (e.tagName === 'A') {
      const u = new URL(e.href, location.href);
      if (u.origin !== location.origin || u.pathname !== here || (u.search === location.search && u.hash === location.hash)) continue;
      kind = 'link'; target = u.search + u.hash;
    } else if (e.tagName === 'SUMMARY') kind = 'summary';
    else kind = 'form button';
    e.dataset.auditIdx = String(i);
    out.push({ idx: i++, kind, label: label(e), target });
  }
  return out;
})()`;

async function parts(p: Page): Promise<Record<string, Box | null>> {
  return p.eval(PARTS_JS);
}

export async function auditPage(p: Page, base: string, path: string, limit = 60): Promise<Row[]> {
  await p.goto(base + path);
  const controls = (await p.eval<{ idx: number; kind: string; label: string; target: string }[]>(ENUMERATE)).slice(0, limit);
  const rows: Row[] = [];
  const seen = new Set<string>();
  for (const c of controls) {
    const key = c.kind + c.label + c.target;
    if (seen.has(key)) continue;
    seen.add(key);
    await p.goto(base + path);
    await p.eval(ENUMERATE);
    const centre = await p.eval<{ x: number; y: number; top: number } | null>(`(() => { const e = document.querySelector('[data-audit-idx="${c.idx}"]'); if (!e) return null; const r = e.getBoundingClientRect(); const top = Math.round(r.top + scrollY); return { x: 0, y: 0, top }; })()`);
    if (!centre) continue;
    // A person scrolls only to reach what they click: a control already in the window is clicked where it is.
    const inView = await p.eval<boolean>(`document.querySelector('[data-audit-idx="${c.idx}"]').getBoundingClientRect().bottom < innerHeight - 40`);
    if (!inView) await p.scrollTo(Math.max(0, centre.top - 350));
    const before = await parts(p);
    const sy0 = await p.scrollY;
    const at = await p.eval<{ x: number; y: number }>(`(() => { const r = document.querySelector('[data-audit-idx="${c.idx}"]').getBoundingClientRect(); return { x: r.left + Math.min(r.width / 2, 40), y: r.top + r.height / 2 }; })()`);
    await p.eval("window.__auditMain = document.querySelector('main')");
    const navigated = await p.clickAt(at.x, at.y);
    const mainSwapped = !navigated && (await p.eval<boolean>("window.__auditMain !== document.querySelector('main')"));
    const sy1 = await p.scrollY;
    const maxScroll = await p.eval<number>("Math.max(0, Math.round(document.documentElement.scrollHeight - innerHeight))");
    const after = await parts(p);
    const shifted = moved(before, after, 1);
    const cls = await p.cls();
    let verdict: Row["verdict"] = "in place";
    if (navigated) verdict = sy0 > 0 || shifted.length ? "JUMP" : "reload";
    else if ((sy1 !== sy0 && !(sy1 >= maxScroll - 1 && sy1 < sy0)) || shifted.length) verdict = "JUMP";
    else if (mainSwapped) verdict = "main swap";
    rows.push({ page: path, control: c.label || "(no text)", kind: c.kind, target: c.target, navigated, scrollBefore: sy0, scrollAfter: sy1, moved: shifted, cls, mainSwapped, verdict });
  }
  return rows;
}

function markdown(rows: Row[]): string {
  const head = "| Page | Control | Kind | Scroll before → after | Moved above | Verdict |\n|---|---|---|---|---|---|\n";
  const body = rows
    .map((r) => `| \`${r.page}\` | ${r.control.replace(/\|/g, "/")} | ${r.kind}${r.target ? ` \`${r.target.slice(0, 40)}\`` : ""} | ${r.scrollBefore} → ${r.scrollAfter} | ${r.moved.join("; ") || "nothing"} | **${r.verdict}** |`)
    .join("\n");
  return head + body + "\n";
}

if (import.meta.main) {
  const args = process.argv.slice(2);
  const mdAt = args.indexOf("--md");
  if (mdAt >= 0) {
    const files = args.slice(mdAt + 2);
    const rows: Row[] = (await Promise.all(files.map((f) => Bun.file(f).json()))).flat();
    const count = (v: string) => rows.filter((r) => r.verdict === v).length;
    const text = `# Console UX audit: every same-page control, driven in Chrome

Generated by \`tools/ux/pages-audit.ts\` over the fixtures server, real Chrome, a real mouse click on each control with the control in the middle of the window.
**${rows.length} controls: ${count("in place")} in place, ${count("reload")} reload (nothing moved), ${count("JUMP")} jump.**

- *in place*: no navigation, scroll kept, nothing above the data moved.
- *reload*: the click reloaded the page; the scroll was at the top and nothing above moved. It works with no script and flashes; agent 1's in-place swap makes it instant.
- *JUMP*: the click reloaded the page and the scroll was lost, or something above moved, or the page scrolled itself.

${markdown(rows)}`;
    await Bun.write(args[mdAt + 1], text);
    console.log(`${rows.length} rows -> ${args[mdAt + 1]}`);
    process.exit(0);
  }
  const base = args.find((a) => a.startsWith("http"))!.replace(/\/$/, "");
  const from = Number(args[args.indexOf("--from") + 1] || 0);
  const to = Number(args[args.indexOf("--to") + 1] || PAGES.length);
  const json = args[args.indexOf("--json") + 1];
  const browser = await Browser.launch();
  const rows: Row[] = [];
  try {
    const page = await browser.page();
    for (const path of PAGES.slice(from, to)) {
      const got = await auditPage(page, base, path);
      rows.push(...got);
      console.log(`${path}: ${got.map((r) => r.verdict).join(", ") || "no same-page control"}`);
    }
  } finally {
    await browser.close();
  }
  if (json) await Bun.write(json, JSON.stringify(rows, null, 1));
}

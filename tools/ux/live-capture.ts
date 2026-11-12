// A frame every 300 ms of the Live page of a real broker: screenshots for LOOKING, and for each frame the structure of the table (what
// is in the head, how many cells each row has, whether the page is wider than the window, which rows are half transparent).
//   bun tools/ux/live-capture.ts --url http://127.0.0.1:9094/live [--seconds 6] [--out target/ux] [--every 300]
// A frame is BAD when the head has other cells than the first frame's, a row is in the head, a body row has another number of cells than
// the head has columns, the page scrolls sideways, or an element of the table has an opacity below 1.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { Browser } from "./cdp.ts";
import { Page } from "./page.ts";

const args = process.argv.slice(2);
const opt = (n: string, d = "") => { const i = args.indexOf(n); return i >= 0 ? args[i + 1] : d; };
const url = opt("--url", "http://127.0.0.1:9094/live");
const seconds = Number(opt("--seconds", "6"));
const every = Number(opt("--every", "300"));
const out = opt("--out", "target/ux");
mkdirSync(out, { recursive: true });
const origin = new URL(url).origin;

const STRUCT = `(() => {
  const t = document.querySelector('table.wire'); if (!t) return null;
  const head = [...t.querySelectorAll('thead th, thead td')].map((c) => c.textContent.trim());
  const headRows = t.querySelectorAll('thead tr').length;
  const rows = [...t.querySelectorAll('tbody tr')];
  const cells = rows.map((r) => r.children.length);
  const dim = [...document.querySelectorAll('#live-wire table.wire, #live-wire table.wire *')].filter((e) => parseFloat(getComputedStyle(e).opacity) < 0.99).length;
  const card = t.parentElement.getBoundingClientRect();
  return { head, headRows, rows: rows.length, cells, inHead: t.querySelectorAll('thead tr').length - 1 + t.querySelectorAll('thead td').length,
    wide: document.documentElement.scrollWidth - document.documentElement.clientWidth, tableOver: Math.round(t.getBoundingClientRect().right - card.right), dim, ids: rows.map((r) => r.id).filter(Boolean).length };
})()`;

const b = await Browser.launch();
const bad: string[] = [];
try {
  const tab = await b.tab();
  const p = new Page(b, tab.sessionId, origin);
  await p.init();
  await p.goto(url);
  await p.waitFor("!!document.querySelector('table.wire tbody tr')", 8000);
  let first: string[] | null = null;
  const n = Math.floor((seconds * 1000) / every);
  for (let i = 0; i < n; i++) {
    const s = await p.eval<any>(STRUCT);
    await p.shot(join(out, `live-flicker-${String(i).padStart(2, "0")}.png`));
    if (s) {
      first ??= s.head;
      const problems: string[] = [];
      if (s.head.join("|") !== first.join("|")) problems.push(`the head changed: ${s.head.join(",")}`);
      if (s.headRows !== 1 || s.inHead > 0) problems.push(`rows in the head (${s.headRows} rows, ${s.inHead} td)`);
      const cols = first.length;
      const odd = s.cells.filter((c: number) => c !== cols).length;
      if (odd) problems.push(`${odd} rows with a cell count other than ${cols} (${[...new Set(s.cells)].join(",")})`);
      if (s.wide > 0) problems.push(`the page is ${s.wide}px wider than the window`);
      if (s.tableOver > 1) problems.push(`the table runs ${s.tableOver}px past its card`);
      if (s.dim > 0) problems.push(`${s.dim} elements are half transparent`);
      console.log(`frame ${i}: rows ${s.rows} ids ${s.ids} head ${s.head.length}${problems.length ? "  BAD: " + problems.join("; ") : ""}`);
      if (problems.length) bad.push(`frame ${i}`);
    }
    await Bun.sleep(every);
  }
} finally {
  b.kill();
}
console.log(bad.length ? `${bad.length} bad frames` : "no bad frame");
process.exit(bad.length ? 1 : 0);

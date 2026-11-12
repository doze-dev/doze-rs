// Optional, manual pixel check of ONE console page against its canvas board. Never run in CI.
//
//   bun tools/pixels/board-pixels.ts <board-id> --url <page url> --canvas <canvas dir>
//        [--mask x,y,w,h]... [--threshold 0.1] [--fail-pct 1.5] [--out target/board-diff/pixels] [--chrome <path>]
//   bun tools/pixels/board-pixels.ts <board-id> --page-png <screenshot.png> --canvas <canvas dir> ...   (compare two PNGs, no Chrome)
//
// It serves nothing: start the console (or `cargo run -p kafka-console --example boards`) yourself and pass the page's URL.
// It takes ONE screenshot with Chrome's own CLI (`--headless=new --screenshot`, window 1440 wide, as tall as the board's PNG,
// 20 s timeout, `nice -n 19`), compares it with `<canvas>/shots/<section>/<section>__<id>.png` per pixel (pixelmatch,
// --threshold default 0.1), and fails when more than --fail-pct percent (default 1.5) of pixels differ. A side-by-side PNG
// (board | page | difference) is written to --out. A screenshot cannot mask `[data-vary]`, so changing text is masked by
// rectangles: --mask x,y,w,h (repeatable, or separated by `;`) paints the rectangle in both images before comparing.
//
// Resource rules (a previous run once spawned 150 GB of Chrome):
//   - ONE Chrome at a time, never in the background; the process is killed (with its own user-data-dir, so only ours) after the page;
//   - before launching, `pgrep -fl "Google Chrome" | wc -l` must be 0, or it aborts (close Chrome first);
//   - `--all` is refused unless PIXELS_ALL=1, and even then pages go one after another.
// Own dependencies (pngjs, pixelmatch) are in tools/pixels/package.json; the rest of tools/ has none.

import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

const root = resolve(import.meta.dir, "../..");
const DEFAULT_CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

interface Args {
  ids: string[];
  url?: string;
  urlTemplate?: string;
  pagePng?: string;
  canvas?: string;
  masks: Rect[];
  threshold: number;
  failPct: number;
  out: string;
  chrome: string;
  all: boolean;
}
type Rect = [number, number, number, number];

export function parseArgs(argv: string[]): Args {
  const a: Args = { ids: [], masks: [], threshold: 0.1, failPct: 1.5, out: join(root, "target/board-diff/pixels"), chrome: DEFAULT_CHROME, all: false };
  for (let i = 0; i < argv.length; i++) {
    const t = argv[i];
    const need = () => {
      const v = argv[++i];
      if (v === undefined) throw new Error(`${t} needs a value`);
      return v;
    };
    if (t === "--url") a.url = need();
    else if (t === "--url-template") a.urlTemplate = need();
    else if (t === "--page-png") a.pagePng = resolve(need());
    else if (t === "--canvas") a.canvas = resolve(need());
    else if (t === "--mask") a.masks.push(...need().split(";").filter(Boolean).map(parseRect));
    else if (t === "--threshold") a.threshold = Number(need());
    else if (t === "--fail-pct") a.failPct = Number(need());
    else if (t === "--out") a.out = resolve(need());
    else if (t === "--chrome") a.chrome = need();
    else if (t === "--all") a.all = true;
    else if (t.startsWith("--")) throw new Error(`unknown option ${t}`);
    else a.ids.push(t);
  }
  return a;
}
function parseRect(s: string): Rect {
  const p = s.split(",").map(Number);
  if (p.length !== 4 || p.some((n) => !Number.isFinite(n) || n < 0)) throw new Error(`--mask wants x,y,w,h (got "${s}")`);
  return p as Rect;
}

/** Where a board's PNG is, found from the committed boards (the section is the directory). */
function shotPath(canvas: string, id: string): string {
  const dir = join(root, "crates/kafka-console/design/boards");
  for (const section of readdirSync(dir)) {
    if (existsSync(join(dir, section, `${id}.html`))) return join(canvas, "shots", section, `${section}__${id}.png`);
  }
  throw new Error(`no board "${id}" in crates/kafka-console/design/boards`);
}

const chromeCount = (): number => {
  const out = Bun.spawnSync(["pgrep", "-fl", "Google Chrome"]).stdout.toString();
  return out.split("\n").filter((l) => l.trim()).length;
};

/** One screenshot with Chrome's own CLI. Returns the PNG path. Always leaves no Chrome of ours behind. */
function screenshot(chrome: string, url: string, height: number, outPng: string): void {
  const n = chromeCount();
  if (n !== 0) throw new Error(`${n} Chrome process(es) are running; close them first (this tool starts one Chrome at a time and refuses to start beside others)`);
  const profile = mkdtempSync(join(tmpdir(), "board-pixels-chrome-"));
  try {
    const r = Bun.spawnSync(
      ["nice", "-n", "19", chrome, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-first-run", "--no-default-browser-check", `--user-data-dir=${profile}`, `--window-size=1440,${height}`, `--screenshot=${outPng}`, url],
      { timeout: 20_000, killSignal: "SIGKILL", stdout: "ignore", stderr: "ignore" },
    );
    if (r.exitCode !== 0 && !existsSync(outPng)) throw new Error(`Chrome did not produce a screenshot (exit ${r.exitCode}, timeout 20 s)`);
  } finally {
    // Kill any helper that is still ours (matched by our own profile directory), then remove the profile.
    Bun.spawnSync(["pkill", "-9", "-f", "--", `--user-data-dir=${profile}`]);
    rmSync(profile, { recursive: true, force: true });
    const left = chromeCount();
    if (left !== 0) console.error(`warning: ${left} Chrome process(es) still running after the screenshot`);
  }
}

function readPng(path: string): PNG {
  return PNG.sync.read(readFileSync(path));
}
function paint(img: PNG, masks: Rect[]): void {
  for (const [x, y, w, h] of masks) {
    for (let j = y; j < Math.min(img.height, y + h); j++)
      for (let i = x; i < Math.min(img.width, x + w); i++) {
        const k = (j * img.width + i) * 4;
        img.data[k] = 255;
        img.data[k + 1] = 0;
        img.data[k + 2] = 255;
        img.data[k + 3] = 255;
      }
  }
}
function crop(img: PNG, w: number, h: number): PNG {
  if (img.width === w && img.height === h) return img;
  const out = new PNG({ width: w, height: h });
  PNG.bitblt(img, out, 0, 0, w, h, 0, 0);
  return out;
}

export interface PixelResult {
  id: string;
  width: number;
  height: number;
  changed: number;
  pct: number;
  failPct: number;
  pass: boolean;
  sizeNote: string | null;
  sideBySide: string;
}

export function compareImages(id: string, boardPng: string, pagePng: string, a: Pick<Args, "masks" | "threshold" | "failPct" | "out">): PixelResult {
  let board = readPng(boardPng);
  let page = readPng(pagePng);
  const w = Math.min(board.width, page.width);
  const h = Math.min(board.height, page.height);
  const sizeNote = board.width !== page.width || board.height !== page.height ? `board ${board.width}x${board.height}, page ${page.width}x${page.height}: compared the common ${w}x${h}` : null;
  board = crop(board, w, h);
  page = crop(page, w, h);
  paint(board, a.masks);
  paint(page, a.masks);
  const diff = new PNG({ width: w, height: h });
  const changed = pixelmatch(board.data, page.data, diff.data, w, h, { threshold: a.threshold });
  const pct = (changed / (w * h)) * 100;
  const side = new PNG({ width: w * 3, height: h });
  PNG.bitblt(board, side, 0, 0, w, h, 0, 0);
  PNG.bitblt(page, side, 0, 0, w, h, w, 0);
  PNG.bitblt(diff, side, 0, 0, w, h, w * 2, 0);
  mkdirSync(a.out, { recursive: true });
  const sideBySide = join(a.out, `${id}.png`);
  writeFileSync(sideBySide, PNG.sync.write(side));
  return { id, width: w, height: h, changed, pct, failPct: a.failPct, pass: pct <= a.failPct && sizeNote === null, sizeNote, sideBySide };
}

if (import.meta.main) {
  try {
    const a = parseArgs(process.argv.slice(2));
    if (a.all) {
      if (process.env.PIXELS_ALL !== "1") throw new Error("--all is refused: it would start a Chrome per board. Check pages one at a time, or set PIXELS_ALL=1 knowingly");
      if (!a.urlTemplate || !a.urlTemplate.includes("{id}") || !a.canvas) throw new Error("--all needs --url-template <url with {id}> and --canvas");
      const dir = join(root, "crates/kafka-console/design/boards");
      const ids = readdirSync(dir).flatMap((sec) => readdirSync(join(dir, sec)).map((f) => f.replace(/\.html$/, "")));
      let failed = 0;
      for (const id of ids) {
        // One Chrome at a time, each killed before the next.
        const boardPng = shotPath(a.canvas, id);
        const pagePng = join(a.out, `${id}.page.png`);
        mkdirSync(a.out, { recursive: true });
        screenshot(a.chrome, a.urlTemplate.replace("{id}", id), readPng(boardPng).height, pagePng);
        const r = compareImages(id, boardPng, pagePng, a);
        console.log(`${r.pass ? "ok  " : "FAIL"} ${id}: ${r.pct.toFixed(2)}%`);
        if (!r.pass) failed++;
      }
      process.exit(failed ? 1 : 0);
    }
    if (a.ids.length !== 1) throw new Error("usage: board-pixels.ts <board-id> (--url <url> | --page-png <file>) --canvas <canvas dir> [--mask x,y,w,h] [--threshold 0.1] [--fail-pct 1.5]");
    if (!a.canvas) throw new Error("--canvas <canvas dir> is required (the folder with shots/)");
    if (!a.url && !a.pagePng) throw new Error("give --url (a running page) or --page-png (an existing screenshot)");
    const id = a.ids[0];
    const boardPng = shotPath(a.canvas, id);
    if (!existsSync(boardPng)) throw new Error(`board screenshot not found: ${boardPng}`);
    let pagePng = a.pagePng;
    if (!pagePng) {
      mkdirSync(a.out, { recursive: true });
      pagePng = join(a.out, `${id}.page.png`);
      screenshot(a.chrome, a.url!, readPng(boardPng).height, pagePng);
    }
    const r = compareImages(id, boardPng, pagePng, a);
    if (r.sizeNote) console.log("note: " + r.sizeNote);
    console.log(`${r.pass ? "ok  " : "FAIL"} ${id}: ${r.changed} of ${r.width * r.height} pixels differ (${r.pct.toFixed(2)}%, limit ${r.failPct}%)`);
    console.log(`side by side (board | page | difference): ${r.sideBySide}`);
    process.exit(r.pass ? 0 : 1);
  } catch (e) {
    console.error("board-pixels: " + (e as Error).message);
    process.exit(2);
  }
}

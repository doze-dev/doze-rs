// The interaction tests: drive the console in real Chrome as a person would and assert NUMBERS (scroll, boxes, layout shift,
// rects, focus, console errors). Not in CI (it needs Chrome). How to run it: docs/port/console/ux-tests.md.
//
//   cargo run -p kafka-console --features examples --example boards 127.0.0.1:19293      # the fixtures server, your own port
//   bun tools/ux/drive.ts --url http://127.0.0.1:19293 [--only drawer,select] [--shots target/ux] [--list]
//
// Scenarios are small named functions in tools/ux/scenarios/*.ts, each file exporting `scenarios: Record<string, (t: T) => Promise<void>>`.
// A scenario asserts with `t.ok(cond, message)`, `t.near(a, b, tolerance, message)` and records numbers with `t.num(name, value)`;
// every scenario also fails on any console error, warning, exception or CSP violation the page produced while it ran.
// Chrome: one at a time (it refuses to start when another is running); take the shared lock around a run when other agents are
// working (docs/port/console/ux-tests.md).

import { readdirSync, writeFileSync, mkdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { Browser, DEFAULT_CHROME } from "./cdp.ts";
import { Page } from "./page.ts";

export interface T {
  page: Page;
  shots: string;
  ok(cond: unknown, message: string): void;
  near(a: number, b: number, tolerance: number, message: string): void;
  num(name: string, value: number | string): void;
}
export type Scenario = (t: T) => Promise<void>;

interface Result { name: string; ok: boolean; ms: number; numbers: Record<string, number | string>; failures: string[] }

const args = process.argv.slice(2);
const opt = (name: string, def = ""): string => { const i = args.indexOf(name); return i >= 0 ? (args[i + 1] ?? def) : def; };
const base = opt("--url").replace(/\/$/, "");
const only = opt("--only") ? opt("--only").split(",") : [];
const shots = resolve(opt("--shots", "target/ux"));
const chrome = opt("--chrome", DEFAULT_CHROME);

const here = resolve(import.meta.dir, "scenarios");
const all: Record<string, Scenario> = {};
for (const f of readdirSync(here).filter((n) => n.endsWith(".ts")).sort()) {
  const mod = (await import(join(here, f))) as { scenarios?: Record<string, Scenario> };
  for (const [name, fn] of Object.entries(mod.scenarios ?? {})) all[name] = fn;
}

if (args.includes("--list")) { console.log(Object.keys(all).join("\n")); process.exit(0); }
if (!base) { console.error("usage: bun tools/ux/drive.ts --url http://127.0.0.1:<port> [--only a,b] [--shots dir] [--list]"); process.exit(2); }

const names = Object.keys(all).filter((n) => only.length === 0 || only.some((o) => n === o || n.startsWith(o)));
mkdirSync(shots, { recursive: true });
const results: Result[] = [];
const browser = await Browser.launch(chrome);
try {
  const tab = await browser.tab();
  const page = new Page(browser, tab.sessionId, base);
  await page.init();
  for (const name of names) {
    const r: Result = { name, ok: true, ms: 0, numbers: {}, failures: [] };
    const t: T = {
      page, shots,
      ok: (c, m) => { if (!c) { r.ok = false; r.failures.push(m); } },
      near: (a, b, tol, m) => { if (!(Math.abs(a - b) <= tol)) { r.ok = false; r.failures.push(`${m}: ${a} vs ${b} (tolerance ${tol})`); } },
      num: (k, v) => { r.numbers[k] = typeof v === "number" ? Math.round(v * 1000) / 1000 : v; },
    };
    page.clearLogs();
    const t0 = Date.now();
    try {
      await all[name](t);
    } catch (e) {
      r.ok = false;
      r.failures.push(`threw: ${(e as Error).message}`);
      try { await page.shot(join(shots, `${name}-failed.png`)); } catch { /* the tab is gone */ }
    }
    if (page.logs.length) { r.ok = false; r.failures.push(...page.logs.map((l) => `console: ${l}`)); }
    r.ms = Date.now() - t0;
    results.push(r);
    console.log(`${r.ok ? "PASS" : "FAIL"}  ${name}  (${r.ms} ms)  ${Object.entries(r.numbers).map(([k, v]) => `${k}=${v}`).join(" ")}`);
    for (const f of r.failures) console.log(`      - ${f}`);
  }
} finally {
  browser.kill();
}
writeFileSync(join(shots, "results.json"), JSON.stringify(results, null, 2));
const bad = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - bad} passed, ${bad} failed (${results.length} scenarios)`);
process.exit(bad ? 1 : 0);

// The board-diff gate: compares the console's rendered pages with the canvas boards by signature.
//
//   bun tools/board-diff.ts [--ids a,b] [--dir target/board-diff] [--live-dir target/board-diff-live] [--base /]
//                           [--require a,b | --require all | --require-file f] [--slice 9a] [--list] [--json]
//                           [--routes f] [--allow f] [--snapshot f] [--boards dir]
//
// For each board id (default: every board in design/boards.json) it looks for `<dir>/<id>.html` (the fixture render,
// compared strictly) and `<live-dir>/<id>.html` (the live render, compared on structure only). A board with no render
// is "not built" and is NOT a failure, unless --require names it: that is how a slice adopts its boards one by one.
// The exit status is 1 when a built board differs from its board by anything the allow-list does not name, when the
// allow-list itself is wrong (no reason, unknown board, stale entry, expired), or when a required board is not built;
// 2 when the inputs are unusable (snapshot missing or out of date with the boards).
//
// What it checks and does not: docs/port/console/gate.md. The signature: tools/board-signature.ts.

import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { designDir, boardsDir, parseArgs, root } from "./lib/boards.ts";
import { type Difference, compare, describe } from "./lib/compare.ts";
import { type AllowEntry, matches, validateAllow } from "./lib/allow.ts";
import { type Mode, type Options, type Routes, type Signature, signature, stableStringify } from "./board-signature.ts";
import { buildSnapshot } from "./boards-snapshot.ts";

export interface DiffOptions {
  dir: string;
  liveDir: string;
  boardsDir: string;
  snapshot: string;
  routes: string;
  allow: string;
  base: string;
  ids?: string[];
  require: string[] | "all";
  slice?: string;
}

export interface RenderResult {
  file: string;
  /** Differences the allow-list does not name. */
  differences: Difference[];
  /** Differences it does, with the entry's reason. */
  allowed: Array<Difference & { reason: string }>;
}
export interface BoardResult {
  id: string;
  section: string;
  fixture: RenderResult | null;
  live: RenderResult | null;
}
export interface Report {
  boards: BoardResult[];
  errors: string[];
  notes: string[];
  summary: { total: number; built: number; passed: number; failed: number; notBuilt: number; allowed: number };
  ok: boolean;
  /** 0, 1 or 2 */
  code: number;
}

export function defaultOptions(): DiffOptions {
  return {
    dir: join(root, "target/board-diff"),
    liveDir: join(root, "target/board-diff-live"),
    boardsDir,
    snapshot: join(designDir, "boards.json"),
    routes: join(designDir, "routes.json"),
    allow: join(designDir, "allow.json"),
    base: "/",
    require: [],
  };
}

function readJson<T>(path: string): T | null {
  return existsSync(path) ? (JSON.parse(readFileSync(path, "utf8")) as T) : null;
}

export function runDiff(o: DiffOptions): Report {
  const errors: string[] = [];
  const notes: string[] = [];
  const empty = (code: number): Report => ({ boards: [], errors, notes, summary: { total: 0, built: 0, passed: 0, failed: 0, notBuilt: 0, allowed: 0 }, ok: false, code });

  // The snapshot must be the one the boards give.
  if (!existsSync(o.snapshot)) {
    errors.push(`${o.snapshot} does not exist: run bun tools/boards-snapshot.ts`);
    return empty(2);
  }
  const fresh = buildSnapshot(o.boardsDir);
  const text = readFileSync(o.snapshot, "utf8");
  if (text !== stableStringify(fresh.snapshot) + "\n") {
    errors.push("boards.json is out of date with design/boards: run bun tools/boards-snapshot.ts and commit it");
    return empty(2);
  }
  const snap = JSON.parse(text) as { boards: Record<string, { section: string; signature: Signature }> };
  const allIds = Object.keys(snap.boards).sort();
  const required = o.require === "all" ? allIds : o.require;
  // A required board is always compared, whatever --ids says.
  const ids = [...new Set([...(o.ids ?? allIds), ...required])];
  for (const id of ids) if (!snap.boards[id]) errors.push(`unknown board id "${id}"`);
  if (errors.length) return empty(2);

  // The Rust test writes { "screens": { id: pattern } }; the tool tests pass the flat map. Both are read.
  const rawRoutes = readJson<Routes & { screens?: Routes }>(o.routes);
  const routes: Routes | null = rawRoutes && typeof rawRoutes.screens === "object" ? rawRoutes.screens : rawRoutes;
  if (!routes) notes.push(`${o.routes} is missing: rendered links cannot be resolved to screens, so every link is "unresolved" (?path)`);
  const opts: Options = { routes, base: o.base };

  const rawAllow = readJson<unknown>(o.allow) ?? [];
  const { entries, errors: allowErrors } = validateAllow(rawAllow, new Set(allIds), o.slice);
  errors.push(...allowErrors);
  const used = new Set<AllowEntry>();
  const judged = new Set<AllowEntry>();

  const results: BoardResult[] = [];
  for (const id of ids) {
    const b = snap.boards[id];
    const one = (dir: string, mode: Mode, render: "fixture" | "live"): RenderResult | null => {
      const file = join(dir, `${id}.html`);
      if (!existsSync(file)) return null;
      const diffs = compare(b.signature, signature(readFileSync(file, "utf8"), opts), mode);
      const res: RenderResult = { file, differences: [], allowed: [] };
      for (const e of entries) if (e.board === id && (!e.render || e.render === render)) judged.add(e);
      for (const d of diffs) {
        const e = entries.find((x) => x.board === id && matches(x, d, render));
        if (e) {
          used.add(e);
          res.allowed.push({ ...d, reason: e.reason });
        } else res.differences.push(d);
      }
      return res;
    };
    results.push({ id, section: b.section, fixture: one(o.dir, "strict", "fixture"), live: one(o.liveDir, "live", "live") });
  }
  for (const e of judged) if (!used.has(e)) errors.push(`stale allow-list entry: ${e.board} / ${e.field}${e.selector ? ` / ${e.selector}` : ""} matches no difference (${e.reason})`);
  for (const id of required) {
    const r = results.find((x) => x.id === id);
    if (r && !r.fixture && !r.live) errors.push(`required board "${id}" is not built (no ${join(o.dir, id + ".html")})`);
  }

  const built = results.filter((r) => r.fixture || r.live);
  const failed = built.filter((r) => [r.fixture, r.live].some((x) => x && x.differences.length > 0));
  const summary = {
    total: results.length,
    built: built.length,
    passed: built.length - failed.length,
    failed: failed.length,
    notBuilt: results.length - built.length,
    allowed: results.reduce((n, r) => n + (r.fixture?.allowed.length ?? 0) + (r.live?.allowed.length ?? 0), 0),
  };
  const ok = errors.length === 0 && failed.length === 0;
  return { boards: results, errors, notes, summary, ok, code: ok ? 0 : 1 };
}

function printReport(r: Report, list: boolean): void {
  for (const n of r.notes) console.log("note: " + n);
  for (const b of r.boards) {
    const built = b.fixture || b.live;
    if (list) {
      const parts = [`fixture ${b.fixture ? "built" : "not built"}`, `live ${b.live ? "built" : "not built"}`];
      console.log(`${b.id.padEnd(30)} ${b.section.padEnd(10)} ${parts.join(", ")}`);
      continue;
    }
    if (!built) continue;
    for (const [name, res] of [["fixture", b.fixture], ["live", b.live]] as const) {
      if (!res) continue;
      const bad = res.differences.length > 0;
      console.log(`${bad ? "FAIL" : "ok  "} ${b.id} (${name})${res.allowed.length ? `  ${res.allowed.length} allowed` : ""}`);
      if (bad) {
        for (const d of res.differences) console.log("       " + describe(d));
        console.log(`       file: ${res.file}`);
      }
      for (const a of res.allowed) console.log(`       allowed ${describe(a)}  [${a.reason}]`);
    }
  }
  if (list) return;
  for (const e of r.errors) console.log("error: " + e);
  const s = r.summary;
  console.log(`${s.built} of ${s.total} boards built: ${s.passed} pass, ${s.failed} fail; ${s.notBuilt} not built${s.allowed ? `; ${s.allowed} differences allowed` : ""}${r.errors.length ? `; ${r.errors.length} error(s)` : ""}`);
}

if (import.meta.main) {
  const { flags, values } = parseArgs(process.argv.slice(2), ["ids", "dir", "live-dir", "base", "require", "require-file", "slice", "routes", "allow", "snapshot", "boards"]);
  const o = defaultOptions();
  if (values.dir) o.dir = resolve(values.dir);
  if (values["live-dir"]) o.liveDir = resolve(values["live-dir"]);
  if (values.base) o.base = values.base;
  if (values.routes) o.routes = resolve(values.routes);
  if (values.allow) o.allow = resolve(values.allow);
  if (values.snapshot) o.snapshot = resolve(values.snapshot);
  if (values.boards) o.boardsDir = resolve(values.boards);
  if (values.slice) o.slice = values.slice;
  if (values.ids) o.ids = values.ids.split(",").map((s) => s.trim()).filter(Boolean);
  if (values.require) o.require = values.require === "all" ? "all" : values.require.split(",").map((s) => s.trim()).filter(Boolean);
  if (values["require-file"]) {
    const ids = readFileSync(resolve(values["require-file"]), "utf8").split(/\s+/).filter((l) => l && !l.startsWith("#"));
    o.require = o.require === "all" ? "all" : [...o.require, ...ids];
  }
  const report = runDiff(o);
  if (flags.has("json")) console.log(JSON.stringify(report, null, 2));
  else printReport(report, flags.has("list"));
  if (flags.has("list")) process.exit(report.code === 2 ? 2 : 0);
  process.exit(report.code);
}

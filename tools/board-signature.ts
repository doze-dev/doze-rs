// The board signature: what a console page must have, extracted the same way from a canvas board and from a rendered page.
//
//   signature(html, { routes, base }) -> Signature
//
// ONE function reads both. A board is recognised by its first-line front matter (`<!--screen {json} -->`); anything
// else is a rendered page. Everything the two have in common is read by the same code, so they are comparable by
// construction; the only board-specific parts are the things the canvas kit adds around a board when it draws it
// (top bar, rail, breadcrumb: the "landmarks" and the title), which a board gets from its front matter.
//
// The signature never contains cell text or numbers. Text is read only where it is a label (a heading, a column,
// a tab, a button, a `dt`, a field label, the lead of a callout). It is normalised: glyph tokens `[[g:name 16]]`
// and SVG are dropped, whitespace is collapsed, `[data-vary]` content is ignored, and `.mono` content inside a
// heading is masked as `{}` (names are data). See docs/port/console/gate.md for the full rules.

import { type El, classes, closest, descendants, findAll, hasClass, parse, rawText } from "./lib/html.ts";

export const SIGNATURE_VERSION = 1;

/** The state words a page may use: `st ok`, `rw refused`, ... */
export const STATE_CLASSES = ["st", "rw"] as const;
export const STATE_VARIANTS = ["ok", "warn", "bad", "refused", "info", "idle"] as const;
/** The variant classes of a button that the signature keeps. */
export const BUTTON_VARIANTS = ["pri", "danger", "fill", "sm"] as const;

export interface Heading {
  /** Full text, `[data-vary]` ignored. */
  text: string;
  /** True when the heading is a name (the topic or group the page is about): the live gate does not compare it. */
  data: boolean;
}
export interface TableSig {
  /** The `th` texts in order. */
  cols: string[];
  /** Whether the table has body rows (the live gate does not compare this). */
  rows: boolean;
  /** `table.t.wire`, the dense wire table. */
  wire: boolean;
}
export interface TabSig {
  /** Label without its count. */
  label: string;
  on: boolean;
}
export interface ButtonSig {
  label: string;
  /** The label with `.mono` content masked and digits replaced by `#`: what the live gate compares. */
  loose: string;
  /** A subset of BUTTON_VARIANTS, in that order. */
  variants: string[];
  /** `op:Name` (data-op), `ui:kind` (data-ui), `to:<screen id>` (a link), `submit`, or `none`. */
  action: string;
}
export interface NoteSig {
  kind: "callout" | "banner";
  /** `warn`, `bad`, `refused` or `` (the plain callout, the green banner). */
  variant: string;
  /** The first bold phrase, or `` if there is none. */
  lead: string;
}
export interface CardSig {
  /** The card's heading, unique on the page (`#2` is appended to a duplicate), or `card#n` for a card without one; `page` for everything outside a card. */
  key: string;
  dt: string[];
  fields: string[];
  notes: NoteSig[];
}
export interface Signature {
  version: number;
  /** `page`, or `overlay` for a dialog or drawer. */
  kind: "page" | "overlay";
  /** (a) The last crumb. Null on an overlay (it has none of its own) and on a page with no breadcrumb. */
  title: string | null;
  /** (a) The h1 texts. */
  h1: Heading[];
  /** (b) `h2 Text` / `h3 Text`, in order. */
  headings: string[];
  /** (c) `table` elements in order. */
  tables: TableSig[];
  /** (d) Tab labels in order. */
  tabs: TabSig[];
  /** (e) Every `.btn` and `button`. */
  buttons: ButtonSig[];
  /** (f) Screen ids linked, sorted, unique. A rendered link that routes.json cannot resolve is `?<path>`; one that several screens share is `a|b`. */
  links: string[];
  /** (g) Cards, in order, then the page level (key `page`) last. */
  cards: CardSig[];
  /** (h) `data-extra` values, sorted, unique. */
  extras: string[];
  /** (i) `data-op` name -> how many. */
  ops: Record<string, number>;
  /** (j) `st ok`, `rw refused`, ... -> how many outside table cells; `st ok in rows` -> 1 if any row has it. */
  states: Record<string, number>;
  /** (k) `banner`, `breadcrumb`, `main`, `rail`, `dialog`. */
  landmarks: string[];
}

/** screen id -> route pattern (`/topics/{topic}`), or several patterns. Generated from the Rust route table. */
export type Routes = Record<string, string | string[]>;
export interface Options {
  routes?: Routes | null;
  /** The console's path prefix (`--console-prefix`), stripped from hrefs before they are matched. */
  base?: string;
}

/** The entry screen of each canvas section, for `href="@live"` style links. Same table as the canvas kit (kit/build.ts SERVICES). */
export const SECTION_ENTRY: Record<string, string> = {
  home: "home", live: "live-wire", topics: "topics-list", log: "topic-log", config: "topic-config",
  producers: "topic-producers", groups: "groups-list", access: "access-open", lab: "lab-home", settings: "settings-home",
};

export interface FrontMatter {
  id: string;
  title: string;
  kind: string;
  from: string;
  [k: string]: unknown;
}

export function frontMatter(html: string): FrontMatter | null {
  const m = html.match(/^\s*<!--screen\s*([\s\S]*?)-->/);
  if (!m) return null;
  try {
    return JSON.parse(m[1]) as FrontMatter;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------------------------------------------
// Text

const norm = (s: string): string => s.replace(/\[\[g:[^\]]*\]\]/g, "").replace(/\s+/g, " ").trim();
const vary = (e: El): boolean => "data-vary" in e.attrs;
const scaffold = (e: El): boolean =>
  "data-scaffold" in e.attrs || hasClass(e, "from") || hasClass(e, "crumbs") || hasClass(e, "rail") || hasClass(e, "topbar");

/** Text of an element: data-vary and scaffolding dropped. */
function text(el: El): string {
  if (vary(el)) return "";
  return norm(rawText(el, (e) => (vary(e) || scaffold(e) ? "" : undefined)));
}
/** Text with `.mono` content masked as `{}`. */
function maskedText(el: El): string {
  if (vary(el)) return "";
  return norm(rawText(el, (e) => (vary(e) || scaffold(e) ? "" : hasClass(e, "mono") ? "{}" : undefined)));
}
const looseOf = (masked: string): string => masked.replace(/\d[\d,.]*/g, "#");

// ---------------------------------------------------------------------------------------------------------------
// Routes

interface Compiled {
  id: string;
  segs: string[];
  literals: number;
}
function splitPath(path: string): string[] {
  return path.split("/").filter(Boolean);
}
const isParam = (seg: string): boolean => /^\{[^}]*\}$/.test(seg) || seg.startsWith(":");
const isRest = (seg: string): boolean => /^\{\*[^}]*\}$/.test(seg) || /^\{[^}]*\.\.\.\}$/.test(seg);

const compiledCache = new WeakMap<Routes, Compiled[]>();
function compile(routes: Routes): Compiled[] {
  let c = compiledCache.get(routes);
  if (c) return c;
  c = [];
  for (const [id, pat] of Object.entries(routes)) {
    for (const p of Array.isArray(pat) ? pat : [pat]) {
      const segs = splitPath(p.split("?")[0]);
      c.push({ id, segs, literals: segs.filter((s) => !isParam(s)).length });
    }
  }
  compiledCache.set(routes, c);
  return c;
}

/** The screen ids whose route pattern matches `path`: those of the most specific matching pattern (ties keep all). */
export function resolveRoute(path: string, routes: Routes): string[] {
  const segs = splitPath(path);
  let best = -1;
  let ids = new Set<string>();
  for (const r of compile(routes)) {
    if (!matches(r.segs, segs)) continue;
    if (r.literals > best) {
      best = r.literals;
      ids = new Set([r.id]);
    } else if (r.literals === best) ids.add(r.id);
  }
  return [...ids].sort();
}
function matches(pattern: string[], segs: string[]): boolean {
  for (let i = 0; i < pattern.length; i++) {
    const p = pattern[i];
    if (isRest(p)) return segs.length > i;
    if (i >= segs.length) return false;
    if (!isParam(p) && p !== segs[i]) return false;
  }
  return pattern.length === segs.length;
}

/** One `href` -> a link token, or null if it is not a link to a screen (external, in-page, mail). */
function linkToken(href: string, isBoard: boolean, opts: Options): string | null {
  const h = href.trim();
  if (!h || h.startsWith("#")) return null;
  if (h.startsWith("@")) {
    const t = h.slice(1);
    if (!t.includes("/") && SECTION_ENTRY[t]) return SECTION_ENTRY[t];
    return t.includes("/") ? t.split("/").pop()! : t;
  }
  if (/^[a-z][a-z0-9+.-]*:/i.test(h) || h.startsWith("//")) return null;
  let path = h.split(/[?#]/)[0];
  if (!path.startsWith("/")) path = "/" + path;
  const base = (opts.base ?? "/").replace(/\/+$/, "");
  if (base && (path === base || path.startsWith(base + "/"))) path = path.slice(base.length) || "/";
  if (isBoard || !opts.routes) return "?" + path;
  const ids = resolveRoute(path, opts.routes);
  return ids.length ? ids.join("|") : "?" + path;
}

// ---------------------------------------------------------------------------------------------------------------
// The one extraction function

/** A dialog or drawer that is showing: not hidden, and a <dialog> must be `open` (an empty <dialog> waiting for htmx is not an overlay page). */
const OVERLAY = (e: El): boolean => {
  if ("hidden" in e.attrs || e.attrs["aria-hidden"] === "true") return false;
  if (e.tag === "dialog") return "open" in e.attrs;
  return e.attrs.role === "dialog" || e.attrs.role === "alertdialog" || hasClass(e, "modal") || hasClass(e, "drawer-panel");
};

export function signature(html: string, opts: Options = {}): Signature {
  const fm = frontMatter(html);
  const isBoard = fm !== null;
  const root = parse(html);

  const overlayEl = isBoard ? null : (findAll(root, OVERLAY)[0] ?? null);
  const kind: Signature["kind"] = isBoard ? (fm!.kind === "dialog" || fm!.kind === "drawer" ? "overlay" : "page") : overlayEl ? "overlay" : "page";

  // The scope: a board is its own content; a rendered page is its <main>, or the overlay on an overlay page.
  let scope: El = root;
  if (!isBoard) {
    scope = overlayEl ?? findAll(root, (e) => e.tag === "main" || hasClass(e, "main") || e.attrs.role === "main")[0] ?? findAll(root, (e) => e.tag === "body")[0] ?? root;
  }

  // Every element of the scope except scaffolding and svg/script/style, in document order.
  const els: El[] = [];
  (function walk(n: El) {
    for (const c of n.children) {
      if (typeof c === "string") continue;
      if (scaffold(c) || c.tag === "svg" || c.tag === "script" || c.tag === "style") continue;
      els.push(c);
      walk(c);
    }
  })(scope);

  // (a) title
  let title: string | null = null;
  if (kind === "page") {
    if (isBoard) title = norm(String(fm!.title ?? "")) || null;
    else {
      const cur = findAll(root, (e) => (hasClass(e, "cur") || e.attrs["aria-current"] === "page") && closest(e, (a) => hasClass(a, "crumbs") || /breadcrumb/i.test(a.attrs["aria-label"] ?? "")) !== null);
      const last = cur[cur.length - 1];
      title = last ? norm(rawText(last)) || null : null;
    }
  }
  const h1 = els.filter((e) => e.tag === "h1").map((e): Heading => ({ text: text(e), data: hasClass(e, "mono") || findAll(e, (d) => hasClass(d, "mono")).length > 0 }));

  // (b)
  const headings = els.filter((e) => e.tag === "h2" || e.tag === "h3").map((e) => `${e.tag} ${maskedText(e)}`);

  // (c)
  const tables = els
    .filter((e) => e.tag === "table")
    .map((t): TableSig => {
      const head = findAll(t, (e) => e.tag === "thead")[0];
      const rows = findAll(t, (e) => e.tag === "tr");
      const headRow = head ? findAll(head, (e) => e.tag === "tr")[0] : rows.find((r) => findAll(r, (e) => e.tag === "th").length > 0 && findAll(r, (e) => e.tag === "td").length === 0);
      const cols = (headRow ? findAll(headRow, (e) => e.tag === "th") : []).map(text);
      const body = rows.filter((r) => findAll(r, (e) => e.tag === "td").length > 0);
      return { cols, rows: body.length > 0, wire: hasClass(t, "wire") };
    });

  // (d)
  const tabs = els
    .filter((e) => hasClass(e, "tab") || e.attrs.role === "tab")
    .map((e): TabSig => {
      const label = norm(rawText(e, (c) => (vary(c) || hasClass(c, "n") || hasClass(c, "count") ? "" : undefined))).replace(/\s+\(?\d[\d,]*\)?$/, "");
      return { label, on: hasClass(e, "on") || e.attrs["aria-current"] !== undefined || e.attrs["aria-selected"] === "true" };
    });

  // (e) and (f)
  const buttons: ButtonSig[] = [];
  const linkSet = new Set<string>();
  for (const e of els) {
    const href = e.tag === "a" ? e.attrs.href : undefined;
    const target = href !== undefined ? linkToken(href, isBoard, opts) : null;
    if (target) linkSet.add(target);
    const isInput = e.tag === "input" && (e.attrs.type === "submit" || e.attrs.type === "button");
    if (e.tag === "button" || hasClass(e, "btn") || isInput) {
      const own = isInput ? norm(e.attrs.value ?? "") : text(e);
      const masked = isInput ? own : maskedText(e);
      const label = own || norm(e.attrs["aria-label"] ?? e.attrs.title ?? "");
      const variants = BUTTON_VARIANTS.filter((v) => hasClass(e, v)) as string[];
      let action = "none";
      if (e.attrs["data-op"]) action = `op:${e.attrs["data-op"]}`;
      else if (e.attrs["data-ui"]) action = `ui:${e.attrs["data-ui"]}`;
      else if (target) action = `to:${target}`;
      else if (e.tag === "button" && (e.attrs.type ?? "submit") === "submit" && closest(e, (f) => f.tag === "form")) action = "submit";
      buttons.push({ label, loose: looseOf(masked || label), variants, action });
    }
  }

  // (g)
  const cardOf = (e: El): El | null => {
    const c = closest(e, (a) => hasClass(a, "card"));
    return c && (c === scope || descendantOf(c, scope)) ? c : null;
  };
  const cardEls = els.filter((e) => hasClass(e, "card"));
  const cardSigs = new Map<El | null, CardSig>();
  const seen = new Map<string, number>();
  let headless = 0;
  for (const c of cardEls) {
    const h = findAll(c, (e) => (e.tag === "h2" || e.tag === "h3") && cardOf(e) === c)[0];
    let key = h ? maskedText(h) : `card#${++headless}`;
    const n = (seen.get(key) ?? 0) + 1;
    seen.set(key, n);
    if (n > 1) key += ` #${n}`;
    cardSigs.set(c, { key, dt: [], fields: [], notes: [] });
  }
  cardSigs.set(null, { key: "page", dt: [], fields: [], notes: [] });
  for (const e of els) {
    const card = cardSigs.get(cardOf(e))!;
    if (e.tag === "dt") card.dt.push(text(e));
    if (hasClass(e, "field")) {
      const label = e.children.find((c): c is El => typeof c !== "string" && (c.tag === "label" || hasClass(c, "lab")));
      if (label) card.fields.push(norm(rawText(label, (c) => (vary(c) || hasClass(c, "opt") ? "" : undefined))));
    }
    if (hasClass(e, "callout") || hasClass(e, "banner")) {
      const isCallout = hasClass(e, "callout");
      const variant = (["warn", "bad", "refused"] as const).find((v) => hasClass(e, v)) ?? "";
      const b = findAll(e, (d) => d.tag === "b")[0];
      card.notes.push({ kind: isCallout ? "callout" : "banner", variant, lead: b ? text(b) : "" });
    }
  }
  const cards = [...cardSigs.entries()].filter(([k]) => k !== null).map(([, v]) => v);
  cards.push(cardSigs.get(null)!);

  // (h) (i) (j)
  const extras = new Set<string>();
  const ops: Record<string, number> = {};
  const states: Record<string, number> = {};
  for (const e of els) {
    if (e.attrs["data-extra"]) extras.add(e.attrs["data-extra"]);
    if (e.attrs["data-op"]) ops[e.attrs["data-op"]] = (ops[e.attrs["data-op"]] ?? 0) + 1;
    const cls = classes(e);
    for (const s of STATE_CLASSES) {
      if (!cls.includes(s)) continue;
      const v = STATE_VARIANTS.find((x) => cls.includes(x));
      if (!v) continue;
      // Rows are data: a state word inside a table body counts once per kind ("some row is st warn"), elsewhere it counts.
      if (closest(e, (a) => a.tag === "td")) states[`${s} ${v} in rows`] = 1;
      else states[`${s} ${v}`] = (states[`${s} ${v}`] ?? 0) + 1;
    }
  }

  // (k) landmarks. A board gets the shell the canvas kit draws around it; a rendered page is read.
  const lm = new Set<string>();
  if (isBoard) ["banner", "breadcrumb", "main", "rail"].forEach((l) => lm.add(l));
  else {
    for (const e of descendants(root)) {
      if (e.tag === "header" || hasClass(e, "topbar") || e.attrs.role === "banner") lm.add("banner");
      if (e.tag === "main" || hasClass(e, "main") || e.attrs.role === "main") lm.add("main");
      if (hasClass(e, "crumbs") || /breadcrumb/i.test(e.attrs["aria-label"] ?? "")) lm.add("breadcrumb");
      if (hasClass(e, "rail") || (e.tag === "nav" && /spaces/i.test(e.attrs["aria-label"] ?? ""))) lm.add("rail");
    }
  }
  if (kind === "overlay") lm.add("dialog");

  return {
    version: SIGNATURE_VERSION,
    kind,
    title,
    h1,
    headings,
    tables,
    tabs,
    buttons,
    links: [...linkSet].sort(),
    cards,
    extras: [...extras].sort(),
    ops: sortKeys(ops),
    states: sortKeys(states),
    landmarks: [...lm].sort(),
  };
}

function descendantOf(el: El, ancestor: El): boolean {
  for (let p = el.parent; p; p = p.parent) if (p === ancestor) return true;
  return false;
}

export function sortKeys<T>(o: Record<string, T>): Record<string, T> {
  return Object.fromEntries(Object.entries(o).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
}

/** Stable JSON: object keys sorted at every level, two-space indent, no trailing whitespace. */
export function stableStringify(v: unknown, indent = 2): string {
  const order = (x: unknown): unknown =>
    Array.isArray(x) ? x.map(order) : x && typeof x === "object" ? Object.fromEntries(Object.entries(x as Record<string, unknown>).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)).map(([k, val]) => [k, order(val)])) : x;
  return JSON.stringify(order(v), null, indent);
}

// ---------------------------------------------------------------------------------------------------------------
// Facts: the signature flattened, for comparing and for reports

export type Mode = "strict" | "live";
export interface Fact {
  /** The signature field: title, h1, heading, table, table.col, table.rows, tab, button, link, card, dt, field, note, extra, op, state, landmark, kind. */
  field: string;
  /** Where the fact lives (`table#0`, `card:This instance`, `tabs`, ...), `` for the page. */
  scope: string;
  value: string;
}
/** A list whose order is part of the signature (checked only when the facts are otherwise equal). */
export interface Sequence {
  field: string;
  scope: string;
  values: string[];
}

/** `tables` replaces sig.tables (see alignTables): a null slot is a table the page lacks. */
export function factsOf(sig: Signature, mode: Mode, tables: Array<TableSig | null> = sig.tables): { facts: Fact[]; sequences: Sequence[] } {
  const facts: Fact[] = [];
  const sequences: Sequence[] = [];
  const add = (field: string, scope: string, value: string) => facts.push({ field, scope, value });
  const live = mode === "live";
  const dataPage = sig.h1.some((h) => h.data);

  add("kind", "", sig.kind);
  // In the live gate a page named after a resource (a mono h1) is not compared by name.
  if (sig.title !== null && !(live && dataPage)) add("title", "", sig.title);
  else if (sig.title === null) add("title", "", "(none)");
  for (const h of sig.h1) add("h1", "", live && h.data ? "{name}" : h.text);
  for (const h of sig.headings) add("heading", "", h);
  sequences.push({ field: "heading", scope: "", values: sig.headings });

  tables.forEach((t, i) => {
    if (!t) return;
    const scope = `table#${i}`;
    add("table", scope, t.wire ? "wire" : "t");
    for (const c of t.cols) add("table.col", scope, c);
    sequences.push({ field: "table.col", scope, values: t.cols });
    if (!live) add("table.rows", scope, t.rows ? "has rows" : "no rows");
  });

  for (const t of sig.tabs) add("tab", "tabs", t.label);
  sequences.push({ field: "tab", scope: "tabs", values: sig.tabs.map((t) => t.label) });
  for (const t of sig.tabs) if (t.on) add("tab.on", "tabs", t.label);

  const buttons = sig.buttons.map((b) => `${live ? b.loose : b.label} [${b.variants.join(" ") || "-"}] -> ${b.action}`);
  for (const b of live ? [...new Set(buttons)] : buttons) add("button", "", b);

  for (const l of sig.links) add("link", "", l);

  for (const c of sig.cards) {
    const scope = `card:${c.key}`;
    if (c.key !== "page") add("card", "", c.key);
    for (const d of c.dt) add("dt", scope, d);
    for (const f of c.fields) add("field", scope, f);
    const notes = c.notes.map((n) => `${n.kind}${n.variant ? "." + n.variant : ""}${live ? "" : ": " + n.lead}`);
    for (const n of live ? [...new Set(notes)] : notes) add("note", scope, n);
  }

  for (const e of sig.extras) add("extra", "", e);
  for (const [k, n] of Object.entries(sig.ops)) {
    if (live) add("op", "", k);
    else for (let i = 0; i < n; i++) add("op", "", k);
  }
  for (const [k, n] of Object.entries(sig.states)) {
    if (live) add("state", "", k);
    else for (let i = 0; i < n; i++) add("state", "", k);
  }
  for (const l of sig.landmarks) add("landmark", "", l);
  return { facts, sequences };
}

/**
 * Make a rendered signature comparable with its board: a link that routes.json resolves to several screens
 * (`a|b`) becomes the one the board links to.
 */
export function align(board: Signature, rendered: Signature): Signature {
  const boardIds = new Set<string>(board.links);
  for (const b of board.buttons) if (b.action.startsWith("to:")) boardIds.add(b.action.slice(3));
  const pick = (tok: string): string => {
    if (!tok.includes("|")) return tok;
    return tok.split("|").find((t) => boardIds.has(t)) ?? tok;
  };
  const out: Signature = {
    ...rendered,
    links: [...new Set(rendered.links.map(pick))].sort(),
    buttons: rendered.buttons.map((b) => (b.action.startsWith("to:") ? { ...b, action: "to:" + pick(b.action.slice(3)) } : b)),
  };
  return out;
}

/**
 * The rendered tables arranged by the board's table index, matched by shared column labels (so a deleted table does
 * not shift every later one). A null slot is a table the page lacks; rendered tables nothing matched follow as extras.
 */
export function alignTables(board: Signature, rendered: Signature): Array<TableSig | null> {
  const free = new Set(rendered.tables.map((_, i) => i));
  const out: Array<TableSig | null> = board.tables.map(() => null);
  board.tables.forEach((bt, i) => {
    let bestJ = -1;
    let bestScore = 0;
    for (const j of free) {
      const rt = rendered.tables[j];
      const shared = rt.cols.filter((c) => bt.cols.includes(c)).length;
      if (shared === 0 && !(rt.cols.length === 0 && bt.cols.length === 0)) continue;
      const score = shared * 2 + (rt.cols.length === bt.cols.length ? 1 : 0) + (rt.wire === bt.wire ? 0.5 : 0) + (j === i ? 0.1 : 0);
      if (score > bestScore) {
        bestScore = score;
        bestJ = j;
      }
    }
    if (bestJ >= 0) {
      out[i] = rendered.tables[bestJ];
      free.delete(bestJ);
    }
  });
  for (const j of [...free].sort((a, b) => a - b)) out.push(rendered.tables[j]);
  return out;
}

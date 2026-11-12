// Accessibility lint for the console's rendered pages. No browser: it reads HTML files and the kit CSS.
//
//   bun tools/a11y-lint.ts [files...] [--css a.css,b.css] [--known file] [--warn] [--json]
//
// Files default to target/board-diff/*.html. CSS defaults to crates/kafka-console/design/kit/*.css (plus any <style> in a page).
// Exit 1 on any finding (--warn lists them and exits 0). --known is a JSON array of { board, check, reason }: a known
// finding of that check on that board is listed as known instead of failing, and an entry that matches nothing is an error.
//
// Checks (the id is what a known entry names):
//   lang          <html lang> present
//   h1            exactly one h1 (none on a dialog or drawer page: the dialog's own heading is the page's)
//   heading-order no skipped heading level (h2 straight to h4)
//   name          every control (button, a[href], input, select, textarea) has an accessible name
//   th-scope      every th has scope, or sits in a thead (a column header)
//   tabindex      no positive tabindex
//   dialog-label  a dialog has aria-labelledby that points at an element with text
//   live-region   aria-live / role=status|log only on a page that has data-extra="live-sentence"
//   status-word   .st and .rw have text (never colour alone)
//   motion        animations or transitions in the CSS need a prefers-reduced-motion block
//   contrast      WCAG AA (4.5:1) for the text/background pairs in the kit CSS, computed from its custom properties
// Not checked here (no browser): reading order with a screen reader, focus order beyond native controls.

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { type El, closest, descendants, findAll, hasClass, parse, rawText } from "./lib/html.ts";
import { kitDir, parseArgs, root } from "./lib/boards.ts";

export interface Finding {
  file: string;
  check: string;
  message: string;
}

export const CHECKS = ["lang", "h1", "heading-order", "name", "th-scope", "tabindex", "dialog-label", "live-region", "status-word", "motion", "contrast"];

const norm = (s: string): string => s.replace(/\[\[g:[^\]]*\]\]/g, "").replace(/\s+/g, " ").trim();
const hidden = (e: El): boolean => closest(e, (a) => "hidden" in a.attrs || a.attrs["aria-hidden"] === "true") !== null;
// A <dialog> with nothing in it is a target waiting for htmx; it is checked when it has content.
const isDialog = (e: El): boolean => (e.tag === "dialog" && e.children.length > 0) || e.attrs.role === "dialog" || e.attrs.role === "alertdialog";

function textOf(e: El): string {
  // img alt counts as text
  return norm(rawText(e, (c) => (c.tag === "img" ? (c.attrs.alt ?? "") : c.attrs["aria-hidden"] === "true" ? "" : undefined)));
}

const label = (e: El): string => {
  const h = e.tag === "input" ? e.attrs.type : "";
  return `<${e.tag}${h ? ` type=${h}` : ""}${e.attrs.class ? ` class="${e.attrs.class}"` : ""}${e.attrs.href ? ` href="${e.attrs.href}"` : ""}>${textOf(e).slice(0, 30)}`;
};

export function lintPage(html: string, file: string): { findings: Finding[]; css: string } {
  const root_ = parse(html);
  const all = [...descendants(root_)];
  const out: Finding[] = [];
  const add = (check: string, message: string) => out.push({ file, check, message });
  const byId = new Map<string, El>();
  for (const e of all) if (e.attrs.id) byId.set(e.attrs.id, e);

  // lang
  const htmlEl = all.find((e) => e.tag === "html");
  if (!htmlEl || !htmlEl.attrs.lang?.trim()) add("lang", "the <html> element has no lang attribute");

  // h1 and heading order
  const dialogs = all.filter(isDialog);
  const heads = all.filter((e) => /^h[1-6]$/.test(e.tag) && !hidden(e));
  const h1s = heads.filter((e) => e.tag === "h1" && !closest(e, isDialog));
  if (h1s.length > 1) add("h1", `${h1s.length} h1 elements (${h1s.map(textOf).map((t) => `"${t}"`).join(", ")}); a page has one`);
  if (h1s.length === 0 && dialogs.length === 0) add("h1", "no h1");
  let prev = h1s.length === 0 && dialogs.length > 0 ? 1 : 0;
  for (const h of heads) {
    const level = +h.tag[1];
    if (level > prev + 1) add("heading-order", `${h.tag} "${textOf(h).slice(0, 40)}" follows ${prev ? "h" + prev : "nothing"}: a level is skipped`);
    prev = level;
  }

  // controls have names
  const labelsFor = new Map<string, El[]>();
  for (const e of all) if (e.tag === "label" && e.attrs.for) (labelsFor.get(e.attrs.for) ?? labelsFor.set(e.attrs.for, []).get(e.attrs.for)!).push(e);
  const named = (e: El): boolean => {
    const lb = e.attrs["aria-labelledby"];
    if (lb && lb.split(/\s+/).some((id) => byId.has(id) && textOf(byId.get(id)!))) return true;
    if (norm(e.attrs["aria-label"] ?? "")) return true;
    if (e.tag === "input" || e.tag === "select" || e.tag === "textarea") {
      if (e.tag === "input" && ["submit", "button", "reset"].includes(e.attrs.type ?? "")) if (norm(e.attrs.value ?? "")) return true;
      if (e.tag === "input" && e.attrs.type === "image" && norm(e.attrs.alt ?? "")) return true;
      if (e.attrs.id && (labelsFor.get(e.attrs.id) ?? []).some((l) => textOf(l))) return true;
      const wrap = closest(e.parent ?? e, (a) => a.tag === "label");
      if (wrap && textOf(wrap)) return true;
      return Boolean(norm(e.attrs.title ?? ""));
    }
    return Boolean(textOf(e)) || Boolean(norm(e.attrs.title ?? ""));
  };
  for (const e of all) {
    const control = e.tag === "button" || (e.tag === "a" && e.attrs.href !== undefined) || e.tag === "select" || e.tag === "textarea" || (e.tag === "input" && e.attrs.type !== "hidden");
    if (!control || hidden(e)) continue;
    if (!named(e)) add("name", `${label(e)} has no accessible name`);
  }

  // th scope
  for (const th of all.filter((e) => e.tag === "th")) {
    if (th.attrs.scope || closest(th, (a) => a.tag === "thead")) continue;
    add("th-scope", `<th>${textOf(th).slice(0, 30)}</th> is in the body and has no scope`);
  }

  // tabindex
  for (const e of all) {
    const t = e.attrs.tabindex;
    if (t !== undefined && Number(t) > 0) add("tabindex", `${label(e)} has tabindex=${t}`);
  }

  // dialogs
  for (const d of dialogs) {
    const ids = (d.attrs["aria-labelledby"] ?? "").split(/\s+/).filter(Boolean);
    if (!ids.length) add("dialog-label", `${label(d)} has no aria-labelledby`);
    else if (!ids.some((id) => byId.has(id) && textOf(byId.get(id)!))) add("dialog-label", `${label(d)}: aria-labelledby="${ids.join(" ")}" points at nothing with text`);
  }

  // live regions
  const hasLiveSentence = all.some((e) => e.attrs["data-extra"] === "live-sentence");
  for (const e of all) {
    const live = (e.attrs["aria-live"] && e.attrs["aria-live"] !== "off") || e.attrs.role === "status" || e.attrs.role === "log";
    if (live && !hasLiveSentence) add("live-region", `${label(e)} is a live region, but the page has no data-extra="live-sentence"`);
  }

  // status words
  for (const e of all) {
    if ((hasClass(e, "st") || hasClass(e, "rw")) && !hidden(e) && !textOf(e)) add("status-word", `${label(e)}: a status with no word (colour alone)`);
  }

  // inline CSS for the CSS checks
  const css = all.filter((e) => e.tag === "style").map((e) => rawText(e)).join("\n");
  return { findings: out, css };
}

// ---------------------------------------------------------------------------------------------------------------
// CSS: motion and contrast

export function lintMotion(css: string, source: string): Finding[] {
  const animates = /@keyframes|[\s;{]animation(-name)?\s*:|[\s;{]transition(-property)?\s*:/.test(css);
  const honours = /@media[^{]*prefers-reduced-motion/.test(css);
  return animates && !honours ? [{ file: source, check: "motion", message: "the CSS animates or transitions but has no @media (prefers-reduced-motion) block" }] : [];
}

type RGB = [number, number, number];
function parseColor(v: string, tokens: Map<string, string>, depth = 0): RGB | null {
  v = v.trim().replace(/\s*!important$/, "");
  const vr = v.match(/^var\(\s*(--[\w-]+)\s*(?:,\s*([^)]+))?\)$/);
  if (vr) {
    if (depth > 8) return null;
    const t = tokens.get(vr[1]) ?? vr[2];
    return t ? parseColor(t, tokens, depth + 1) : null;
  }
  let m = v.match(/^#([0-9a-f]{3})$/i);
  if (m) return [...m[1]].map((c) => parseInt(c + c, 16)) as RGB;
  m = v.match(/^#([0-9a-f]{6})$/i);
  if (m) return [0, 2, 4].map((i) => parseInt(m![1].slice(i, i + 2), 16)) as RGB;
  m = v.match(/^rgb\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)\s*\)$/i);
  if (m) return [+m[1], +m[2], +m[3]];
  if (v === "white") return [255, 255, 255];
  if (v === "black") return [0, 0, 0];
  return null;
}
const lum = ([r, g, b]: RGB): number => {
  const f = (c: number) => ((c /= 255) <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
};
export const ratio = (a: RGB, b: RGB): number => {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};
const hex = ([r, g, b]: RGB): string => "#" + [r, g, b].map((c) => c.toString(16).padStart(2, "0")).join("");

interface Rule {
  selector: string;
  decls: Map<string, string>;
}
function parseRules(css: string): Rule[] {
  const src = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const rules: Rule[] = [];
  let i = 0;
  let selStart = 0;
  const stack: number[] = [];
  for (; i < src.length; i++) {
    const c = src[i];
    if (c === "{") {
      stack.push(i);
      // A block that contains no further "{" before its "}" is a rule; at-rules with nested rules are descended into.
      const close = src.indexOf("}", i);
      const nested = src.indexOf("{", i + 1);
      if (close !== -1 && (nested === -1 || nested > close)) {
        const selector = src.slice(selStart, i).trim();
        const body = src.slice(i + 1, close);
        const decls = new Map<string, string>();
        for (const d of body.split(";")) {
          const k = d.indexOf(":");
          if (k > 0) decls.set(d.slice(0, k).trim().toLowerCase(), d.slice(k + 1).trim());
        }
        if (!selector.startsWith("@")) rules.push({ selector, decls });
        i = close;
        stack.pop();
        selStart = i + 1;
      } else selStart = i + 1;
    } else if (c === "}") {
      stack.pop();
      selStart = i + 1;
    }
  }
  return rules;
}

export interface ContrastPair {
  fg: string;
  bg: string;
  ratio: number;
  /** 4.5 for text, 3 for graphical objects (the domain hues colour glyphs only). */
  min: number;
  selectors: string[];
}

// Colours that are not text. The domain hues (.d-msg, ...) tint glyphs and are held to the 3:1 of WCAG 1.4.11; separators
// (.arr, .sl) are decoration. Everything else is text and needs 4.5:1.
const GRAPHICAL = /^\.d-[a-z]+$/;
const DECORATIVE = /(^|[\s.])(arr|sl)$/;

/** Every text/background pair the CSS implies, with its contrast ratio. */
export function contrastPairs(css: string): ContrastPair[] {
  const rules = parseRules(css);
  const tokens = new Map<string, string>();
  for (const r of rules) for (const [k, v] of r.decls) if (k.startsWith("--")) tokens.set(k, v);
  const tok = (n: string) => parseColor(`var(${n})`, tokens);
  const surfaces: RGB[] = ["--bg", "--panel", "--well"].map(tok).filter((x): x is RGB => x !== null);
  const side = tok("--side");
  const text = tok("--text");
  const pairs = new Map<string, ContrastPair>();
  const add = (fg: RGB, bg: RGB, sel: string) => {
    const min = GRAPHICAL.test(sel) ? 3 : 4.5;
    const k = hex(fg) + hex(bg) + min;
    const p = pairs.get(k) ?? { fg: hex(fg), bg: hex(bg), ratio: ratio(fg, bg), min, selectors: [] };
    if (!p.selectors.includes(sel) && p.selectors.length < 6) p.selectors.push(sel);
    pairs.set(k, p);
  };
  for (const r of rules) {
    const sel = r.selector;
    if (/::?(before|after)|\bsvg\b|\.gl\b|:hover|:focus/.test(sel) || DECORATIVE.test(sel)) continue; // decoration, glyphs, states with their own colours
    if (r.decls.has("opacity") && Number(r.decls.get("opacity")) < 1) continue; // disabled controls are exempt
    const fgV = r.decls.get("color");
    const bgV = r.decls.get("background-color") ?? r.decls.get("background");
    const fg = fgV ? parseColor(fgV, tokens) : null;
    const bg = bgV ? parseColor(bgV, tokens) : null;
    if (fg && bg) add(fg, bg, sel);
    else if (fg) {
      for (const s of surfaces) add(fg, s, sel);
      if (side && /rail|\.ni\b|\.pin\b|\.hints|\.eyebrow/.test(sel)) add(fg, side, sel);
    } else if (bg && text && bgV && !/gradient|url/.test(bgV) && /\b(td|th|\.btn|\.callout|\.banner|\.pill|\.fchip|\.chip|\.tab|\.ev|\.sg|\.opt|\.cause|\.fold|\.tag|\.rule)\b/.test(sel)) add(text, bg, sel);
  }
  return [...pairs.values()];
}

export function lintContrast(css: string, source: string): Finding[] {
  return contrastPairs(css)
    .filter((p) => p.ratio < p.min)
    .sort((a, b) => a.ratio - b.ratio)
    .map((p) => ({ file: source, check: "contrast", message: `${p.fg} on ${p.bg} is ${p.ratio.toFixed(2)}:1, below ${p.min}:1 (${p.selectors.join(", ")})` }));
}

// ---------------------------------------------------------------------------------------------------------------

export interface Known {
  board: string;
  check: string;
  reason: string;
}
export function applyKnown(findings: Finding[], known: Known[]): { failing: Finding[]; knownFindings: Array<Finding & { reason: string }>; errors: string[] } {
  const errors: string[] = [];
  const used = new Set<Known>();
  const failing: Finding[] = [];
  const knownFindings: Array<Finding & { reason: string }> = [];
  for (const k of known) {
    if (!k.reason?.trim()) errors.push(`known entry ${k.board}/${k.check}: a reason is required`);
    if (!CHECKS.includes(k.check)) errors.push(`known entry ${k.board}/${k.check}: unknown check`);
  }
  for (const f of findings) {
    const board = basename(f.file).replace(/\.html$/, "");
    const k = known.find((x) => x.board === board && x.check === f.check);
    if (k) {
      used.add(k);
      knownFindings.push({ ...f, reason: k.reason });
    } else failing.push(f);
  }
  for (const k of known) if (!used.has(k)) errors.push(`stale known entry: ${k.board}/${k.check} matches no finding`);
  return { failing, knownFindings, errors };
}

export interface LintRun {
  findings: Finding[];
  known: Array<Finding & { reason: string }>;
  errors: string[];
  files: number;
}
export function lintFiles(files: string[], cssFiles: string[], known: Known[] = []): LintRun {
  const findings: Finding[] = [];
  const inline: string[] = [];
  for (const f of files) {
    const { findings: fs, css } = lintPage(readFileSync(f, "utf8"), f);
    findings.push(...fs);
    if (css) inline.push(css);
  }
  const css = [...cssFiles.map((f) => readFileSync(f, "utf8")), ...inline].join("\n");
  const label = cssFiles.length ? cssFiles.map((f) => basename(f)).join("+") : "(inline css)";
  findings.push(...lintMotion(css, label), ...lintContrast(css, label));
  const { failing, knownFindings, errors } = applyKnown(findings, known);
  return { findings: failing, known: knownFindings, errors, files: files.length };
}

if (import.meta.main) {
  const { flags, values, rest } = parseArgs(process.argv.slice(2), ["css", "known"]);
  let files = rest.map((f) => resolve(f));
  if (!files.length) {
    const dir = join(root, "target/board-diff");
    files = existsSync(dir) ? readdirSync(dir).filter((f) => f.endsWith(".html")).sort().map((f) => join(dir, f)) : [];
    if (!files.length) {
      console.error(`no files given and none in ${dir}: render the pages first`);
      process.exit(2);
    }
  }
  const cssFiles = values.css ? values.css.split(",").map((f) => resolve(f)) : existsSync(kitDir) ? readdirSync(kitDir).filter((f) => f.endsWith(".css")).sort().map((f) => join(kitDir, f)) : [];
  const known: Known[] = values.known ? JSON.parse(readFileSync(resolve(values.known), "utf8")) : [];
  const run = lintFiles(files, cssFiles, known);
  if (flags.has("json")) console.log(JSON.stringify(run, null, 2));
  else {
    for (const f of run.findings) console.log(`${flags.has("warn") ? "warn" : "FAIL"} ${basename(f.file)} [${f.check}] ${f.message}`);
    for (const f of run.known) console.log(`known ${basename(f.file)} [${f.check}] ${f.message}  (${f.reason})`);
    for (const e of run.errors) console.log("error: " + e);
    console.log(`${run.files} page(s), ${cssFiles.length} css file(s): ${run.findings.length} finding(s), ${run.known.length} known, ${run.errors.length} error(s)`);
  }
  process.exit(flags.has("warn") ? 0 : run.findings.length || run.errors.length ? 1 : 0);
}

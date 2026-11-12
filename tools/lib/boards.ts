// Where the design contract lives, and how to read it. Shared by the board tools.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { type FrontMatter, frontMatter, type Signature, signature } from "../board-signature.ts";

export const root = resolve(import.meta.dir, "../..");
export const designDir = join(root, "crates/kafka-console/design");
export const boardsDir = join(designDir, "boards");
export const kitDir = join(designDir, "kit");

export interface BoardFile {
  id: string;
  section: string;
  path: string;
  html: string;
  fm: FrontMatter;
}

/** Every `<dir>/<section>/<id>.html` with a screen front matter. Directories whose name starts with `_` or is `docs` are not boards. */
export function loadBoards(dir: string = boardsDir): BoardFile[] {
  const out: BoardFile[] = [];
  if (!existsSync(dir)) return out;
  for (const section of readdirSync(dir).sort()) {
    const sdir = join(dir, section);
    if (!statSync(sdir).isDirectory() || section.startsWith("_") || section === "docs") continue;
    for (const f of readdirSync(sdir).sort()) {
      if (!f.endsWith(".html")) continue;
      const path = join(sdir, f);
      const html = readFileSync(path, "utf8");
      const fm = frontMatter(html);
      if (!fm) continue;
      out.push({ id: f.replace(/\.html$/, ""), section, path, html, fm });
    }
  }
  return out;
}

/** `--name value` and `--name=value` arguments; positional arguments; boolean flags. */
export function parseArgs(argv: string[], valued: string[]): { flags: Set<string>; values: Record<string, string>; rest: string[] } {
  const flags = new Set<string>();
  const values: Record<string, string> = {};
  const rest: string[] = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith("--")) {
      rest.push(a);
      continue;
    }
    const [name, inline] = a.slice(2).split(/=(.*)/s);
    if (valued.includes(name)) {
      const v = inline ?? argv[++i];
      if (v === undefined) throw new Error(`--${name} needs a value`);
      values[name] = v;
    } else flags.add(name);
  }
  return { flags, values, rest };
}

/**
 * The title a board's page has: its front-matter title, except for a STATE of a page ("Something needs you" is Home in
 * a different moment), whose page title is its parent's. The real page has one route and one breadcrumb for all states.
 */
export function pageTitle(fm: Record<string, unknown>, byId: Map<string, Record<string, unknown>>): string {
  const own = Array.isArray(fm.crumbs) && fm.crumbs.length > 0; // a state with its own breadcrumb is a screen of its own
  if (fm.kind === "state" && typeof fm.from === "string" && !own) {
    const parent = byId.get(fm.from);
    if (parent) return pageTitle(parent, byId);
  }
  return String(fm.title ?? "");
}

/** The signature of a board: `signature(html)` with a state's title replaced by its page's (see pageTitle). */
export function boardSignature(b: BoardFile, byId: Map<string, Record<string, unknown>>): Signature {
  const s = signature(b.html);
  if (s.title !== null) s.title = pageTitle(b.fm as Record<string, unknown>, byId);
  return s;
}

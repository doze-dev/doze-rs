// Where the design contract lives, and how to read it. Shared by the board tools.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { type FrontMatter, frontMatter } from "../board-signature.ts";

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

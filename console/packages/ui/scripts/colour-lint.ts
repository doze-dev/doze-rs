// The colour lint of @doze/ui: shared code names roles, never a colour. It fails on any literal colour in src/: a hex
// (`#fff`, `#16150f`), a colour function (`rgb(`, `hsl(`, `oklch(`, ...), a Tailwind palette class (`bg-yellow-400`,
// `text-white`, `border-black/10`), or a named colour as a Tailwind arbitrary value (`bg-[red]`). Values live in a theme
// file (themes/*.css), where they belong to a product.
//
//   bun scripts/colour-lint.ts        exits 1 and lists the findings (run by `bun run lint` in console/)
//   tests/colours.test.ts             the same check, and the check's own tests
//
// Allowed: `transparent`, `currentColor`, `inherit`, and `color-mix(...)` of roles.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const PALETTE = "slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose";
const PROPERTY = "bg|text|border(?:-[trblxyse])?|ring|ring-offset|outline|fill|stroke|from|via|to|shadow|inset-shadow|inset-ring|drop-shadow|text-shadow|decoration|divide|accent|caret|placeholder";
const NAMED = "black|white|red|green|blue|yellow|orange|purple|pink|gray|grey|silver|navy|teal|aqua|lime|maroon|olive|fuchsia|gold|crimson|indigo|violet|tomato|coral|salmon|khaki|beige|ivory";

const RULES: { what: string; re: RegExp }[] = [
  { what: "a hex colour", re: /#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3,4})(?![0-9a-zA-Z_-])/g },
  { what: "a colour function", re: /(?<![\w-])(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/g },
  { what: "a Tailwind palette class", re: new RegExp(`(?<![\\w-])(?:${PROPERTY})-(?:black|white|(?:${PALETTE})-(?:50|[1-9]00|950))(?![\\w-])`, "g") },
  { what: "a named colour as an arbitrary value", re: new RegExp(`(?<![\\w-])(?:${PROPERTY})-\\[(?:color:)?(?:${NAMED})\\]`, "g") },
];

export type Finding = { line: number; text: string; what: string };

/** The literal colours in one file's text. */
export function literalColours(source: string): Finding[] {
  const out: Finding[] = [];
  source.split("\n").forEach((lineText, i) => {
    for (const { what, re } of RULES) for (const m of lineText.matchAll(re)) out.push({ line: i + 1, text: m[0], what });
  });
  return out;
}

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else if (/\.(?:tsx?|css)$/.test(name) && !/\.test\.tsx?$/.test(name)) out.push(path);
  }
  return out;
}

/** Every finding under `dir`, as `path:line: what (text)`. */
export function lintDir(dir: string): string[] {
  return walk(dir)
    .sort()
    .flatMap((path) => literalColours(readFileSync(path, "utf8")).map((f) => `${relative(dir, path)}:${f.line}: ${f.what} (${f.text})`));
}

if (import.meta.main) {
  const src = join(import.meta.dir, "../src");
  const found = lintDir(src);
  if (found.length > 0) {
    console.error(`colour-lint: ${found.length} literal colour${found.length === 1 ? "" : "s"} in packages/ui/src (shared code names roles only):`);
    for (const f of found) console.error(`  ${f}`);
    process.exit(1);
  }
  console.log("colour-lint: packages/ui/src names roles only");
}

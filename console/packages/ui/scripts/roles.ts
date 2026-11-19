// The roles @doze/ui reads, and what a theme file sets: the two halves of the theme-completeness test (tests/roles.test.ts).
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else if (/\.(?:tsx?|css)$/.test(name) && !/\.test\.tsx?$/.test(name)) out.push(path);
  }
  return out;
}

/** The ways the library reads a custom property: `var(--x)`, Tailwind's short form `h-(--x)`, and `getPropertyValue("--x")`. */
const READS = [/var\(\s*(--[\w-]+)/g, /-\((--[\w-]+)\)/g, /getPropertyValue\(\s*["'](--[\w-]+)["']\s*\)/g];
/** The ways it sets one itself: a CSS declaration (base.css's @theme names), a style key (`"--gap": spacing`), a Tailwind
 * arbitrary property (`[--dp-x:1.5rem]`). */
const SETS = [/(?:^|[\s{;])(--[\w-]+)\s*:/gm, /["'](--[\w-]+)["']\s*:/g, /\[(--[\w-]+):/g];
/** Set at run time by a library, not by a theme. */
const NOT_OURS = /^--(?:radix|tw)-/;

/** Every custom property the sources under `dir` read and do not set themselves: the roles a theme must give. Sorted. */
export function rolesRead(dir: string): string[] {
  const reads = new Set<string>();
  const sets = new Set<string>();
  for (const path of walk(dir)) {
    const text = readFileSync(path, "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
    for (const re of READS) for (const m of text.matchAll(re)) reads.add(m[1]!);
    for (const re of SETS) for (const m of text.matchAll(re)) sets.add(m[1]!);
  }
  return [...reads].filter((r) => !sets.has(r) && !NOT_OURS.test(r)).sort();
}

export type Theme = {
  /** `:root { … }` */
  light: Map<string, string>;
  /** `:root.dark { … }`: the person chose dark. */
  darkClass: Map<string, string>;
  /** `@media (prefers-color-scheme: dark) { :root:not(.light) { … } }`: the OS is dark and the person chose nothing. */
  darkSystem: Map<string, string>;
};

/** The custom properties a theme file sets, by where it sets them. */
export function parseTheme(css: string): Theme {
  const theme: Theme = { light: new Map(), darkClass: new Map(), darkSystem: new Map() };
  const text = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const stack: string[] = [];
  let buf = "";
  const declare = (decl: string) => {
    const m = /^\s*(--[\w-]+)\s*:\s*([\s\S]+?)\s*$/.exec(decl);
    if (!m) return;
    const where = stack.join(" | ");
    const into = where === ":root" ? theme.light : where === ":root.dark" ? theme.darkClass : where === "@media (prefers-color-scheme: dark) | :root:not(.light)" ? theme.darkSystem : null;
    into?.set(m[1]!, m[2]!);
  };
  for (const ch of text) {
    if (ch === "{") {
      stack.push(buf.trim().replace(/\s+/g, " "));
      buf = "";
    } else if (ch === "}") {
      declare(buf);
      stack.pop();
      buf = "";
    } else if (ch === ";") {
      declare(buf);
      buf = "";
    } else buf += ch;
  }
  return theme;
}

/** A value that is a colour: it must differ by mode, so dark must set it too. */
export const isColour = (value: string) => /^#|^(?:rgba?|hsla?|oklch|oklab|lab|lch|hwb|color|color-mix)\(/.test(value);

/** What is wrong with a theme for the roles the library reads; empty when it is complete. */
export function themeProblems(theme: Theme, roles: string[]): string[] {
  const out: string[] = [];
  for (const r of roles) if (!theme.light.has(r)) out.push(`${r}: not set in :root (light)`);
  for (const [name, value] of theme.light) {
    if (!isColour(value)) continue;
    if (!theme.darkClass.has(name)) out.push(`${name}: a colour in light, not set in :root.dark`);
    if (!theme.darkSystem.has(name)) out.push(`${name}: a colour in light, not set in the prefers-color-scheme: dark block`);
  }
  for (const [name, value] of theme.darkClass) {
    if (!theme.light.has(name)) out.push(`${name}: set in :root.dark but not in :root`);
    if (theme.darkSystem.get(name) !== value) out.push(`${name}: :root.dark says ${value}, the prefers-color-scheme: dark block says ${theme.darkSystem.get(name) ?? "nothing"}`);
  }
  for (const name of theme.darkSystem.keys()) if (!theme.darkClass.has(name)) out.push(`${name}: set in the prefers-color-scheme: dark block but not in :root.dark`);
  return out;
}

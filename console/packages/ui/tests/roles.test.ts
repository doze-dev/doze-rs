// Guard rail (b): every role the library reads is set by both themes, for light and for dark. See scripts/roles.ts.
import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { isColour, parseTheme, rolesRead, themeProblems } from "../scripts/roles";

const src = join(import.meta.dir, "../src");
const roles = rolesRead(src);
const theme = (name: string) => parseTheme(readFileSync(join(import.meta.dir, `../themes/${name}.css`), "utf8"));

test("the library reads roles, and the known ones are among them", () => {
  // Not the whole list (it grows with the library): one of each family, so a change that stops the scan from seeing a
  // family fails here and not silently.
  for (const r of ["--background", "--primary", "--destructive-foreground", "--scrim", "--good", "--refused", "--pick", "--sidebar-accent", "--radius", "--font-sans", "--font-mono", "--body-size", "--control-h", "--row-h", "--dur-in", "--dur-out", "--ease", "--dur-highlight"]) expect(roles).toContain(r);
  // What the library sets itself, and what a library sets at run time, are not roles.
  for (const r of ["--sidebar-width", "--dp-x", "--gap", "--color-primary", "--radius-lg", "--radix-popover-trigger-width", "--normal-bg"]) expect(roles).not.toContain(r);
  expect(roles.length).toBeGreaterThan(50);
});

for (const name of ["kafka", "aws"]) {
  test(`themes/${name}.css sets every role, for light and for dark`, () => {
    const t = theme(name);
    expect(themeProblems(t, roles)).toEqual([]);
    // The dark blocks are not empty shells: every colour role read is in them.
    const colourRoles = roles.filter((r) => isColour(t.light.get(r) ?? ""));
    expect(colourRoles.length).toBeGreaterThan(40);
    for (const r of colourRoles) expect(t.darkClass.has(r) && t.darkSystem.has(r), r).toBe(true);
  });
}

test("the check sees a theme that is not complete", () => {
  const t = parseTheme(`
    :root { --background: #fff; --radius: 4px; --extra: #000; }
    @media (prefers-color-scheme: dark) { :root:not(.light) { --background: #111; } }
    :root.dark { --background: #000; --only-dark: #222; }
    .other { --foreground: #333; }
  `);
  expect(themeProblems(t, ["--background", "--foreground", "--radius"])).toEqual([
    "--foreground: not set in :root (light)",
    "--extra: a colour in light, not set in :root.dark",
    "--extra: a colour in light, not set in the prefers-color-scheme: dark block",
    "--background: :root.dark says #000, the prefers-color-scheme: dark block says #111",
    "--only-dark: set in :root.dark but not in :root",
    "--only-dark: :root.dark says #222, the prefers-color-scheme: dark block says nothing",
  ]);
});

test("a theme file is read with its comments, font faces and other rules ignored", () => {
  const t = parseTheme(`/* --ghost: #fff; */ @font-face { font-family: "X"; src: url("./x.woff2"); } :root { --a: #fff; /* why */ --b: 1px; } html:not(.dark) [data-x] { --c: #000; }`);
  expect([...t.light]).toEqual([["--a", "#fff"], ["--b", "1px"]]);
});

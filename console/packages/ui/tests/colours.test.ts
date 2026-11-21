// Guard rail (a): shared code names roles only. See scripts/colour-lint.ts.
import { expect, test } from "bun:test";
import { join } from "node:path";
import { lintDir, literalColours } from "../scripts/colour-lint";

const found = (s: string) => literalColours(s).map((f) => f.text);

test("the lint sees every kind of literal colour", () => {
  expect(found('className="bg-[#16150f] text-[#fff]"')).toEqual(["#16150f", "#fff"]);
  expect(found("color: #FFD60A80;")).toEqual(["#FFD60A80"]);
  expect(found("background: rgb(22 21 15 / 28%); border-color: rgba(0,0,0,.5)")).toEqual(["rgb(", "rgba("]);
  expect(found("color: hsl(48 100% 52%); fill: oklch(0.8 0.2 90); stroke: color(display-p3 1 0 0)")).toEqual(["hsl(", "oklch(", "color("]);
  expect(found('"bg-yellow-400 hover:bg-yellow-500/90 dark:text-zinc-50 border-b-slate-950"')).toEqual(["bg-yellow-400", "bg-yellow-500", "text-zinc-50", "border-b-slate-950"]);
  expect(found('"fixed inset-0 bg-black/50" "bg-destructive text-white" "ring-offset-white"')).toEqual(["bg-black", "text-white", "ring-offset-white"]);
  expect(found('"from-sky-100 via-white to-rose-200 shadow-black/10 fill-white stroke-red-500"')).toEqual(["from-sky-100", "via-white", "to-rose-200", "shadow-black", "fill-white", "stroke-red-500"]);
  expect(found('"bg-[red] text-[color:white]"')).toEqual(["bg-[red]", "text-[color:white]"]);
});

test("the lint leaves roles and non-colours alone", () => {
  for (const ok of [
    '"bg-primary text-primary-foreground border-input ring-ring/50 bg-scrim text-good bg-tint-warn"',
    '"bg-accent/40 text-accent-foreground hover:bg-destructive/90 fill-current text-transparent bg-transparent"',
    "background-color: color-mix(in oklab, var(--primary) 14%, transparent);",
    "--color-bad: var(--bad); color: currentColor; border-color: inherit;",
    'document.querySelector("#main"); const sel = `#${id}`; // see issue #12',
    '"text-[13.5px] tracking-[0.04em] w-[9.25rem] text-xs text-left border-separate to-transparent"',
    '"data-[state=open]:bg-accent **:data-[slot=command-input-wrapper]:h-12 size-4 text-balance"',
  ])
    expect(found(ok)).toEqual([]);
});

test("a finding names its line", () => {
  expect(literalColours("a\nb\n  color: #abc;\n")).toEqual([{ line: 3, text: "#abc", what: "a hex colour" }]);
});

test("packages/ui/src has no literal colour", () => {
  expect(lintDir(join(import.meta.dir, "../src"))).toEqual([]);
});

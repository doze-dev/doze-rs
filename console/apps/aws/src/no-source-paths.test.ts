import { expect, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import ts from "typescript";

// A page says what doze-aws does in plain words; the source file that does it is cited in the deviations doc and in code
// comments, never in text the user reads. The scanner below reads each file as TypeScript, so comments never count and a
// string, a template's text or JSX text always does.

const ROOT = join(import.meta.dir);
const DIRS = ["pages", "kit"];
const SOURCE_PATH = /\.rs\b|crates\//;

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return files(path);
    return /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name) ? [path] : [];
  });
}

/** The text a user can read in one source file: every string, template text and JSX text, comments left out. */
export function visibleText(path: string, source: string): { line: number; text: string }[] {
  const kind = path.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  const file = ts.createSourceFile(path, source, ts.ScriptTarget.Latest, true, kind);
  const found: { line: number; text: string }[] = [];
  const add = (node: ts.Node, text: string) => {
    found.push({ line: file.getLineAndCharacterOfPosition(node.getStart(file)).line + 1, text });
  };
  const walk = (node: ts.Node) => {
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) add(node, node.text);
    else if (ts.isTemplateHead(node) || ts.isTemplateMiddle(node) || ts.isTemplateTail(node)) add(node, node.text);
    else if (ts.isJsxText(node)) add(node, node.text);
    ts.forEachChild(node, walk);
  };
  walk(file);
  return found;
}

test("no page or kit file shows a source file or crate path to the user", () => {
  const offenders: string[] = [];
  for (const dir of DIRS) {
    for (const path of files(join(ROOT, dir))) {
      for (const { line, text } of visibleText(path, readFileSync(path, "utf8"))) {
        if (SOURCE_PATH.test(text)) offenders.push(`${path.slice(ROOT.length + 1)}:${line}: ${text.slice(0, 80)}`);
      }
    }
  }
  expect(offenders).toEqual([]);
});

test("the scan sees strings, template text and JSX text, and skips comments", () => {
  const src = [
    "// crates/aws-x/src/a.rs in a comment",
    "/** b.rs in a doc comment */",
    'export const a = "crates/aws-y/src/c.rs";',
    "export const b = `d.rs ${1} e.rs`;",
    "export const c = <p>f.rs <b>g</b></p>;",
  ].join("\n");
  const text = visibleText("x.tsx", src).map((t) => t.text);
  expect(text.filter((t) => SOURCE_PATH.test(t)).length).toBe(4);
  expect(text.join("|")).not.toContain("comment");
});

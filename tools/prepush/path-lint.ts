// A lint over the lines a branch adds for the bug class Windows runs keep finding: paths built or compared as text with `/`.
// On Windows `Path::join("a/b")` keeps the `/`, `format!("{}/x", dir.display())` mixes separators, and a test that compares
// `.display().to_string()` with a `/` literal fails there only (names-hardening, 2026-11-27: `...\ca/root.crt` against
// `...\ca\root.crt`; a test that read a PEM roots file Windows does not have).
//
//   bun tools/prepush/path-lint.ts [--base <ref>]
//
// Exit 1 only for a clear assert comparison (an assert that holds both a path's display text and a `/` literal); everything else is
// a warning to read. Only added or changed lines are judged, so old code never fails a branch. A line holding `path-lint: ok` is
// skipped (say why next to it), and so are files whose name says they are one platform's (`unix`, `linux`, `macos`, `darwin`).

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { changedFiles, root } from "./plan";

interface Finding {
  file: string;
  line: number;
  rule: string;
  fail: boolean;
  text: string;
}

/** String literals in a piece of Rust (plain and raw), roughly: enough for a lint that only warns. */
function literals(code: string): string[] {
  return [...code.matchAll(/r#*"((?:[^"\\]|\\.)*)"#*|"((?:[^"\\]|\\.)*)"/g)].map((m) => m[1] ?? m[2] ?? "");
}

/** A literal that reads as a relative path with a `/` in it (not a URL, not a comment, not a lone "/"). */
function slashPath(lit: string): boolean {
  return lit.includes("/") && !lit.includes("://") && lit !== "/" && !lit.startsWith("//") && !/\s/.test(lit.trim());
}

const DISPLAY = /\.display\(\)|to_string_lossy\(\)|\.to_str\(\)/;

/** Added line numbers (in the new file) of one file against the base. */
function addedLines(base: string, file: string): Set<number> {
  const out = execFileSync("git", ["diff", "-U0", "--no-color", base, "--", file], { cwd: root, encoding: "utf8" });
  const lines = new Set<number>();
  for (const m of out.matchAll(/^@@ -\S+ \+(\d+)(?:,(\d+))? @@/gm)) {
    const start = Number(m[1]);
    const count = m[2] === undefined ? 1 : Number(m[2]);
    for (let i = 0; i < count; i++) lines.add(start + i);
  }
  return lines;
}

/** Each `assert!`/`assert_eq!`/`assert_ne!` call: its first and last line and its text (parentheses matched). */
function asserts(src: string): { from: number; to: number; text: string }[] {
  const found = [];
  for (const m of src.matchAll(/\b(?:debug_)?assert(?:_eq|_ne)?!\s*\(/g)) {
    let depth = 0;
    let end = m.index!;
    for (let i = m.index! + m[0].length - 1; i < src.length; i++) {
      if (src[i] === "(") depth++;
      else if (src[i] === ")" && --depth === 0) {
        end = i;
        break;
      }
    }
    const from = src.slice(0, m.index!).split("\n").length;
    const text = src.slice(m.index!, end + 1);
    found.push({ from, to: from + text.split("\n").length - 1, text });
  }
  return found;
}

export function lint(baseRef = "origin/main"): Finding[] {
  const { base, files } = changedFiles(baseRef);
  const findings: Finding[] = [];
  for (const file of files) {
    if (!file.endsWith(".rs") || file.startsWith("vendor/") || /unix|linux|macos|darwin/.test(file)) continue;
    const abs = resolve(root, file);
    if (!existsSync(abs)) continue;
    const src = readFileSync(abs, "utf8");
    const lines = src.split("\n");
    const added = addedLines(base, file);
    if (added.size === 0) continue;
    const skip = (n: number) => (lines[n - 1] ?? "").includes("path-lint: ok");
    const add = (line: number, rule: string, fail: boolean) => {
      if (!skip(line)) findings.push({ file, line, rule, fail, text: (lines[line - 1] ?? "").trim() });
    };
    for (const a of asserts(src)) {
      let touched = false;
      for (let n = a.from; n <= a.to; n++) if (added.has(n)) touched = true;
      if (!touched || skip(a.from)) continue;
      if (DISPLAY.test(a.text) && literals(a.text).some(slashPath)) {
        add(a.from, "an assert compares a path's text with a `/` literal (Windows writes `\\`): compare Paths, or build the expected one with join", true);
      }
    }
    for (const n of added) {
      const line = lines[n - 1] ?? "";
      if (/^\s*\/\//.test(line)) continue;
      for (const m of line.matchAll(/\.join\(\s*(r#*)?"([^"]*)"/g)) {
        if (slashPath(m[2])) add(n, `join("${m[2]}") keeps the \`/\` on Windows: join each part`, false);
      }
      for (const m of line.matchAll(/format!\(\s*"([^"]*)"/g)) {
        if (!m[1].includes("://") && /\}\/|\/\{/.test(m[1]) && /display|path|dir|home|root|file|tmp/i.test(line)) {
          add(n, "format! builds a path with `/`: join it, or say why the text is meant (a URL, a key)", false);
        }
      }
      if (DISPLAY.test(line) && /\.(contains|ends_with|starts_with)\(|==/.test(line) && literals(line).some(slashPath)) {
        add(n, "a path's text is compared with a `/` literal (Windows writes `\\`)", false);
      }
      if (/\bbundle\(\)\s*\.(unwrap|expect)\(/.test(line)) {
        add(n, "bundle() assumed: a machine with no PEM roots file (Windows) has none", false);
      }
      if (/"\/etc\/(ssl|pki)|ca-certificates\.crt|"\/usr\/local\/etc\/openssl/.test(line)) {
        add(n, "a Unix roots file named: Windows keeps its roots in the system store", false);
      }
    }
  }
  return findings;
}

if (import.meta.main) {
  const i = process.argv.indexOf("--base");
  const findings = lint(i > 0 ? process.argv[i + 1] : "origin/main");
  for (const f of findings) {
    console.log(`${f.fail ? "error" : "warning"}: ${f.file}:${f.line}: ${f.rule}\n    ${f.text}`);
  }
  const fails = findings.filter((f) => f.fail).length;
  console.log(`path-lint: ${fails} errors, ${findings.length - fails} warnings (mark a deliberate line with \`path-lint: ok\`)`);
  process.exit(fails ? 1 : 0);
}

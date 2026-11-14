// Checks docs/port/behaviours/console.md (the Phase 9 console catalogue) against itself and the repository.
//
//   bun tools/check-console-catalogue.ts [--lean-tree] [--default-tree]     (from anywhere; the root is found from this file)
//
// It fails (exit 1) on:
//   1. a rule defined twice: a heading `## CON-n.` with a number already defined;
//   2. a reference to CON-n, anywhere in the docs or the sources, that the catalogue never defines;
//   3. a test name quoted in the catalogue (`path::to::a_snake_case_test_name`) with no `fn` of that name in any Rust source, and a
//      `file.rs` quoted that exists nowhere;
//   4. with --lean-tree: `cargo tree -p doze-kafka --no-default-features -e normal` names axum, hyper or tower (the lean build
//      must carry no HTTP);
//   5. with --default-tree: `cargo tree -p doze-kafka -e normal` (the default build) does not name axum, hyper and kafka-console
//      (decision D1 of docs/port/phase9.md: the console is in the default build; `default = []` would silently take it out and
//      the size limits, being upper bounds, would not notice).
// Only the standard library is used.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { basename, join, relative, resolve } from "node:path";

const root = resolve(import.meta.dir, "..");
const catalogue = process.argv.find((a) => a.endsWith(".md")) ?? join(root, "docs/port/behaviours/console.md");
const skip = new Set(["target", "node_modules", ".git", "corpus", "artifacts", "vendor", "scratchpad"]);
const scanned = /\.(md|rs|go|sh|ts|toml|yml|hcl)$/;

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (skip.has(name)) continue;
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else if (scanned.test(name)) out.push(path);
  }
  return out;
}

const problems: string[] = [];
const fail = (m: string) => problems.push(m);
const lines = readFileSync(catalogue, "utf8").split("\n");

const defined = new Map<string, number>();
lines.forEach((text, i) => {
  if (!/^#{1,6} /.test(text)) return;
  for (const m of text.matchAll(/CON-(\d+)\./g)) {
    if (defined.has(m[1])) fail(`CON-${m[1]} is defined twice: lines ${defined.get(m[1])} and ${i + 1}`);
    else defined.set(m[1], i + 1);
  }
});

const files = ["docs", "crates", "xtask", "interop", "conformance", "tools", ".github"].flatMap((d) => {
  try {
    return walk(join(root, d));
  } catch {
    return [];
  }
});
let references = 0;
for (const file of files) {
  const text = readFileSync(file, "utf8");
  if (!text.includes("CON-")) continue;
  const rel = relative(root, file);
  text.split("\n").forEach((line, i) => {
    for (const m of line.matchAll(/\bCON-(\d+)(?![\d-])/g)) {
      references++;
      if (!defined.has(m[1])) fail(`${rel}:${i + 1}: CON-${m[1]} is never defined in the catalogue`);
    }
  });
}

const rustFiles = files.filter((f) => f.endsWith(".rs"));
const fns = new Set([...rustFiles.map((f) => readFileSync(f, "utf8")).join("\n").matchAll(/\bfn\s+([A-Za-z_][A-Za-z0-9_]*)/g)].map((m) => m[1]));
const fileNames = new Set(rustFiles.map((f) => basename(f)));
let tests = 0;
lines.forEach((text, i) => {
  for (const span of text.matchAll(/`([^`]+)`/g)) {
    const quoted = span[1];
    const last = quoted.split("::").pop()!.trim();
    if (/^[a-z][a-z0-9]*(_[a-z0-9]+){2,}$/.test(last) && !quoted.includes("/") && !quoted.endsWith(".rs") && !/^--/.test(quoted)) {
      tests++;
      if (!fns.has(last)) fail(`line ${i + 1}: no \`fn ${last}\` in the Rust sources (\`${quoted}\`)`);
    }
    const file = quoted.match(/^([\w./-]+\.rs)$/);
    if (file && !fileNames.has(basename(file[1]))) fail(`line ${i + 1}: the file \`${quoted}\` exists nowhere`);
  }
});

let lean = "";
if (process.argv.includes("--lean-tree")) {
  const r = Bun.spawnSync(["cargo", "tree", "-p", "doze-kafka", "--no-default-features", "-e", "normal", "--prefix", "none"], { cwd: root });
  const tree = r.stdout.toString();
  if (r.exitCode !== 0) fail(`cargo tree failed: ${r.stderr.toString().slice(0, 200)}`);
  const names = new Set(tree.split("\n").map((l) => l.split(" ")[0]));
  const bad = ["axum", "hyper", "tower", "http", "kafka-console"].filter((n) => names.has(n));
  if (bad.length) fail(`the lean build's tree has ${bad.join(", ")}`);
  lean = `, lean tree ${names.size - 1} packages with no HTTP`;
}

let full = "";
if (process.argv.includes("--default-tree")) {
  const r = Bun.spawnSync(["cargo", "tree", "-p", "doze-kafka", "-e", "normal", "--prefix", "none"], { cwd: root });
  if (r.exitCode !== 0) fail(`cargo tree failed: ${r.stderr.toString().slice(0, 200)}`);
  const names = new Set(r.stdout.toString().split("\n").map((l) => l.split(" ")[0]));
  const missing = ["axum", "hyper", "kafka-console"].filter((n) => !names.has(n));
  if (missing.length) fail(`the default build's tree lacks ${missing.join(", ")}: the console is not in the default build (decision D1)`);
  full = `, default tree ${names.size - 1} packages with the console`;
}

console.log(`console catalogue: ${defined.size} rules defined, ${references} references, ${tests} test names, ${files.length} files read${lean}${full}`);
if (problems.length > 0) {
  for (const p of problems) console.log(`FAIL  ${p}`);
  console.log(`${problems.length} problem(s)`);
  process.exit(1);
}

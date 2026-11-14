// Checks docs/port/behaviours/registry.md (the Phase 10 catalogue) against itself and the repository.
//
//   bun tools/check-registry-catalogue.ts       (from anywhere; the root is found from this file)
//
// It fails (exit 1) on:
//   1. a rule or deviation defined twice: a heading `## REG-n.` or a bullet `- **REG-Dn.**` with a number already defined;
//   2. a reference to REG-n or REG-Dn, anywhere in the docs or the sources, that the catalogue never defines (the second
//      number of a range `REG-1 to REG-30` may be a reserved one);
//   3. a deviation defined but missing from the index at the top of the catalogue, or an index line for none;
//   4. a test name quoted in the catalogue (`path::to::a_snake_case_test_name`) with no `fn` of that name in any Rust
//      source (a name ending in `*` must be the start of one), and a `file.rs` quoted that exists nowhere.
// Only the standard library is used.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { basename, join, relative, resolve } from "node:path";

const root = resolve(import.meta.dir, "..");
// An optional path replaces the catalogue (the checker's own self-test uses it).
const catalogue = process.argv[2] ?? join(root, "docs/port/behaviours/registry.md");
const skip = new Set(["target", "node_modules", ".git", "corpus", "artifacts", "vendor", "scratchpad"]);
const scanned = /\.(md|rs|go|sh|ts|toml|yml|hcl)$/;

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (skip.has(name)) continue;
    const path = join(dir, name);
    const stat = statSync(path);
    if (stat.isDirectory()) walk(path, out);
    else if (scanned.test(name)) out.push(path);
  }
  return out;
}

const problems: string[] = [];
const fail = (message: string) => problems.push(message);

const lines = readFileSync(catalogue, "utf8").split("\n");

// 1. Definitions.
const defined = new Map<string, number>(); // "7" or "D7" -> line
const define = (id: string, line: number) => {
  const first = defined.get(id);
  if (first !== undefined) fail(`REG-${id} is defined twice: lines ${first} and ${line}`);
  else defined.set(id, line);
};
lines.forEach((text, i) => {
  if (/^#{1,6} /.test(text)) {
    for (const m of text.matchAll(/REG-(D?\d+)\./g)) define(m[1], i + 1);
  }
  const bullet = text.match(/^- \*\*REG-(D\d+)\.\*\*/);
  if (bullet) define(bullet[1], i + 1);
});

// 2. References, in every doc and source.
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
  if (!text.includes("REG-")) continue;
  const rel = relative(root, file);
  text.split("\n").forEach((line, i) => {
    const ranges = [...line.matchAll(/REG-(D?\d+) to REG-(D?\d+)/g)];
    const ends = new Set(ranges.map((m) => m.index! + m[0].lastIndexOf("REG-")));
    for (const m of line.matchAll(/REG-(D?\d+)(?![\d-])/g)) {
      references++;
      const isRangeEnd = ends.has(m.index!);
      if (!defined.has(m[1]) && !isRangeEnd) fail(`${rel}:${i + 1}: REG-${m[1]} is never defined in the catalogue`);
    }
  });
}

// 3. The index of deviations.
const indexStart = lines.findIndex((l) => /^## Index/.test(l));
const indexEnd = lines.findIndex((l, i) => i > indexStart && /^## /.test(l));
if (indexStart < 0) fail("the catalogue has no `## Index` section");
const indexed = new Set<string>();
lines.slice(indexStart + 1, indexEnd < 0 ? undefined : indexEnd).forEach((l, i) => {
  const m = l.match(/^- \*\*REG-(D\d+)\*\*/);
  if (!m) return;
  if (indexed.has(m[1])) fail(`the index lists REG-${m[1]} twice (line ${indexStart + 2 + i})`);
  indexed.add(m[1]);
  if (!defined.has(m[1])) fail(`the index lists REG-${m[1]}, which is never defined (line ${indexStart + 2 + i})`);
});
for (const id of defined.keys()) {
  if (id.startsWith("D") && !indexed.has(id)) fail(`REG-${id} (line ${defined.get(id)}) is missing from the index`);
}

// 4. Test names and files quoted in the catalogue.
const rustFiles = files.filter((f) => f.endsWith(".rs"));
const rustText = rustFiles.map((f) => readFileSync(f, "utf8")).join("\n");
const fns = new Set([...rustText.matchAll(/\bfn\s+([A-Za-z_][A-Za-z0-9_]*)/g)].map((m) => m[1]));
const fileNames = new Set(rustFiles.map((f) => basename(f)));
const allFiles = new Set(walk(join(root, "crates")).map((f) => relative(root, f)));
let tests = 0;
lines.forEach((text, i) => {
  for (const span of text.matchAll(/`([^`]+)`/g)) {
    const quoted = span[1];
    const last = quoted.split("::").pop()!.trim();
    // A quoted snake_case sentence of three words or more is a test, with a path (`a::b::the_test`) or without.
    const isTest = /^[a-z][a-z0-9]*(_[a-z0-9]+){2,}(_?\*)?$/.test(last) && !quoted.includes("/") && !quoted.endsWith(".rs");
    if (isTest) {
      tests++;
      if (last.endsWith("*")) {
        const prefix = last.slice(0, -1);
        if (![...fns].some((f) => f.startsWith(prefix))) fail(`line ${i + 1}: no \`fn ${prefix}…\` for \`${quoted}\``);
      } else if (!fns.has(last)) fail(`line ${i + 1}: no \`fn ${last}\` in the Rust sources (\`${quoted}\`)`);
    }
    const file = quoted.match(/^([\w./-]+\.rs)$/);
    if (file) {
      const name = basename(file[1]);
      const ok = file[1].includes("/") ? allFiles.has(file[1]) || [...allFiles].some((f) => f.endsWith(file[1])) : fileNames.has(name);
      if (!ok) fail(`line ${i + 1}: the file \`${quoted}\` exists nowhere`);
    }
  }
});

const rules = [...defined.keys()].filter((k) => !k.startsWith("D")).length;
console.log(
  `registry catalogue: ${rules} rules and ${defined.size - rules} deviations defined, ${references} references, ${tests} test names, ${files.length} files read`,
);
if (problems.length > 0) {
  for (const p of problems) console.log(`FAIL  ${p}`);
  console.log(`${problems.length} problem(s)`);
  process.exit(1);
}

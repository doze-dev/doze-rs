// What `just prepush` runs for this tree: the files changed against origin/main, the workspace packages they touch and every
// package that depends on those (normal, build and dev edges), and from that which of the expensive suites CI always runs must run
// here. CI has no path filters (it runs every job on every run), so the selection mirrors what each job exercises:
//
//   - a suite runs when the reverse-dependency closure of the changed packages reaches one of its packages;
//   - a change to a file outside every package that is not known to be inert (Cargo.lock, rust-toolchain.toml, ci.yml, the CI
//     scripts, this tool, models/, ...) selects everything;
//   - web/ selects the kafka console's job, console/ the aws console's two.
//
// The cheap, whole-workspace gates (fmt, clippy with every feature set CI lints, the workspace tests, rustdoc, the generated and
// catalogued files) never depend on the diff.
//
//   bun tools/prepush/plan.ts [--base <ref>]      prints the plan (what would run and why) and nothing else

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { relative, resolve } from "node:path";

export const root = resolve(import.meta.dir, "../..");

/** Files outside every package that change nothing CI runs. Anything else outside a package selects every suite. */
const INERT = [
  /^docs\//,
  /^spikes\//,
  /^scripts\//,
  /^[^/]+\.md$/,
  /^(LICENSE|NOTICE|justfile|\.gitignore)$/,
  /^\.github\/workflows\/(release|modules-release)\.yml$/,
];

export interface Plan {
  base: string;
  changed: string[];
  /** Why everything is selected (a file outside every package that is not inert), or null. */
  global: string | null;
  /** Workspace packages the changed files belong to. */
  touched: string[];
  /** `touched` and every package that depends on one of them. */
  closure: Set<string>;
  /** suite -> the reason it runs, or null when it does not. */
  suites: Map<string, string | null>;
}

/** git's output with its trailing newline gone (not its leading spaces: `status --porcelain` columns start with one). */
function git(...args: string[]): string {
  return execFileSync("git", args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trimEnd();
}

/** Changed files: origin/main's merge base to HEAD, plus what is modified, staged or untracked (not ignored) in the tree. */
export function changedFiles(baseRef: string): { base: string; files: string[] } {
  const base = git("merge-base", "HEAD", baseRef);
  const files = new Set(git("diff", "--name-only", base, "HEAD").split("\n").filter(Boolean));
  for (const line of git("status", "--porcelain", "--untracked-files=all").split("\n").filter(Boolean)) {
    const path = line.slice(3);
    // A rename reads `old -> new`: both sides changed.
    for (const p of path.split(" -> ")) files.add(p.replace(/^"|"$/g, ""));
  }
  return { base, files: [...files].sort() };
}

interface Meta {
  /** Package name -> its directory, relative to the root (path packages only: the workspace and vendor/). */
  dirs: Map<string, string>;
  /** Package name -> the path packages that depend on it, any edge kind. */
  dependents: Map<string, Set<string>>;
}

function metadata(): Meta {
  const out = execFileSync("cargo", ["metadata", "--format-version", "1", "--locked"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 256 << 20,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const m = JSON.parse(out) as {
    packages: { id: string; name: string; manifest_path: string; source: string | null }[];
    resolve: { nodes: { id: string; deps: { pkg: string }[] }[] };
  };
  const byId = new Map(m.packages.map((p) => [p.id, p]));
  const dirs = new Map<string, string>();
  for (const p of m.packages) {
    if (p.source === null) dirs.set(p.name, relative(root, resolve(p.manifest_path, "..")));
  }
  const dependents = new Map<string, Set<string>>();
  for (const node of m.resolve.nodes) {
    const from = byId.get(node.id);
    if (!from || from.source !== null) continue;
    for (const d of node.deps) {
      const to = byId.get(d.pkg);
      if (!to || to.source !== null) continue;
      if (!dependents.has(to.name)) dependents.set(to.name, new Set());
      dependents.get(to.name)!.add(from.name);
    }
  }
  return { dirs, dependents };
}

/** The `-p <package>` names a CI script passes to cargo. */
export function scriptPackages(script: string): string[] {
  const text = readFileSync(resolve(root, script), "utf8");
  return [...new Set([...text.matchAll(/-p ([a-z0-9][a-z0-9_-]*)/g)].map((m) => m[1]))];
}

/** Each suite and the packages whose closure selects it (path rules are applied separately). */
export function suiteRoots(): Map<string, string[]> {
  return new Map([
    ["os", scriptPackages(".github/ci/os.sh")],
    ["modules", scriptPackages(".github/ci/modules.sh")],
    ["app", ["doze-app"]],
    ["weight-doze", ["doze-cli"]],
    ["weight-kafka", ["doze-kafka"]],
    ["weight-aws", ["doze-aws"]],
    ["console-e2e", ["doze-aws", "aws-console"]],
    ["web", ["kafka-console"]],
    ["console-check", ["aws-console"]],
  ]);
}

/** Path rules: a changed file under one of these prefixes selects the suite whatever the packages say. */
const PATH_RULES: [string, RegExp][] = [
  ["web", /^web\//],
  ["console-check", /^console\//],
  ["console-e2e", /^console\//],
  ["weight-doze", /^xtask\/weight-limits\.txt$/],
  ["weight-kafka", /^xtask\/weight-limits\.txt$/],
  ["weight-aws", /^xtask\/weight-limits\.txt$/],
];

export function plan(baseRef = "origin/main", extraSuites: string[] = []): Plan {
  const { base, files } = changedFiles(baseRef);
  const meta = metadata();
  const dirList = [...meta.dirs.entries()].filter(([, d]) => d !== "").sort((a, b) => b[1].length - a[1].length);
  const touched = new Set<string>();
  let global: string | null = null;
  for (const f of files) {
    const owner = dirList.find(([, d]) => f === d || f.startsWith(d + "/"));
    if (owner) {
      touched.add(owner[0]);
      continue;
    }
    if (/^(web|console)\//.test(f)) continue; // their own suites, below
    if (INERT.some((r) => r.test(f))) continue;
    global ??= f;
  }
  const closure = new Set<string>();
  const queue = [...touched];
  while (queue.length) {
    const p = queue.pop()!;
    if (closure.has(p)) continue;
    closure.add(p);
    for (const d of meta.dependents.get(p) ?? []) queue.push(d);
  }
  const suites = new Map<string, string | null>();
  for (const [suite, roots] of suiteRoots()) {
    let why: string | null = null;
    if (global) why = `${global} is outside every package`;
    const hit = roots.filter((r) => closure.has(r));
    if (!why && hit.length) {
      const direct = [...touched].filter((t) => closure.has(t)).slice(0, 3);
      why = `${hit.slice(0, 3).join(", ")}${hit.length > 3 ? ` (+${hit.length - 3})` : ""} reached from ${direct.join(", ")}${touched.size > 3 ? ", ..." : ""}`;
    }
    for (const [s, r] of PATH_RULES) {
      if (!why && s === suite) {
        const f = files.find((x) => r.test(x));
        if (f) why = `${f} changed`;
      }
    }
    if (!why && extraSuites.includes(suite)) why = "asked for (--with)";
    suites.set(suite, why);
  }
  return { base, changed: files, global, touched: [...touched].sort(), closure, suites };
}

/** The fuzz job's steps whose crates the closure reaches (each step names its crates in `crates/<name>/fuzz`). */
export function fuzzCrates(stepText: string): string[] {
  return [...new Set([...stepText.matchAll(/crates\/([a-z0-9_-]+)\/fuzz/g)].map((m) => m[1]))];
}

if (import.meta.main) {
  const i = process.argv.indexOf("--base");
  const p = plan(i > 0 ? process.argv[i + 1] : "origin/main");
  console.log(`base ${p.base.slice(0, 9)}; ${p.changed.length} changed files; packages touched: ${p.touched.join(", ") || "none"}`);
  console.log(`closure: ${p.closure.size} packages${p.global ? `; everything selected (${p.global})` : ""}`);
  for (const [s, why] of p.suites) console.log(`  ${s.padEnd(14)} ${why ? `runs: ${why}` : "skipped: nothing it covers changed"}`);
}

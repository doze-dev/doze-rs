// The hash of the sources the doze-aws bundle is built from. crates/aws-console/src/web/tests.rs computes the same hash the
// same way and fails when web-dist/source-hash.txt differs: the committed bundle is stale.
//
// Files: everything under console/ except the directories in SKIP_DIRS, the files skipFile names, and every app under
// console/apps/ but `aws` (the gallery, and doze-kafka's app when it moves here, do not go into this bundle). Sorted by path
// (bytes, "/" separators, relative to console/); FNV-1a 64 over, per file: the path, a 0 byte, the content, a 0 byte.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";

export const SKIP_DIRS = new Set(["node_modules", "e2e", "dist", "test-results", "playwright-report", ".tmp"]);
export const skipFile = (name: string) => name === "README.md" || name === "eslint.config.js" || name.endsWith(".test.ts") || name.endsWith(".test.tsx") || name === ".DS_Store";

function walk(root: string, dir: string, out: string[]) {
  const apps = join(root, "apps");
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      const otherApp = dir === apps && name !== "aws";
      if (!SKIP_DIRS.has(name) && !otherApp) walk(root, path, out);
    } else if (!skipFile(name)) out.push(path);
  }
}

/** The sources, as paths relative to `consoleDir`, sorted by their bytes. */
export function sourceFiles(consoleDir: string): string[] {
  const out: string[] = [];
  walk(consoleDir, consoleDir, out);
  return out.map((p) => relative(consoleDir, p).split(sep).join("/")).sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
}

export function sourceHash(consoleDir: string): string {
  let h = 0xcbf29ce484222325n;
  const prime = 0x100000001b3n;
  const mask = 0xffffffffffffffffn;
  const eat = (bytes: Uint8Array) => {
    for (const b of bytes) {
      h ^= BigInt(b);
      h = (h * prime) & mask;
    }
  };
  for (const rel of sourceFiles(consoleDir)) {
    eat(new TextEncoder().encode(rel));
    eat(new Uint8Array([0]));
    eat(readFileSync(join(consoleDir, rel)));
    eat(new Uint8Array([0]));
  }
  return h.toString(16).padStart(16, "0");
}

if (import.meta.main) console.log(sourceHash(join(import.meta.dir, "../../..")));

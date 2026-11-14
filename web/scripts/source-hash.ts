// The hash of the web/ sources a bundle is built from. crates/kafka-console/src/web/tests.rs computes the same hash the same
// way and fails when web-dist/source-hash.txt differs: the committed bundle is stale.
//
// Files: everything under web/ except the directories in SKIP_DIRS and the files in SKIP_FILES (nothing in them reaches the
// bundle). Sorted by path (bytes, "/" separators); FNV-1a 64 over, per file: the path, a 0 byte, the content, a 0 byte.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";

export const SKIP_DIRS = new Set(["node_modules", "e2e", "dist", "test-results", "playwright-report", ".tmp"]);
export const SKIP_FILES = (name: string) => name === "README.md" || name === "eslint.config.js" || name.endsWith(".test.ts") || name === ".DS_Store";

function walk(dir: string, out: string[]) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      if (!SKIP_DIRS.has(name)) walk(path, out);
    } else if (!SKIP_FILES(name)) out.push(path);
  }
}

export function sourceFiles(webDir: string): string[] {
  const out: string[] = [];
  walk(webDir, out);
  return out.map((p) => relative(webDir, p).split(sep).join("/")).sort((a, b) => (Buffer.from(a) < Buffer.from(b) ? -1 : a === b ? 0 : 1));
}

export function sourceHash(webDir: string): string {
  let h = 0xcbf29ce484222325n;
  const prime = 0x100000001b3n;
  const mask = 0xffffffffffffffffn;
  const eat = (bytes: Uint8Array) => {
    for (const b of bytes) {
      h ^= BigInt(b);
      h = (h * prime) & mask;
    }
  };
  for (const rel of sourceFiles(webDir)) {
    eat(new TextEncoder().encode(rel));
    eat(new Uint8Array([0]));
    eat(readFileSync(join(webDir, rel)));
    eat(new Uint8Array([0]));
  }
  return h.toString(16).padStart(16, "0");
}

if (import.meta.main) console.log(sourceHash(join(import.meta.dir, "..")));

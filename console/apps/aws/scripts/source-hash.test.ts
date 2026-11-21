// The same cases as crates/aws-console/src/web/tests.rs (`the_hash_of_the_sources_is_sixteen_hex_digits_and_changes_with_a_file`),
// so the two implementations of the walk are held to one behaviour.
import { expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { sourceFiles, sourceHash } from "./source-hash";

function tree(): string {
  const root = mkdtempSync(join(tmpdir(), "doze-aws-hash-"));
  mkdirSync(join(root, "apps/aws/src"), { recursive: true });
  mkdirSync(join(root, "apps/gallery"), { recursive: true });
  mkdirSync(join(root, "packages/ui/node_modules/x"), { recursive: true });
  writeFileSync(join(root, "apps/aws/src/main.tsx"), "a");
  writeFileSync(join(root, "packages/ui/button.tsx"), "b");
  return root;
}

test("the hash is sixteen hex digits, and what does not go into the bundle does not change it", () => {
  const root = tree();
  try {
    const first = sourceHash(root);
    expect(first).toMatch(/^[0-9a-f]{16}$/);
    writeFileSync(join(root, "apps/gallery/main.tsx"), "g");
    writeFileSync(join(root, "packages/ui/node_modules/x/i.js"), "n");
    writeFileSync(join(root, "apps/aws/src/main.test.ts"), "t");
    writeFileSync(join(root, "apps/aws/src/page.test.tsx"), "t");
    writeFileSync(join(root, "README.md"), "r");
    writeFileSync(join(root, "eslint.config.js"), "e");
    mkdirSync(join(root, "apps/aws/e2e"), { recursive: true });
    writeFileSync(join(root, "apps/aws/e2e/x.spec.ts"), "s");
    expect(sourceHash(root)).toBe(first);
    writeFileSync(join(root, "packages/ui/button.tsx"), "c");
    expect(sourceHash(root)).not.toBe(first);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the files are relative to console/, with slashes, sorted by their bytes", () => {
  const root = tree();
  try {
    writeFileSync(join(root, "apps/aws/Z.ts"), "z");
    writeFileSync(join(root, "apps/aws/a.ts"), "a");
    expect(sourceFiles(root)).toEqual(["apps/aws/Z.ts", "apps/aws/a.ts", "apps/aws/src/main.tsx", "packages/ui/button.tsx"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("the hash of a known tree is the FNV-1a 64 the Rust test computes", () => {
  // One file `a` holding `b`: FNV-1a 64 over "a", 0, "b", 0.
  const root = mkdtempSync(join(tmpdir(), "doze-aws-hash-"));
  try {
    writeFileSync(join(root, "a"), "b");
    let h = 0xcbf29ce484222325n;
    for (const byte of [0x61, 0, 0x62, 0]) h = ((h ^ BigInt(byte)) * 0x100000001b3n) & 0xffffffffffffffffn;
    expect(sourceHash(root)).toBe(h.toString(16).padStart(16, "0"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

// bun run build: the Vite build into crates/kafka-console/web-dist/, then what the Rust side needs from it:
//   1. a .gz beside every text file (level 9; the server sends it as it is to a client that accepts gzip),
//   2. web-dist/manifest.rs: one include_bytes! line per file (the .gz where there is one), included by src/web.rs,
//   3. web-dist/source-hash.txt: the hash of the web/ sources (scripts/source-hash.ts), which a Rust test checks.
// It fails if index.html carries an inline script or a script file evaluates strings: the CSP forbids both.
import { gzipSync } from "bun";
import { readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { sourceHash } from "./source-hash.ts";

const web = join(import.meta.dir, "..");
const dist = join(web, "../crates/kafka-console/web-dist");

const vite = Bun.spawnSync(["bunx", "--bun", "vite", "build", "--logLevel", "warn"], { cwd: web, stdout: "inherit", stderr: "inherit" });
if (vite.exitCode !== 0) process.exit(vite.exitCode ?? 1);

const TYPES: Record<string, [string, boolean]> = {
  html: ["text/html; charset=utf-8", true],
  js: ["text/javascript; charset=utf-8", true],
  css: ["text/css; charset=utf-8", true],
  svg: ["image/svg+xml", true],
  json: ["application/json", true],
  txt: ["text/plain; charset=utf-8", true],
  woff2: ["font/woff2", false],
  png: ["image/png", false],
};

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else out.push(relative(dist, path).split(sep).join("/"));
  }
  return out;
}

const problems: string[] = [];
const files = walk(dist).filter((f) => !f.endsWith(".gz") && f !== "manifest.rs" && f !== "source-hash.txt").sort();
const lines: string[] = [];
let raw = 0;
let embedded = 0;
for (const rel of files) {
  const ext = rel.split(".").pop() ?? "";
  const kind = TYPES[ext];
  if (!kind) {
    problems.push(`${rel}: no content type for .${ext}`);
    continue;
  }
  const [type, text] = kind;
  const bytes = readFileSync(join(dist, rel));
  raw += bytes.length;
  if (ext === "html" && /<script(?![^>]*\bsrc=)[^>]*>/i.test(bytes.toString())) problems.push(`${rel}: an inline <script> (the CSP forbids it)`);
  if (ext === "js" && /\beval\(|new Function\(/.test(bytes.toString())) problems.push(`${rel}: evaluates a string (the CSP forbids it)`);
  let path = rel;
  if (text) {
    const gz = gzipSync(bytes, { level: 9 });
    writeFileSync(join(dist, `${rel}.gz`), gz);
    embedded += gz.length;
    path = `${rel}.gz`;
  } else embedded += bytes.length;
  lines.push(
    `    WebFile {\n        path: ${JSON.stringify(rel)},\n        content_type: ${JSON.stringify(type)},\n        gzip: ${text},\n        bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), ${JSON.stringify(`/web-dist/${path}`)})),\n    },`,
  );
}
if (problems.length) {
  for (const p of problems) console.error(`build: ${p}`);
  process.exit(1);
}
writeFileSync(
  join(dist, "manifest.rs"),
  `// Written by web/scripts/build.ts (bun run build in web/). Do not edit.\n\n/// Every file of the bundle.\npub const FILES: &[WebFile] = &[\n${lines.join("\n")}\n];\n`,
);
writeFileSync(join(dist, "source-hash.txt"), `${sourceHash(web)}\n`);
// Vite's own copy of the public dir name list is not needed; nothing else is left behind.
rmSync(join(dist, ".vite"), { recursive: true, force: true });
console.log(`web-dist: ${files.length} files, ${(raw / 1024).toFixed(1)} KiB raw, ${(embedded / 1024).toFixed(1)} KiB embedded`);

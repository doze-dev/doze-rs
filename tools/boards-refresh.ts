// Refreshes the committed design contract from the canvas.
//
//   bun tools/boards-refresh.ts <canvas-dir> [--dry-run]
//
// <canvas-dir> is the canvas project (the folder with src/, kit/ and shots/). The board sources
// `src/<section>/<id>.html` are copied to crates/kafka-console/design/boards/<section>/<id>.html, and
// `kit/paper.css`, `kit/kafka.css` to design/kit/. Folders `docs` and `_docs` are not boards. Every file must carry a
// `<!--screen {json} -->` front matter whose id is the file name. The report lists the ids added and removed
// (and changed) since the last refresh; removed boards are deleted from the contract. Then run
// `bun tools/boards-snapshot.ts` to regenerate design/boards.json and commit both.
//
// Dependency-free: only the standard library.

import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { boardsDir, kitDir, loadBoards, parseArgs } from "./lib/boards.ts";
import { frontMatter } from "./board-signature.ts";

const { flags, rest } = parseArgs(process.argv.slice(2), []);
const dry = flags.has("dry-run");
if (rest.length !== 1) {
  console.error("usage: bun tools/boards-refresh.ts <canvas-dir> [--dry-run]");
  process.exit(2);
}
const canvas = resolve(rest[0]);
const src = join(canvas, "src");
if (!existsSync(src)) {
  console.error(`no src/ in ${canvas}`);
  process.exit(2);
}

const problems: string[] = [];
const incoming = new Map<string, { section: string; path: string; html: string }>();
for (const section of readdirSync(src).sort()) {
  const dir = join(src, section);
  if (!statSync(dir).isDirectory() || section === "docs" || section === "_docs") continue;
  for (const f of readdirSync(dir).sort()) {
    if (!f.endsWith(".html")) {
      problems.push(`${section}/${f}: not an .html file`);
      continue;
    }
    const path = join(dir, f);
    const html = readFileSync(path, "utf8");
    const fm = frontMatter(html);
    const id = f.replace(/\.html$/, "");
    if (!fm) problems.push(`${section}/${f}: no <!--screen {json} --> front matter (not a board)`);
    else if (fm.id !== id) problems.push(`${section}/${f}: front matter id "${fm.id}" is not the file name`);
    else if (incoming.has(id)) problems.push(`${section}/${f}: id "${id}" is also in ${incoming.get(id)!.section}`);
    else incoming.set(id, { section, path, html });
  }
}
for (const css of ["paper.css", "kafka.css"]) if (!existsSync(join(canvas, "kit", css))) problems.push(`kit/${css} is missing`);
if (problems.length) {
  console.error("refresh refused:\n" + problems.map((p) => "  x " + p).join("\n"));
  process.exit(1);
}

const have = new Map(loadBoards().map((b) => [b.id, b]));
const added = [...incoming.keys()].filter((id) => !have.has(id)).sort();
const removed = [...have.keys()].filter((id) => !incoming.has(id)).sort();
const changed = [...incoming.keys()].filter((id) => have.has(id) && have.get(id)!.html !== incoming.get(id)!.html).sort();

if (!dry) {
  for (const id of removed) rmSync(have.get(id)!.path);
  for (const [id, b] of incoming) {
    const dest = join(boardsDir, b.section, `${id}.html`);
    mkdirSync(join(boardsDir, b.section), { recursive: true });
    if (have.has(id) && have.get(id)!.path !== dest) rmSync(have.get(id)!.path);
    copyFileSync(b.path, dest);
  }
  mkdirSync(kitDir, { recursive: true });
  for (const css of ["paper.css", "kafka.css"]) copyFileSync(join(canvas, "kit", css), join(kitDir, css));
}

// Verify what is on disk now (or would be).
const after = dry ? incoming.size : loadBoards().length;
const sections = new Map<string, number>();
for (const b of incoming.values()) sections.set(b.section, (sections.get(b.section) ?? 0) + 1);
console.log(`${dry ? "would copy" : "copied"} ${incoming.size} boards in ${sections.size} sections (${[...sections].map(([s, n]) => `${s} ${n}`).join(", ")})`);
console.log(`added   ${added.length}${added.length ? ": " + added.join(", ") : ""}`);
console.log(`removed ${removed.length}${removed.length ? ": " + removed.join(", ") : ""}`);
console.log(`changed ${changed.length}${changed.length ? ": " + changed.join(", ") : ""}`);
if (after !== incoming.size) {
  console.error(`count mismatch: ${incoming.size} sources, ${after} boards on disk`);
  process.exit(1);
}
console.log(dry ? "dry run: nothing written" : "next: bun tools/boards-snapshot.ts, then commit design/ and boards.json");

// Computes the signature of every board and writes crates/kafka-console/design/boards.json.
//
//   bun tools/boards-snapshot.ts [--check] [--boards dir] [--out file]
//
// Run when a board changes (after tools/boards-refresh.ts). The output is deterministic: boards sorted by id, keys
// sorted, two-space indent, one trailing newline. With --check nothing is written; the exit status is 1 if the
// committed file differs from what the boards give (CI uses this so the snapshot cannot go stale).
// The signature itself is defined, once, in tools/board-signature.ts and documented in docs/port/console/gate.md.

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { boardSignature, boardsDir, designDir, loadBoards, parseArgs } from "./lib/boards.ts";
import { type Signature, SIGNATURE_VERSION, signature, stableStringify } from "./board-signature.ts";

export interface Snapshot {
  version: number;
  boards: Record<string, { section: string; signature: Signature }>;
}

export function buildSnapshot(dir: string = boardsDir): { snapshot: Snapshot; problems: string[] } {
  const problems: string[] = [];
  const boards = loadBoards(dir);
  const ids = new Set<string>();
  const snapshot: Snapshot = { version: SIGNATURE_VERSION, boards: {} };
  for (const b of boards.sort((x, y) => (x.id < y.id ? -1 : 1))) {
    if (ids.has(b.id)) problems.push(`${b.section}/${b.id}: duplicate id`);
    ids.add(b.id);
    if (b.fm.id !== b.id) problems.push(`${b.section}/${b.id}: front matter id is "${b.fm.id}"`);
    snapshot.boards[b.id] = { section: b.section, signature: signature(b.html) };
  }
  // A state of a page has the page's title (one route, one breadcrumb); see pageTitle.
  const fms = new Map(boards.map((b) => [b.id, b.fm as Record<string, unknown>]));
  for (const b of boards) if (snapshot.boards[b.id]) snapshot.boards[b.id].signature = boardSignature(b, fms);
  for (const [id, { signature: s }] of Object.entries(snapshot.boards)) {
    for (const l of s.links) if (!ids.has(l)) problems.push(`${id}: links to "${l}", which is not a board`);
    for (const b of s.buttons) if (b.action.startsWith("to:") && !ids.has(b.action.slice(3))) problems.push(`${id}: button "${b.label}" goes to "${b.action.slice(3)}", which is not a board`);
    const empty = s.headings.length + s.h1.length + s.buttons.length + s.links.length + s.cards.length === 0;
    if (empty) problems.push(`${id}: empty signature`);
  }
  return { snapshot, problems };
}

if (import.meta.main) {
  const { flags, values } = parseArgs(process.argv.slice(2), ["boards", "out"]);
  const out = values.out ?? join(designDir, "boards.json");
  const { snapshot, problems } = buildSnapshot(values.boards ?? boardsDir);
  const text = stableStringify(snapshot) + "\n";
  const n = Object.keys(snapshot.boards).length;
  for (const p of problems) console.error("x " + p);
  if (flags.has("check")) {
    const have = existsSync(out) ? readFileSync(out, "utf8") : "";
    if (have !== text) {
      console.error(`${out} is out of date with the boards: run bun tools/boards-snapshot.ts`);
      process.exit(1);
    }
    console.log(`boards.json is current (${n} boards)`);
    process.exit(problems.length ? 1 : 0);
  }
  writeFileSync(out, text);
  console.log(`wrote ${out}: ${n} boards`);
  process.exit(problems.length ? 1 : 0);
}

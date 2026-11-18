// Mutants of the real sources: truncations, deletions, duplications and insertions of HCL
// fragments, so refusals happen inside realistic structure. bun gen-mut.ts <crate> <n> <seed>
import { readdirSync } from "node:fs";
const crate = process.argv[2];
const n = Number(process.argv[3] ?? 1500);
let seed = Number(process.argv[4] ?? 11);
const rnd = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648);
const int = (k: number) => Math.floor(rnd() * k);
const srcs = readdirSync(`${crate}/testdata/sources`).filter((f) => f.endsWith(".hcl")).sort()
  .map((f) => Bun.file(`${crate}/testdata/sources/${f}`));
const texts = await Promise.all(srcs.map((f) => f.text()));
const frags = ["{", "}", "[", "]", "(", ")", "\"", "${", "%{", "}", "~}", "\n", ",", "=", ":", ".", "..", "...", "?", "<<EOT\n", "EOT\n",
  "for", "in", "if", "=>", "#", "/*", "*/", "\\", "\\q", "$", "1.2.3", "a.0.1", ".*.", "[*]", "&", ";", "'", "`", "\t", "\r", "“", "%{ endif }", "%{ else }"];
const out: { id: string; src: string }[] = [];
for (let i = 0; i < n; i++) {
  let s = texts[int(texts.length)];
  const ops = 1 + int(3);
  for (let k = 0; k < ops; k++) {
    const at = int(s.length + 1);
    switch (int(4)) {
      case 0: s = s.slice(0, at); break; // truncate
      case 1: s = s.slice(0, at) + s.slice(at + 1 + int(6)); break; // delete
      case 2: s = s.slice(0, at) + frags[int(frags.length)] + s.slice(at); break; // insert
      default: { const b = int(s.length + 1); s = s.slice(0, at) + s.slice(Math.min(at, b), Math.max(at, b)) + s.slice(at); } // duplicate
    }
  }
  out.push({ id: `mut.${String(i).padStart(4, "0")}`, src: s });
}
await Bun.write(`${crate}/testdata/syntax/extra-mut.json`, JSON.stringify(out));

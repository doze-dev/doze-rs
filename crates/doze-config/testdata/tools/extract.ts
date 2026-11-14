// H4 corpus: every inline HCL of doze's internal/config tests (Go raw strings that look like
// doze.hcl), every ```hcl fence of the docs TestDocExamplesParse reads, and examples/*.
// bun extract.ts <doze checkout> <out dir>
import { readdirSync, mkdirSync, statSync } from "node:fs";
const [doze, out] = process.argv.slice(2);
mkdirSync(`${out}/files`, { recursive: true });
const seen = new Set<string>();
const index: { id: string; from: string }[] = [];
const hash = (s: string) => new Bun.CryptoHasher("sha256").update(s).digest("hex").slice(0, 10);
const add = async (src: string, from: string) => {
  if (!/\b(module|app|service|task|stack|variable|locals)\b/.test(src)) return;
  const id = hash(src);
  if (seen.has(id)) return;
  seen.add(id);
  index.push({ id, from });
  await Bun.write(`${out}/files/${id}.hcl`, src);
};
const cfg = `${doze}/internal/config`;
for (const f of readdirSync(cfg).filter((f) => f.endsWith("_test.go")).sort()) {
  const text = await Bun.file(`${cfg}/${f}`).text();
  for (const m of text.matchAll(/`([^`]*)`/g)) {
    const s = m[1];
    if (s.includes("{") || s.includes("=")) await add(s.endsWith("\n") ? s : s + "\n", f);
  }
}
const docs = ["README.md", "CONTRIBUTING.md", "docs/getting-started.md"];
const walk = (d: string): string[] =>
  readdirSync(d).flatMap((n) => (statSync(`${d}/${n}`).isDirectory() ? walk(`${d}/${n}`) : [`${d}/${n}`]));
for (const p of [...docs.map((d) => `${doze}/${d}`), ...walk(`${doze}/website/src/content/docs`).filter((p) => /\.mdx?$/.test(p))]) {
  if (!(await Bun.file(p).exists())) continue;
  const text = await Bun.file(p).text();
  for (const m of text.matchAll(/```(?:hcl|terraform)[^\n]*\n([\s\S]*?)```/g)) await add(m[1], p.slice(doze.length + 1));
}
for (const ex of readdirSync(`${doze}/examples`)) {
  const p = `${doze}/examples/${ex}/doze.hcl`;
  if (await Bun.file(p).exists()) await add(await Bun.file(p).text(), `examples/${ex}`);
}
await Bun.write(`${out}/index.json`, JSON.stringify(index, null, 1));
console.log(index.length, "files");

// Builds crates/doze-hcl/testdata/syntax/{cases.json,go-lex.jsonl,go-check.jsonl}:
// random fragment sequences, the real sources, and every eval-corpus expression as `x = <expr>`.
// bun build-syntax-corpus.ts <crate dir> <oracle-dump>
import { readdirSync, mkdirSync } from "node:fs";
const crate = process.argv[2];
const oracle = process.argv[3];
const dir = `${crate}/testdata/syntax`;
mkdirSync(dir, { recursive: true });
const gen = (n: number, seed: number, p: string) =>
  JSON.parse(Bun.spawnSync(["bun", `${import.meta.dir}/gen-lex.ts`, String(n), String(seed), p]).stdout.toString());
const cases: { id: string; src: string }[] = [...gen(2500, 7, "rnd")];
for (const f of readdirSync(`${crate}/testdata/sources`).filter((f) => f.endsWith(".hcl")).sort()) {
  cases.push({ id: `src.${f.slice(0, -4)}`, src: await Bun.file(`${crate}/testdata/sources/${f}`).text() });
}
for (const f of readdirSync(`${crate}/testdata/eval`).filter((f) => /^exprs-.*\.json$/.test(f)).sort()) {
  for (const c of await Bun.file(`${crate}/testdata/eval/${f}`).json()) cases.push({ id: `expr.${c.id}`, src: `x = ${c.expr}\n` });
}
for (const extra of ["mut"]) {
  const p = `${dir}/extra-${extra}.json`;
  if (await Bun.file(p).exists()) cases.push(...(await Bun.file(p).json()));
}
await Bun.write(`${dir}/cases.json`, JSON.stringify(cases, null, 0).replaceAll('},{"id"', '},\n{"id"') + "\n");
const run = (mode: string) => {
  const r = Bun.spawnSync([oracle, mode, `${dir}/cases.json`], { env: { PATH: "/usr/bin:/bin" } });
  if (r.exitCode !== 0) throw new Error(r.stderr.toString());
  return r.stdout.toString();
};
await Bun.write(`${dir}/go-lex.jsonl`, run("lex"));
await Bun.write(`${dir}/go-check.jsonl`, run("check"));
console.log(cases.length, "cases");

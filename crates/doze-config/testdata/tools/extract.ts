// H4 corpus: inline HCL of doze's tests (Go raw strings that look like doze.hcl), every ```hcl
// fence of doze's docs and of doze-website, doze's examples/*, and doze-examples' doze.hcl files.
// bun extract.ts <doze checkout> <doze-website> <doze-examples> <out dir>
// Ids are the first 10 hex of sha256(source); the first source seen wins `from`.
import { readdirSync, mkdirSync, statSync } from "node:fs";
import { basename } from "node:path";
const [doze, website, examples, out] = process.argv.slice(2);
if (!out) throw new Error("usage: bun extract.ts <doze> <doze-website> <doze-examples> <out>");
mkdirSync(`${out}/files`, { recursive: true });
const seen = new Set<string>();
const index: { id: string; from: string }[] = [];
const count: Record<string, number> = {};
const hash = (s: string) => new Bun.CryptoHasher("sha256").update(s).digest("hex").slice(0, 10);
const add = async (src: string, from: string, source: string) => {
  if (!/\b(module|app|service|task|stack|variable|locals)\b/.test(src)) return;
  const id = hash(src);
  if (seen.has(id)) return;
  seen.add(id);
  index.push({ id, from });
  count[source] = (count[source] ?? 0) + 1;
  await Bun.write(`${out}/files/${id}.hcl`, src);
};
const skipDirs = new Set(["node_modules", "dist", ".git", "vendor", "testdata", ".astro"]);
// sorted=false keeps readdir order, which the original corpus used for doze's website (first `from` wins).
const walk = (d: string, sorted = true): string[] => {
  const names = readdirSync(d);
  if (sorted) names.sort();
  return names.flatMap((n) => {
    const p = `${d}/${n}`;
    if (statSync(p).isDirectory()) return skipDirs.has(n) ? [] : walk(p, sorted);
    return [p];
  });
};

// Strings outside internal/config must look like HCL, not Go, JSON or shell.
const looksHcl = (s: string) => {
  if (/^\s*[[{]/.test(s)) return false; // JSON
  if (/\bfunc\b|:=|^\s*package\s|^\s*#!|^\s*\$ /m.test(s)) return false; // Go, shell
  if (/^\s*"[^"\n]*"\s*:/m.test(s)) return false; // JSON members
  if (/^\s*[\w-]+\s*=\s*"[^"\n]*"[ \t]+[A-Za-z]/m.test(s)) return false; // prose: `sleep = "never" keeps the app on`
  return /^\s*[A-Za-z_][\w-]*(\s+("[^"\n]*"|[A-Za-z_][\w-]*))*\s*\{\s*(#.*|\/\/.*)?$/m.test(s) || /^\s*[A-Za-z_][\w-]*\s*=\s*\S/m.test(s);
};
const goStrings = async (file: string, from: string, strict: boolean, source: string) => {
  const text = await Bun.file(file).text();
  for (const m of text.matchAll(/`([^`]*)`/g)) {
    const s = m[1];
    if (!(s.includes("{") || s.includes("="))) continue;
    if (strict && !looksHcl(s)) continue;
    await add(s.endsWith("\n") ? s : s + "\n", from, source);
  }
};
const fences = async (file: string, from: string, source: string) => {
  const text = await Bun.file(file).text();
  for (const m of text.matchAll(/```(?:hcl|terraform)[^\n]*\n([\s\S]*?)```/g)) await add(m[1], from, source);
};

// 1. doze internal/config tests, unchanged from the original corpus (from = bare file name).
const cfg = `${doze}/internal/config`;
for (const f of readdirSync(cfg).filter((f) => f.endsWith("_test.go")).sort())
  await goStrings(`${cfg}/${f}`, f, false, "doze internal/config tests");
// 2. doze docs and the in-repo website, as before.
const docs = ["README.md", "CONTRIBUTING.md", "docs/getting-started.md"];
const inRepoSite = `${doze}/website/src/content/docs`;
const sitePaths = statSync(inRepoSite, { throwIfNoEntry: false }) ?walk(inRepoSite, false).filter((p) => /\.mdx?$/.test(p)) : [];
for (const p of [...docs.map((d) => `${doze}/${d}`), ...sitePaths]) {
  if (!(await Bun.file(p).exists())) continue;
  await fences(p, p.slice(doze.length + 1), "doze docs");
}
// 3. doze examples/*/doze.hcl, as before.
for (const ex of readdirSync(`${doze}/examples`).sort()) {
  const p = `${doze}/examples/${ex}/doze.hcl`;
  if (await Bun.file(p).exists()) await add(await Bun.file(p).text(), `examples/${ex}`, "doze examples");
}
// 4. New: every other *_test.go in the doze checkout (strict filter).
for (const p of walk(doze).filter((p) => p.endsWith("_test.go") && !p.startsWith(`${cfg}/`)))
  await goStrings(p, `doze/${p.slice(doze.length + 1)}`, true, "doze other tests");
// 5. New: every hcl/terraform fence in doze-website content files.
const wsName = basename(website);
for (const p of walk(`${website}/src`).filter((p) => /\.(mdx?|markdoc|txt)$/.test(p)))
  await fences(p, `${wsName}/${p.slice(website.length + 1)}`, "doze-website");
// 6. New: doze.hcl and *.doze.hcl in doze-examples.
const exName = basename(examples);
for (const p of walk(examples).filter((p) => /(^|\/)(doze\.hcl|[^/]+\.doze\.hcl)$/.test(p))) {
  const rel = p.slice(examples.length + 1);
  const from = `${exName}/${basename(p) === "doze.hcl" ? rel.replace(/\/?doze\.hcl$/, "") || "doze.hcl" : rel}`;
  await add(await Bun.file(p).text(), from, "doze-examples");
}
await Bun.write(`${out}/index.json`, JSON.stringify(index, null, 1));
console.log(index.length, "files", count);

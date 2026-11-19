// The decode corpus: bodies read against schemas, as single files and merged.
// bun gen-content.ts <crate dir> > ../decode/cases.json
import { readdirSync } from "node:fs";
const crate = process.argv[2];
const schemas = {
  a: { attrs: [["name", true], ["port", false], ["run", false]], blocks: [["app", ["name"]], ["env", []], ["module", ["type", "name"]]] },
  b: { attrs: [["stack", false], ["version", false]], blocks: [["app", ["name"]], ["service", ["name"]], ["task", ["name"]], ["module", ["type", "name"]], ["variable", ["name"]], ["locals", []]] },
  c: { attrs: [], blocks: [] },
};
const small = [
  "", "name = 1\n", "port = 1\n", "name = 1\nport = 2\nrun = 3\n", "nmae = 1\n", "name = 1\nextra = 2\n", "app = 1\nname = 2\n",
  "name = 1\napp \"a\" {}\n", "name = 1\napp {}\n", "name = 1\napp \"a\" \"b\" {}\n", "name = 1\nenv \"x\" {}\n", "name = 1\nenv {}\nenv {}\n",
  "name = 1\nmodule \"pg\" {}\n", "name = 1\nmodule pg db {}\n", "name = 1\napps \"a\" {}\n", "name = 1\nname2 {}\n", "name = 1\nport {}\n",
  "name = 1\napp \"a\" { port = 1 }\n", "name = 1\napp \"a\" {\n  name = \"x\"\n}\n", "name = 1\napp \"a\" { nam = 1 }\n",
  "# c\nport = 1\n", "\n\n  port = 1\n", "app \"a\" {\n}\n", "app \"a\" { run = 1 }\n", "/* c */ port = 1\n",
  "name = 1\napp \"a\" {\n  app \"b\" {}\n}\n", "name = 1\nunknown \"x\" {}\nother = 2\n",
];
const cases: any[] = [];
let n = 0;
const add = (srcs: string[], mode: string, s: keyof typeof schemas) => cases.push({ id: `dec.${String(n++).padStart(4, "0")}`, srcs, mode, ...schemas[s] });
for (const src of small) for (const mode of ["content", "partial", "just"]) for (const s of ["a", "c"] as const) add([src], mode, s);
for (let i = 0; i < small.length; i++) for (const mode of ["content", "partial"]) add([small[i], small[(i * 7 + 3) % small.length]], mode, "a");
const srcs = readdirSync(`${crate}/testdata/sources`).filter((f) => f.endsWith(".hcl")).sort();
for (const f of srcs) {
  const t = await Bun.file(`${crate}/testdata/sources/${f}`).text();
  for (const mode of ["content", "partial"]) add([t], mode, "b");
}
for (let i = 0; i + 1 < srcs.length; i += 2) {
  const a = await Bun.file(`${crate}/testdata/sources/${srcs[i]}`).text();
  const b = await Bun.file(`${crate}/testdata/sources/${srcs[i + 1]}`).text();
  add([a, b], "content", "b");
}
console.log(JSON.stringify(cases).replaceAll('},{"id"', '},\n{"id"'));

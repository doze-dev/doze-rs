// Generates the corpus of config blocks the Go and Rust modules are compared on, from each module's own published
// description (`oracle describe`): the example, every declared version, every argument alone and together, pairs,
// nested blocks (and a duplicated label), version gates, an unknown argument, and a type mismatch.
//
//   bun tools/module-oracle/corpus.ts describe.jsonl > tools/module-oracle/corpus.jsonl

type Arg = { Name: string; Type: string; Default: string; Required: boolean; Since: string; Until: string };
type Block = { Name: string; Label: string; Args: Arg[] };
type Desc = { Versions: string[]; Example: string; Config: Arg[] | null; Blocks: Block[] | null };

const CORE = new Set(["count", "for_each", "depends_on", "enabled", "after", "version", "listen", "port", "sleep"]);

// A plausible value for an argument, by type and name (names the modules validate get values they accept).
function sample(a: Arg, variant = 0): string | null {
  const n = a.Name;
  switch (a.Type) {
    case "number":
      return n.includes("port") ? String(7100 + variant) : String(7 + variant);
    case "bool":
      return variant % 2 === 0 ? "true" : "false";
    case "map(string)":
      return variant % 2 === 0 ? `{ tuning = "on" }` : `{ alpha = "1", beta = "two" }`;
    case "list(string)":
      return n === "extensions" ? `["pg_trgm"]` : `["a", "b"]`;
    case "string": {
      if (n.includes("password") || n.includes("token")) return `"s3cret-${variant}"`;
      if (n.includes("memory") || n.includes("buffers")) return `"256MB"`;
      if (n.includes("policy")) return `"allkeys-lru"`;
      if (n.includes("retention")) return `"24h"`;
      if (n === "locale" || n.startsWith("lc_")) return `"C"`;
      if (n === "encoding") return `"UTF8"`;
      if (n.includes("time") || n.includes("until")) return `"2030-01-01"`;
      return `"value-${variant}"`;
    }
    default:
      return null;
  }
}

function block(engine: string, version: string, body: string[]): string {
  const lines = [`  version = "${version}"`, ...body.map((l) => "  " + l)];
  return `${engine} "t" {\n${lines.join("\n")}\n}\n`;
}

function nested(b: Block, label: string, variant = 0): string {
  const inner = b.Args.map((a) => [a.Name, sample(a, variant)] as const).filter(([, v]) => v !== null);
  const lab = b.Label ? ` "${label}"` : "";
  return `${b.Name}${lab} {\n${inner.map(([k, v]) => `    ${k} = ${v}`).join("\n")}\n  }`;
}

const cases: { id: string; engine: string; version: string; hcl: string }[] = [];
const add = (engine: string, kind: string, version: string, hcl: string) =>
  cases.push({ id: `${engine}/${String(cases.filter((c) => c.engine === engine).length).padStart(3, "0")}-${kind}`, engine, version, hcl });

const input = (await Bun.file(process.argv[2]).text()).trim().split("\n").map((l) => JSON.parse(l));
for (const { engine, description: d } of input as { engine: string; description: Desc }[]) {
  const latest = d.Versions.at(-1) ?? "";
  const args = (d.Config ?? []).filter((a) => !CORE.has(a.Name));
  const plain = args.filter((a) => a.Type !== "block" && sample(a) !== null);
  const blocks: Block[] = [...(d.Blocks ?? []), ...args.filter((a) => a.Type === "block").map((a) => ({ Name: a.Name, Label: "name", Args: [] }))];
  const example = d.Example.match(/version\s*=\s*"([^"]+)"/)?.[1] ?? latest;
  add(engine, "example", example, d.Example.endsWith("\n") ? d.Example : d.Example + "\n");
  for (const v of d.Versions) add(engine, `version-${v}`, v, block(engine, v, []));
  plain.forEach((a, i) => add(engine, `arg-${a.Name}`, latest, block(engine, latest, [`${a.Name} = ${sample(a, i)}`])));
  plain.forEach((a, i) => add(engine, `arg-${a.Name}-alt`, latest, block(engine, latest, [`${a.Name} = ${sample(a, i + 1)}`])));
  if (plain.length > 1) add(engine, "all-args", latest, block(engine, latest, plain.map((a, i) => `${a.Name} = ${sample(a, i)}`)));
  for (let i = 0; i + 1 < plain.length; i += 2) {
    add(engine, `pair-${plain[i].Name}-${plain[i + 1].Name}`, latest, block(engine, latest, [plain[i], plain[i + 1]].map((a, j) => `${a.Name} = ${sample(a, j + 3)}`)));
  }
  for (const b of blocks) {
    add(engine, `block-${b.Name}`, latest, block(engine, latest, [nested(b, "first")]));
    add(engine, `block-${b.Name}-two`, latest, block(engine, latest, [nested(b, "first"), nested(b, "second", 1)]));
    if (b.Label) add(engine, `block-${b.Name}-duplicate`, latest, block(engine, latest, [nested(b, "same"), nested(b, "same", 1)]));
    // Descriptions do not always say whether a block is labelled (or say it wrongly): try the other form too.
    const other = { ...b, Label: b.Label ? "" : "name" };
    add(engine, `block-${b.Name}-`, latest, block(engine, latest, [nested(other, "first")]));
    // With every argument set (some blocks need one: kvrocks namespaces need a password).
    add(engine, `block-${b.Name}-with-args`, latest, block(engine, latest, [...plain.map((a, i) => `${a.Name} = ${sample(a, i)}`), nested(b, "first")]));
  }
  for (const a of args.filter((a) => a.Since)) {
    const older = d.Versions.find((v) => Number(v.split(".")[0]) < Number(a.Since));
    if (older && sample(a)) add(engine, `gate-${a.Name}`, older, block(engine, older, [`${a.Name} = ${sample(a)}`]));
  }
  add(engine, "unknown-arg", latest, block(engine, latest, ["bogus_setting = 1"]));
  const str = plain.find((a) => a.Type === "string");
  if (str) add(engine, `wrong-type-${str.Name}`, latest, block(engine, latest, [`${str.Name} = ["not", "a", "string"]`]));
}
for (const c of cases) console.log(JSON.stringify(c));
console.error(`${cases.length} cases: ${[...new Set(cases.map((c) => c.engine))].map((e) => `${e} ${cases.filter((c) => c.engine === e).length}`).join(", ")}`);

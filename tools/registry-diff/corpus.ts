// A generated corpus of schema evolutions: for each format, chains of three schemas (v1, v2, v3), each a random change of
// the one before. Each chain is a scenario: v1 and v2 registered at NONE, then for every level the subject is set to it and
// v3 checked against the subject (all versions the level compares, and the latest alone, verbose), then v3 registered at one
// level. The verdicts and reasons are whatever the reference registry answers.
import { j, reg, type Step } from "./lib.ts";

const LEVELS = ["NONE", "BACKWARD", "BACKWARD_TRANSITIVE", "FORWARD", "FORWARD_TRANSITIVE", "FULL", "FULL_TRANSITIVE"];

/** mulberry32: a small seeded generator, so a corpus is the same every run. */
function rng(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
type R = () => number;
const pick = <T>(r: R, xs: readonly T[]): T => xs[Math.floor(r() * xs.length)];
const clone = <T>(x: T): T => structuredClone(x);

// ---------- Avro ----------
const AVRO_PRIMS = ["int", "long", "float", "double", "string", "bytes", "boolean", "null"] as const;
type AvroField = { name: string; type: unknown; default?: unknown; aliases?: string[] };
type AvroRec = { type: "record"; name: string; namespace?: string; fields: AvroField[] };

function avroType(r: R, depth: number, n: { k: number }): unknown {
  const x = r();
  if (depth > 1 || x < 0.55) return pick(r, AVRO_PRIMS);
  if (x < 0.65) return { type: "enum", name: `E${n.k++}`, symbols: ["A", "B", "C"].slice(0, 1 + Math.floor(r() * 3)) };
  if (x < 0.72) return { type: "array", items: avroType(r, depth + 1, n) };
  if (x < 0.79) return { type: "map", values: avroType(r, depth + 1, n) };
  if (x < 0.85) return { type: "fixed", name: `F${n.k++}`, size: 1 + Math.floor(r() * 8) };
  if (x < 0.93) return ["null", pick(r, ["int", "string", "long"])];
  return { type: "record", name: `N${n.k++}`, fields: [{ name: "x", type: pick(r, AVRO_PRIMS) }] };
}
function avroDefault(t: unknown): unknown {
  if (Array.isArray(t)) return t[0] === "null" ? null : undefined;
  switch (t) {
    case "int": case "long": return 0;
    case "float": case "double": return 1.5;
    case "string": return "";
    case "bytes": return "\u0000";
    case "boolean": return false;
    case "null": return null;
  }
  const o = t as { type?: string; symbols?: string[] };
  if (o?.type === "enum") return o.symbols?.[0];
  if (o?.type === "array") return [];
  if (o?.type === "map") return {};
  return undefined;
}
function avroStart(r: R, n: { k: number }): AvroRec {
  const fields: AvroField[] = [];
  const count = 1 + Math.floor(r() * 4);
  for (let i = 0; i < count; i++) fields.push({ name: `f${i}`, type: avroType(r, 0, n) });
  return { type: "record", name: "Rec", namespace: "gen", fields };
}
function avroMutate(r: R, s0: AvroRec, n: { k: number }): AvroRec {
  const s = clone(s0);
  const f = s.fields.length ? pick(r, s.fields) : undefined;
  const promotions: Record<string, string[]> = { int: ["long", "float", "double"], long: ["float", "double", "int"], float: ["double", "long"], double: ["float"], string: ["bytes"], bytes: ["string"] };
  switch (pick(r, ["add", "addDefault", "remove", "retype", "promote", "nullable", "enumAdd", "enumRemove", "rename", "alias", "rename-record", "namespace", "default", "fixed", "items", "reorder"])) {
    case "add": s.fields.push({ name: `g${n.k++}`, type: avroType(r, 0, n) }); break;
    case "addDefault": {
      const t = avroType(r, 0, n);
      const d = avroDefault(t);
      s.fields.push(d === undefined ? { name: `g${n.k++}`, type: t } : { name: `g${n.k++}`, type: t, default: d });
      break;
    }
    case "remove": if (f) s.fields = s.fields.filter((x) => x !== f); break;
    case "retype": if (f) { f.type = avroType(r, 0, n); delete f.default; } break;
    case "promote": if (f && typeof f.type === "string" && promotions[f.type]) { f.type = pick(r, promotions[f.type]); delete f.default; } break;
    case "nullable": if (f) { f.type = Array.isArray(f.type) ? f.type[1] : ["null", f.type]; if (Array.isArray(f.type)) f.default = null; else delete f.default; } break;
    case "enumAdd": { const e = s.fields.find((x) => (x.type as any)?.type === "enum"); if (e) (e.type as any).symbols.push(`S${n.k++}`); else s.fields.push({ name: `g${n.k++}`, type: { type: "enum", name: `E${n.k++}`, symbols: ["X"] } }); break; }
    case "enumRemove": { const e = s.fields.find((x) => (x.type as any)?.type === "enum" && (x.type as any).symbols.length > 1); if (e) (e.type as any).symbols.pop(); break; }
    case "rename": if (f) f.name = `r${n.k++}`; break;
    case "alias": if (f) { f.aliases = [f.name]; f.name = `a${n.k++}`; } break;
    case "rename-record": s.name = pick(r, ["Rec", "Other", "Rec2"]); break;
    case "namespace": s.namespace = pick(r, ["gen", "gen2", undefined as unknown as string]); if (!s.namespace) delete s.namespace; break;
    case "default": if (f) { const d = avroDefault(f.type); if (f.default !== undefined) delete f.default; else if (d !== undefined) f.default = d; } break;
    case "fixed": { const e = s.fields.find((x) => (x.type as any)?.type === "fixed"); if (e) (e.type as any).size += 1; break; }
    case "items": { const e = s.fields.find((x) => (x.type as any)?.type === "array" || (x.type as any)?.type === "map"); if (e) { const t = e.type as any; if (t.type === "array") t.items = pick(r, AVRO_PRIMS); else t.values = pick(r, AVRO_PRIMS); } break; }
    case "reorder": s.fields.reverse(); break;
  }
  return s;
}

// ---------- Protobuf ----------
const PB_SCALARS = ["int32", "int64", "uint32", "uint64", "sint32", "sint64", "fixed32", "fixed64", "sfixed32", "sfixed64", "bool", "string", "bytes", "double", "float"];
type PbField = { name: string; type: string; num: number; label?: "repeated" | "optional" | "required"; oneof?: string };
type PbMsg = { syntax: "proto2" | "proto3"; pkg: string; name: string; fields: PbField[]; nested: boolean; enumVals: string[] };

function pbStart(r: R): PbMsg {
  const syntax = r() < 0.75 ? "proto3" : "proto2";
  const fields: PbField[] = [];
  const count = 1 + Math.floor(r() * 4);
  for (let i = 0; i < count; i++) fields.push({ name: `f${i}`, type: pick(r, PB_SCALARS), num: i + 1, label: syntax === "proto2" ? pick(r, ["optional", "required", "repeated"] as const) : r() < 0.2 ? "repeated" : undefined });
  return { syntax, pkg: "gen", name: "Msg", fields, nested: r() < 0.3, enumVals: ["E_ZERO", "E_ONE"] };
}
function pbText(m: PbMsg): string {
  const lines = [`syntax = "${m.syntax}";`, `package ${m.pkg};`, "", `message ${m.name} {`];
  const oneofs = new Map<string, PbField[]>();
  for (const f of m.fields) {
    if (f.oneof) { oneofs.set(f.oneof, [...(oneofs.get(f.oneof) ?? []), f]); continue; }
    const label = f.label ? `${f.label} ` : m.syntax === "proto2" ? "optional " : "";
    lines.push(`  ${label}${f.type} ${f.name} = ${f.num};`);
  }
  for (const [o, fs] of oneofs) {
    lines.push(`  oneof ${o} {`);
    for (const f of fs) lines.push(`    ${f.type} ${f.name} = ${f.num};`);
    lines.push("  }");
  }
  if (m.nested) lines.push("  message Inner {", `    ${m.syntax === "proto2" ? "optional " : ""}string v = 1;`, "  }");
  lines.push("}");
  lines.push(`enum Kind {`, ...m.enumVals.map((v, i) => `  ${v} = ${i};`), "}");
  return lines.join("\n") + "\n";
}
function pbMutate(r: R, m0: PbMsg, n: { k: number }): PbMsg {
  const m = clone(m0);
  const f = m.fields.length ? pick(r, m.fields) : undefined;
  const next = () => Math.max(0, ...m.fields.map((x) => x.num)) + 1;
  const extra = ["Msg.Inner", "Kind", "Inner"];
  switch (pick(r, ["add", "remove", "retype", "renumber", "rename", "repeat", "oneof", "oneofRemove", "package", "rename-message", "nested", "enum", "message-type", "label", "syntax"])) {
    case "add": m.fields.push({ name: `g${n.k++}`, type: pick(r, PB_SCALARS), num: next(), label: m.syntax === "proto2" ? pick(r, ["optional", "required"] as const) : undefined }); break;
    case "remove": if (f) m.fields = m.fields.filter((x) => x !== f); break;
    case "retype": if (f) f.type = pick(r, PB_SCALARS); break;
    case "renumber": if (f) f.num = next() + 1; break;
    case "rename": if (f) f.name = `r${n.k++}`; break;
    case "repeat": if (f && !f.oneof) f.label = f.label === "repeated" ? (m.syntax === "proto2" ? "optional" : undefined) : "repeated"; break;
    case "oneof": if (f && f.label !== "repeated" && f.label !== "required") { f.oneof = "choice"; delete f.label; } else m.fields.push({ name: `o${n.k++}`, type: "string", num: next(), oneof: "choice" }); break;
    case "oneofRemove": { const o = m.fields.find((x) => x.oneof); if (o) m.fields = m.fields.filter((x) => x !== o); break; }
    case "package": m.pkg = pick(r, ["gen", "gen2"]); break;
    case "rename-message": m.name = pick(r, ["Msg", "Msg2"]); break;
    case "nested": m.nested = !m.nested; break;
    case "enum": if (r() < 0.5) m.enumVals.push(`E_V${n.k++}`); else if (m.enumVals.length > 1) m.enumVals.pop(); break;
    case "message-type": if (f) { f.type = m.nested ? pick(r, extra) : "Kind"; } break;
    case "label": if (f && m.syntax === "proto2" && !f.oneof) f.label = pick(r, ["optional", "required", "repeated"] as const); break;
    case "syntax": m.syntax = m.syntax === "proto3" ? "proto2" : "proto3"; break;
  }
  return m;
}

// ---------- JSON Schema ----------
type Js = Record<string, any>;
const JS_TYPES = ["string", "integer", "number", "boolean", "array", "object", "null"];
function jsProp(r: R): Js {
  const t = pick(r, JS_TYPES);
  const p: Js = { type: t };
  if (t === "string" && r() < 0.3) p.maxLength = 1 + Math.floor(r() * 20);
  if ((t === "integer" || t === "number") && r() < 0.3) p.minimum = Math.floor(r() * 10);
  if (t === "array") p.items = { type: pick(r, ["string", "integer"]) };
  if (t === "object") p.properties = { x: { type: "string" } };
  if (r() < 0.1) return { enum: ["a", "b"] };
  return p;
}
function jsStart(r: R): Js {
  const s: Js = { type: "object", properties: {} };
  if (r() < 0.3) s.$schema = pick(r, ["http://json-schema.org/draft-07/schema#", "http://json-schema.org/draft-04/schema#", "https://json-schema.org/draft/2020-12/schema"]);
  const count = 1 + Math.floor(r() * 4);
  for (let i = 0; i < count; i++) s.properties[`p${i}`] = jsProp(r);
  if (r() < 0.4) s.required = ["p0"];
  if (r() < 0.3) s.additionalProperties = r() < 0.5;
  return s;
}
function jsMutate(r: R, s0: Js, n: { k: number }): Js {
  const s = clone(s0);
  const keys = Object.keys(s.properties);
  const k = keys.length ? pick(r, keys) : undefined;
  switch (pick(r, ["add", "remove", "retype", "require", "unrequire", "closed", "open", "maxLength", "minimum", "enum", "items", "intnum", "oneOf", "default"])) {
    case "add": s.properties[`q${n.k++}`] = jsProp(r); break;
    case "remove": if (k) { delete s.properties[k]; if (s.required) s.required = s.required.filter((x: string) => x !== k); if (!s.required?.length) delete s.required; } break;
    case "retype": if (k) s.properties[k] = jsProp(r); break;
    case "require": if (k) s.required = [...new Set([...(s.required ?? []), k])]; break;
    case "unrequire": delete s.required; break;
    case "closed": s.additionalProperties = false; break;
    case "open": if (r() < 0.5) delete s.additionalProperties; else s.additionalProperties = true; break;
    case "maxLength": { const p = Object.values(s.properties).find((x: any) => x.type === "string") as Js | undefined; if (p) p.maxLength = p.maxLength ? (r() < 0.5 ? p.maxLength + 1 : Math.max(0, p.maxLength - 1)) : 5; break; }
    case "minimum": { const p = Object.values(s.properties).find((x: any) => x.type === "integer" || x.type === "number") as Js | undefined; if (p) p.minimum = (p.minimum ?? 0) + (r() < 0.5 ? 1 : -1); break; }
    case "enum": { const p = Object.values(s.properties).find((x: any) => x.enum) as Js | undefined; if (p) { if (r() < 0.5) p.enum.push(`v${n.k++}`); else if (p.enum.length > 1) p.enum.pop(); } else if (k) s.properties[k] = { enum: ["a", "b"] }; break; }
    case "items": { const p = Object.values(s.properties).find((x: any) => x.type === "array") as Js | undefined; if (p) p.items = { type: pick(r, ["string", "integer", "number"]) }; break; }
    case "intnum": { const p = Object.values(s.properties).find((x: any) => x.type === "integer" || x.type === "number") as Js | undefined; if (p) p.type = p.type === "integer" ? "number" : "integer"; break; }
    case "oneOf": if (k) s.properties[k] = { oneOf: [s.properties[k], { type: "null" }] }; break;
    case "default": if (k) s.properties[k].default = null; break;
  }
  return s;
}

function chain(fmt: "AVRO" | "PROTOBUF" | "JSON", texts: string[], r: R): Step[] {
  const s = "/subjects/c";
  const extra = fmt === "AVRO" ? {} : { schemaType: fmt };
  const body = (t: string) => reg(t, extra);
  const steps: Step[] = [
    { m: "PUT", p: "/config/c", b: j({ compatibility: "NONE" }) },
    { m: "POST", p: `${s}/versions`, b: body(texts[0]) },
    { m: "POST", p: `${s}/versions`, b: body(texts[1]) },
  ];
  for (const l of LEVELS) {
    steps.push({ m: "PUT", p: "/config/c", b: j({ compatibility: l }) });
    steps.push({ m: "POST", p: "/compatibility/subjects/c/versions?verbose=true", b: body(texts[2]) });
    steps.push({ m: "POST", p: "/compatibility/subjects/c/versions/latest?verbose=true", b: body(texts[2]) });
  }
  steps.push({ m: "PUT", p: "/config/c", b: j({ compatibility: pick(r, LEVELS) }) });
  steps.push({ m: "POST", p: `${s}/versions`, b: body(texts[2]) });
  steps.push({ m: "GET", p: `${s}/versions/latest` });
  if (fmt === "PROTOBUF") {
    for (const k of [1, 2, 3]) steps.push({ m: "GET", p: `/schemas/ids/@${k}?format=serialized` });
  }
  return steps;
}

/** `count` chains per format, as scenarios named corpus/<format>/<n>. */
export function corpus(count: number, seed: number): Record<string, Step[]> {
  const out: Record<string, Step[]> = {};
  for (const fmt of ["AVRO", "PROTOBUF", "JSON"] as const) {
    const r = rng(seed * 7919 + fmt.length);
    for (let i = 0; i < count; i++) {
      const n = { k: 0 };
      let texts: string[];
      if (fmt === "AVRO") {
        const a = avroStart(r, n), b = avroMutate(r, a, n), c = avroMutate(r, b, n);
        texts = [a, b, c].map((x) => j(x));
      } else if (fmt === "PROTOBUF") {
        const a = pbStart(r), b = pbMutate(r, a, n), c = pbMutate(r, b, n);
        texts = [a, b, c].map(pbText);
      } else {
        const a = jsStart(r), b = jsMutate(r, a, n), c = jsMutate(r, b, n);
        texts = [a, b, c].map((x) => j(x));
      }
      out[`corpus/${fmt.toLowerCase()}/${String(i).padStart(3, "0")}`] = chain(fmt, texts, r);
    }
  }
  return out;
}

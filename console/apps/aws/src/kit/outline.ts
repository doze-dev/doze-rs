// The call drawn as an indented outline, the way the SNS boards draw it: the operation, then each member on a line of its own
// (`Name = value`), a nested member's name over its members, a long value under its name. Pure.

/** A line that would run past this many characters (indent included) puts its value on the next line, as the boards do. */
export const OUTLINE_WIDTH = 32;

const isMap = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/** One scalar as a person reads it: a string as it is (no quotes), the rest as JSON writes it. */
function scalar(v: unknown): string {
  if (typeof v === "string") return v;
  if (v === null || v === undefined) return "null";
  return String(v);
}

function entries(v: unknown, depth: number, out: string[]): void {
  const pad = "  ".repeat(depth);
  if (Array.isArray(v)) {
    v.forEach((item, i) => {
      if (i > 0 && (isMap(item) || Array.isArray(item))) out.push("");
      member(null, item, depth, out);
    });
  } else if (isMap(v)) {
    for (const [k, item] of Object.entries(v)) member(k, item, depth, out);
  } else out.push(...scalar(v).split("\n").map((l) => pad + l));
}

/** A list of `{Key, Value}` structures (a resource's tags) is a map to a person: `owner = orders`. */
function pairs(v: unknown): Record<string, unknown> | null {
  if (!Array.isArray(v) || v.length === 0) return null;
  const out: Record<string, unknown> = {};
  for (const item of v) {
    if (!isMap(item)) return null;
    const keys = Object.keys(item).sort().join();
    const [k, val] = keys === "Key,Value" ? [item.Key, item.Value] : keys === "key,value" ? [item.key, item.value] : [];
    if (typeof k !== "string" || typeof val !== "string") return null;
    out[k] = val;
  }
  return out;
}

function member(name: string | null, v: unknown, depth: number, out: string[]): void {
  const pad = "  ".repeat(depth);
  const asMap = pairs(v);
  if (asMap) return member(name, asMap, depth, out);
  const nested = Array.isArray(v) || isMap(v);
  if (nested && (Array.isArray(v) ? v.length : Object.keys(v as object).length) === 0) {
    out.push(name === null ? `${pad}${Array.isArray(v) ? "[]" : "{}"}` : `${pad}${name} = ${Array.isArray(v) ? "[]" : "{}"}`);
  } else if (nested) {
    if (name !== null) out.push(pad + name);
    entries(v, name === null ? depth : depth + 1, out);
  } else {
    const text = scalar(v);
    if (name === null) out.push(...text.split("\n").map((l) => pad + l));
    else if (!text.includes("\n") && pad.length + name.length + 3 + text.length <= OUTLINE_WIDTH) out.push(`${pad}${name} = ${text}`);
    else {
      out.push(pad + name);
      out.push(...text.split("\n").map((l) => `${pad}  ${l}`));
    }
  }
}

/** `operation` over `input` as the outline: `CreateTopic`, then `  Name = deliveries`, and a nested member's name over its members. */
export function outlineText(operation: string, input: unknown, service?: string): string {
  const out = [`${service ? `${service} ` : ""}${operation}`];
  if (isMap(input) || Array.isArray(input)) entries(input, 1, out);
  else if (input !== undefined) out.push(`  ${scalar(input)}`);
  return out.join("\n");
}

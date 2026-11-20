// DynamoDB's typed JSON (`{"S": "ord-1"}`, `{"N": "19.25"}`) and plain JSON, both ways, and an item checked against its table's
// keys. Plain JSON cannot say a set, a binary, or a number past what a JSON number holds exactly: an item with one is edited
// in DynamoDB JSON, and the page says why.

export type AttributeValue =
  | { S: string }
  | { N: string }
  | { B: string }
  | { BOOL: boolean }
  | { NULL: true }
  | { SS: string[] }
  | { NS: string[] }
  | { BS: string[] }
  | { L: AttributeValue[] }
  | { M: Record<string, AttributeValue> };

export type Item = Record<string, AttributeValue>;

/** A key of the table or of an index: its attribute and the type that attribute has. */
export type KeyPart = { name: string; type: "S" | "N" | "B"; role: "HASH" | "RANGE" };

const TYPES = ["S", "N", "B", "BOOL", "NULL", "SS", "NS", "BS", "L", "M"] as const;

/** The one type of an attribute value (`S`). */
export function typeOf(v: AttributeValue): string {
  return Object.keys(v)[0] ?? "";
}

/** Whether a DynamoDB number survives a JSON number: at most 15 significant digits (DynamoDB keeps 38). */
export function exactInJson(n: string): boolean {
  const m = /^[+-]?(\d*)(?:\.(\d*))?(?:[eE][+-]?\d+)?$/.exec(n.trim());
  if (!m || !Number.isFinite(Number(n))) return false;
  const digits = `${m[1] ?? ""}${(m[2] ?? "").replace(/0+$/, "")}`.replace(/^0+/, "");
  return digits.length <= 15;
}

/** Why `v` cannot be written as plain JSON, or null when it can. */
export function plainBlocker(v: AttributeValue): string | null {
  if ("B" in v || "BS" in v) return "it holds a binary value";
  if ("SS" in v || "NS" in v) return "it holds a set";
  if ("N" in v) return exactInJson(v.N) ? null : `the number ${v.N} is more precise than a JSON number holds`;
  if ("L" in v) return v.L.map(plainBlocker).find((b) => b !== null) ?? null;
  if ("M" in v) return Object.values(v.M).map(plainBlocker).find((b) => b !== null) ?? null;
  return null;
}

/** An attribute value as plain JSON. */
export function toPlain(v: AttributeValue): unknown {
  if ("S" in v) return v.S;
  if ("N" in v) return Number(v.N);
  if ("BOOL" in v) return v.BOOL;
  if ("NULL" in v) return null;
  if ("L" in v) return v.L.map(toPlain);
  if ("M" in v) return Object.fromEntries(Object.entries(v.M).map(([k, x]) => [k, toPlain(x)]));
  if ("SS" in v) return v.SS;
  if ("NS" in v) return v.NS.map(Number);
  if ("BS" in v) return v.BS;
  return v.B;
}

/** Plain JSON as an attribute value: a string is `S`, a number `N`, a list `L`, an object `M`. */
export function fromPlain(x: unknown): AttributeValue {
  if (typeof x === "string") return { S: x };
  if (typeof x === "number") return { N: String(x) };
  if (typeof x === "boolean") return { BOOL: x };
  if (x === null) return { NULL: true };
  if (Array.isArray(x)) return { L: x.map(fromPlain) };
  return { M: Object.fromEntries(Object.entries(x as Record<string, unknown>).map(([k, v]) => [k, fromPlain(v)])) };
}

export const itemToPlain = (item: Item) => Object.fromEntries(Object.entries(item).map(([k, v]) => [k, toPlain(v)]));

/** Why `item` cannot be edited as plain JSON, or null. */
export function itemPlainBlocker(item: Item): string | null {
  for (const [k, v] of Object.entries(item)) {
    const b = plainBlocker(v);
    if (b) return `${k}: ${b}`;
  }
  return null;
}

/** Whether `v` is a well-formed attribute value (one known type, its value of the right shape). */
function wellFormed(v: unknown): v is AttributeValue {
  if (typeof v !== "object" || v === null || Array.isArray(v)) return false;
  const keys = Object.keys(v);
  if (keys.length !== 1) return false;
  const [t] = keys as [string];
  const x = (v as Record<string, unknown>)[t];
  switch (t) {
    case "S":
    case "B":
      return typeof x === "string";
    case "N":
      return typeof x === "string" && x.trim() !== "" && !Number.isNaN(Number(x));
    case "BOOL":
      return typeof x === "boolean";
    case "NULL":
      return x === true;
    case "SS":
    case "BS":
      return Array.isArray(x) && x.length > 0 && x.every((s) => typeof s === "string");
    case "NS":
      return Array.isArray(x) && x.length > 0 && x.every((s) => typeof s === "string" && !Number.isNaN(Number(s)));
    case "L":
      return Array.isArray(x) && x.every(wellFormed);
    case "M":
      return typeof x === "object" && x !== null && !Array.isArray(x) && Object.values(x).every(wellFormed);
    default:
      return false;
  }
}

/** The item a text is, in `mode`, checked against the table's `keys`; or the words of what is wrong. */
export function parseItem(text: string, mode: "dynamodb" | "plain", keys: KeyPart[]): { item: Item } | { error: string } {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (e) {
    return { error: `Not JSON: ${(e as Error).message}` };
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return { error: "An item is a JSON object of its attributes." };
  let item: Item;
  if (mode === "plain") {
    item = Object.fromEntries(Object.entries(raw).map(([k, v]) => [k, fromPlain(v)]));
  } else {
    for (const [k, v] of Object.entries(raw)) {
      if (!wellFormed(v)) return { error: `${k} is not a DynamoDB value: write it as one of ${TYPES.map((t) => `{"${t}": …}`).slice(0, 3).join(", ")} and so on.` };
    }
    item = raw as Item;
  }
  for (const k of keys) {
    const v = item[k.name];
    if (!v) return { error: `Every item needs its ${k.role === "HASH" ? "partition" : "sort"} key, ${k.name}.` };
    if (typeOf(v) !== k.type) return { error: `${k.name} is a key of type ${k.type}; this gives it ${typeOf(v)}.` };
  }
  return { item };
}

/** The item written for the editor, in `mode`. */
export function itemText(item: Item, mode: "dynamodb" | "plain"): string {
  return JSON.stringify(mode === "plain" ? itemToPlain(item) : item, null, 2);
}

/** An attribute value in one line for a table cell. */
export function cell(v: AttributeValue | undefined): string {
  if (!v) return "";
  if ("S" in v) return v.S;
  if ("N" in v) return v.N;
  if ("BOOL" in v) return String(v.BOOL);
  if ("NULL" in v) return "null";
  if ("B" in v) return `<binary ${v.B.length} b64>`;
  return JSON.stringify(toPlain(v));
}

/** The key of `item` for `keys` (what GetItem and DeleteItem take). */
export function keyOf(item: Item, keys: KeyPart[]): Item {
  return Object.fromEntries(keys.filter((k) => k.role === "HASH" || k.role === "RANGE").map((k) => [k.name, item[k.name] as AttributeValue]));
}

/** A typed key value from what was typed (`19.25` for an `N`). */
export function keyValue(type: KeyPart["type"], text: string): AttributeValue {
  return type === "N" ? { N: text.trim() } : type === "B" ? { B: text.trim() } : { S: text };
}

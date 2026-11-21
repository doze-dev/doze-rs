// An item as rows of name, type and value (dynamodb__item-create, item-edit): the rows an item makes, and the item the rows make. The
// types are DynamoDB's ten. A String, Number or Binary is its text; a Boolean is true or false; a Null has no value; a set is a JSON
// list of texts; a List or Map is DynamoDB JSON (`[{"S": "x"}]`, `{"a": {"N": "1"}}`).
import { type AttributeValue, type Item, type KeyPart, typeOf, wellFormed } from "./json";

export const ATTRIBUTE_TYPES = ["S", "N", "B", "BOOL", "NULL", "SS", "NS", "BS", "L", "M"] as const;
export type AttributeType = (typeof ATTRIBUTE_TYPES)[number];

export const TYPE_NAMES: Record<AttributeType, string> = { S: "String", N: "Number", B: "Binary", BOOL: "Boolean", NULL: "Null", SS: "String set", NS: "Number set", BS: "Binary set", L: "List", M: "Map" };

export type AttrRow = { id: string; name: string; type: AttributeType; value: string; /** A key attribute: its name and type are the table's. */ key: "HASH" | "RANGE" | null };

let counter = 0;
export const newAttr = (name = "", type: AttributeType = "S", value = "", key: AttrRow["key"] = null): AttrRow => ({ id: `attr${++counter}`, name, type, value, key });

/** What a value is written as in its row. */
export function valueText(v: AttributeValue): string {
  if ("S" in v) return v.S;
  if ("N" in v) return v.N;
  if ("B" in v) return v.B;
  if ("BOOL" in v) return String(v.BOOL);
  if ("NULL" in v) return "";
  if ("SS" in v) return JSON.stringify(v.SS);
  if ("NS" in v) return JSON.stringify(v.NS);
  if ("BS" in v) return JSON.stringify(v.BS);
  if ("L" in v) return JSON.stringify(v.L);
  return JSON.stringify(v.M);
}

/** The rows of an item, its keys first (the table's own order), then the rest in the order the item has them. */
export function rowsOf(item: Item, keys: readonly KeyPart[]): AttrRow[] {
  const named = new Set(keys.map((k) => k.name));
  return [
    ...keys.map((k) => newAttr(k.name, k.type, item[k.name] ? valueText(item[k.name]!) : "", k.role)),
    ...Object.entries(item).filter(([n]) => !named.has(n)).map(([n, v]) => newAttr(n, typeOf(v) as AttributeType, valueText(v))),
  ];
}

/** The rows a new item starts from: its keys, empty. */
export const blankRows = (keys: readonly KeyPart[]): AttrRow[] => keys.map((k) => newAttr(k.name, k.type, "", k.role));

const texts = (s: string): string[] | null => {
  try {
    const v: unknown = JSON.parse(s);
    return Array.isArray(v) && v.length > 0 && v.every((x) => typeof x === "string") ? (v as string[]) : null;
  } catch {
    return null;
  }
};

/** A row's value, or the words of what is wrong with it. */
export function attributeOf(r: AttrRow): { value: AttributeValue } | { error: string } {
  const bad = (m: string) => ({ error: `${r.name || "An attribute"}: ${m}` });
  switch (r.type) {
    case "S":
    case "B":
      return r.key && r.value === "" ? bad("a key cannot be empty.") : { value: r.type === "S" ? { S: r.value } : { B: r.value.trim() } };
    case "N":
      return r.value.trim() !== "" && !Number.isNaN(Number(r.value)) ? { value: { N: r.value.trim() } } : bad("a number is needed.");
    case "BOOL":
      return r.value === "true" || r.value === "false" ? { value: { BOOL: r.value === "true" } } : bad("choose true or false.");
    case "NULL":
      return { value: { NULL: true } };
    case "SS":
    case "BS": {
      const list = texts(r.value);
      return list ? { value: (r.type === "SS" ? { SS: list } : { BS: list }) as AttributeValue } : bad('a set is a JSON list of texts, like ["a", "b"].');
    }
    case "NS": {
      const list = texts(r.value);
      return list && list.every((x) => !Number.isNaN(Number(x))) ? { value: { NS: list } } : bad('a number set is a JSON list of number texts, like ["1", "2.5"].');
    }
    case "L":
    case "M": {
      let parsed: unknown;
      try {
        parsed = JSON.parse(r.value);
      } catch {
        return bad(`${r.type === "L" ? "a list" : "a map"} is DynamoDB JSON.`);
      }
      const v = { [r.type]: parsed };
      return wellFormed(v) ? { value: v } : bad(`${r.type === "L" ? "a list" : "a map"} holds DynamoDB values, like {"S": "x"}.`);
    }
  }
}

/** The item the rows make, or the first thing wrong. A blank row (no name, no value) is left out; a repeated name is refused. */
export function itemOf(rows: readonly AttrRow[]): { item: Item } | { error: string } {
  const item: Item = {};
  for (const r of rows) {
    if (!r.key && r.name.trim() === "" && r.value === "") continue;
    const name = r.name.trim();
    if (name === "") return { error: "Every attribute needs a name." };
    if (name in item) return { error: `${name} is there twice.` };
    const a = attributeOf({ ...r, name });
    if ("error" in a) return a;
    item[name] = a.value;
  }
  return { item };
}

/** An item's key as an address member. The router reads a value that looks like JSON as JSON and drops it from a text member, so the key
 * is written in base64 (URL-safe, no padding). */
export function keyParam(key: Item): string {
  const bytes = new TextEncoder().encode(JSON.stringify(key));
  return btoa(String.fromCharCode(...bytes)).replaceAll("+", "-").replaceAll("/", "_").replaceAll("=", "");
}

/** The key an address names, or null when it is not one. */
export function paramKey(text: string | undefined): Item | null {
  if (!text) return null;
  try {
    const bin = atob(text.replaceAll("-", "+").replaceAll("_", "/"));
    const v: unknown = JSON.parse(new TextDecoder().decode(Uint8Array.from(bin, (c) => c.charCodeAt(0))));
    return typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Item) : null;
  } catch {
    return null;
  }
}

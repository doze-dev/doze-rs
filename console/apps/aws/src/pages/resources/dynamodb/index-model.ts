// A global secondary index made or dropped by UpdateTable (dynamodb__index-create, index-delete): the form, the call, what is wrong with it.
import { NAME_RULE, type KeyType, type Projection, type TableInfo } from "./model";
import { units } from "./create-model";

export type IndexForm = { name: string; pk: string; pkType: KeyType; sk: string; skType: KeyType; projection: Projection; include: string; read: string; write: string };

export const blankIndex = (): IndexForm => ({ name: "", pk: "", pkType: "S", sk: "", skType: "S", projection: "ALL", include: "", read: "5", write: "5" });

/** The attributes an INCLUDE projection names, trimmed and without repeats. */
export const includeList = (text: string): string[] => [...new Set(text.split(/[,\s]+/).map((s) => s.trim()).filter(Boolean))];

/** The attribute names the table's keys and indexes already define, with their types. */
function defined(t: TableInfo): Map<string, KeyType> {
  const m = new Map<string, KeyType>([[t.hash.name, t.hash.type]]);
  if (t.range) m.set(t.range.name, t.range.type);
  for (const i of [...t.global, ...t.local]) {
    m.set(i.hash.name, i.hash.type);
    if (i.range) m.set(i.range.name, i.range.type);
  }
  return m;
}

/** The type an attribute already has in the table's definitions, when it has one: an index key of that name must be of it. */
export const knownType = (t: TableInfo, name: string): KeyType | undefined => defined(t).get(name.trim());

/** The UpdateTable that creates the index. Null until it has a name and a partition key. */
export function createIndexInput(t: TableInfo, f: IndexForm): Record<string, unknown> | null {
  const sk = f.sk.trim();
  if (!f.name || !f.pk.trim()) return null;
  const known = defined(t);
  const keys = [{ name: f.pk.trim(), type: f.pkType }, ...(sk ? [{ name: sk, type: f.skType }] : [])];
  const include = includeList(f.include);
  return {
    TableName: t.name,
    // Only the attributes no key of the table or of an index defines yet.
    AttributeDefinitions: keys.filter((k) => !known.has(k.name)).map((k) => ({ AttributeName: k.name, AttributeType: k.type })),
    GlobalSecondaryIndexUpdates: [
      {
        Create: {
          IndexName: f.name,
          KeySchema: [{ AttributeName: f.pk.trim(), KeyType: "HASH" }, ...(sk ? [{ AttributeName: sk, KeyType: "RANGE" }] : [])],
          Projection: { ProjectionType: f.projection, ...(f.projection === "INCLUDE" && include.length > 0 ? { NonKeyAttributes: include } : {}) },
          ...(t.mode === "PROVISIONED" ? { ProvisionedThroughput: { ReadCapacityUnits: units(f.read) ?? 5, WriteCapacityUnits: units(f.write) ?? 5 } } : {}),
        },
      },
    ],
  };
}

/** The UpdateTable that drops an index. */
export const deleteIndexInput = (table: string, index: string): Record<string, unknown> => ({ TableName: table, GlobalSecondaryIndexUpdates: [{ Delete: { IndexName: index } }] });

export type IndexProblems = { name?: string; pk?: string; sk?: string; include?: string; read?: string; write?: string };

export function indexProblems(t: TableInfo, f: IndexForm): IndexProblems {
  const p: IndexProblems = {};
  const taken = [...t.global, ...t.local].some((i) => i.name === f.name);
  if (f.name && !NAME_RULE.test(f.name)) p.name = "Use 3 to 255 letters, numbers, underscore, hyphen and period.";
  else if (taken) p.name = `${f.name} is already an index of ${t.name}.`;
  for (const [key, name, type] of [["pk", f.pk.trim(), f.pkType], ["sk", f.sk.trim(), f.skType]] as const) {
    const was = name ? knownType(t, name) : undefined;
    if (was && was !== type) p[key] = `${name} is already defined in ${t.name} as ${was === "S" ? "a String" : was === "N" ? "a Number" : "a Binary"}; an index key of that name must be of that type.`;
  }
  if (f.sk.trim() && f.sk.trim() === f.pk.trim()) p.sk = "The sort key is another attribute than the partition key.";
  if (f.projection === "INCLUDE") {
    const n = includeList(f.include).length;
    if (n < 1 || n > 20) p.include = "Name 1 to 20 attributes.";
  }
  if (t.mode === "PROVISIONED") {
    if (units(f.read) === null) p.read = "ReadCapacityUnits is a whole number, at least 1.";
    if (units(f.write) === null) p.write = "WriteCapacityUnits is a whole number, at least 1.";
  }
  return p;
}

/** Why the button is off, when it is. */
export function indexWhyOff(t: TableInfo, f: IndexForm): string | null {
  if (!f.name) return "Give the index a name.";
  if (!f.pk.trim()) return "Give the index a partition key.";
  const p = indexProblems(t, f);
  return p.name ?? p.pk ?? p.sk ?? p.include ?? p.read ?? p.write ?? null;
}

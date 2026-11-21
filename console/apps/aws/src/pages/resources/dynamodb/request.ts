// A table's key schema and indexes from DescribeTable, and the Scan or Query a page's form makes.
import { type AttributeValue, type KeyPart, keyValue } from "./json";

type KeySchema = { AttributeName: string; KeyType: "HASH" | "RANGE" }[];
export type Described = {
  Table?: {
    KeySchema?: KeySchema;
    AttributeDefinitions?: { AttributeName: string; AttributeType: "S" | "N" | "B" }[];
    GlobalSecondaryIndexes?: { IndexName: string; KeySchema: KeySchema }[];
    LocalSecondaryIndexes?: { IndexName: string; KeySchema: KeySchema }[];
    ItemCount?: number;
  };
};

/** The table (`index` empty) or one of its indexes, and its keys. */
export type Source = { index: string; keys: KeyPart[]; global: boolean };

/** The table and each index, by what DescribeTable said. */
export function sourcesOf(d: Described): Source[] {
  const types = new Map((d.Table?.AttributeDefinitions ?? []).map((a) => [a.AttributeName, a.AttributeType]));
  const keys = (s: KeySchema | undefined): KeyPart[] => (s ?? []).map((k) => ({ name: k.AttributeName, role: k.KeyType, type: types.get(k.AttributeName) ?? "S" }));
  return [
    { index: "", keys: keys(d.Table?.KeySchema), global: false },
    ...(d.Table?.GlobalSecondaryIndexes ?? []).map((i) => ({ index: i.IndexName, keys: keys(i.KeySchema), global: true })),
    ...(d.Table?.LocalSecondaryIndexes ?? []).map((i) => ({ index: i.IndexName, keys: keys(i.KeySchema), global: false })),
  ];
}

export const SORT_OPS = ["=", "<", "<=", ">", ">=", "between", "begins_with"] as const;
export type SortOp = (typeof SORT_OPS)[number];

export type Form = { mode: "scan" | "query"; index: string; pk: string; sortOp: SortOp; sk: string; sk2: string; filter: string; names: string; values: string };

export const START: Form = { mode: "scan", index: "", pk: "", sortOp: "=", sk: "", sk2: "", filter: "", names: "", values: "" };

function object(text: string, what: string): Record<string, unknown> | string {
  if (!text.trim()) return {};
  try {
    const v: unknown = JSON.parse(text);
    return typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : `${what} is a JSON object.`;
  } catch {
    return `${what} is not JSON.`;
  }
}

/** The operation and input the form asks for (without paging), or the words of what is wrong. */
export function request(form: Form, source: Source): { op: "Scan" | "Query"; input: Record<string, unknown> } | string {
  const input: Record<string, unknown> = form.index ? { IndexName: form.index } : {};
  const names = object(form.names, "ExpressionAttributeNames");
  if (typeof names === "string") return names;
  const values = object(form.values, "ExpressionAttributeValues");
  if (typeof values === "string") return values;
  const ownNames: Record<string, string> = {};
  const ownValues: Record<string, AttributeValue> = {};
  if (form.mode === "query") {
    const pk = source.keys.find((k) => k.role === "HASH");
    const sk = source.keys.find((k) => k.role === "RANGE");
    if (!pk) return "This table has no partition key to query by.";
    if (!form.pk.trim()) return `Give the partition key, ${pk.name}.`;
    ownNames["#pk"] = pk.name;
    ownValues[":pk"] = keyValue(pk.type, form.pk);
    let cond = "#pk = :pk";
    if (sk && form.sk.trim()) {
      ownNames["#sk"] = sk.name;
      ownValues[":sk"] = keyValue(sk.type, form.sk);
      if (form.sortOp === "between") {
        if (!form.sk2.trim()) return "Between needs two values.";
        ownValues[":sk2"] = keyValue(sk.type, form.sk2);
        cond += " AND #sk BETWEEN :sk AND :sk2";
      } else if (form.sortOp === "begins_with") {
        if (sk.type === "N") return "begins_with works on a string or binary sort key.";
        cond += " AND begins_with(#sk, :sk)";
      } else {
        cond += ` AND #sk ${form.sortOp} :sk`;
      }
    }
    input.KeyConditionExpression = cond;
  }
  if (form.filter.trim()) input.FilterExpression = form.filter.trim();
  const allNames = { ...names, ...ownNames };
  const allValues = { ...values, ...ownValues };
  if (Object.keys(allNames).length > 0) input.ExpressionAttributeNames = allNames;
  if (Object.keys(allValues).length > 0) input.ExpressionAttributeValues = allValues;
  return { op: form.mode === "query" ? "Query" : "Scan", input };
}

/** What follows the count of items a page read: how to use the ones returned, or that none matched. */
export function afterRead(returned: number): string {
  return returned === 0 ? "No items match." : "Choose an item to see its JSON, edit or delete it.";
}

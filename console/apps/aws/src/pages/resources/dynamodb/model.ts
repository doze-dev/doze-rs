// A table as the pages read it: DescribeTable's answer in the shapes the tabs draw (keys with their types, indexes, capacity,
// encryption, stream), the tables' list rows from the console's columns (crates/aws-console/src/resources/dynamodb.rs), and the
// words the boards say of them. Pure: no calls here.
import { number, plural } from "@doze/ui/lib/format";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { timeMs } from "../model";

export type KeyType = "S" | "N" | "B";
export type KeyAttr = { name: string; type: KeyType };
export type Projection = "ALL" | "KEYS_ONLY" | "INCLUDE";
export type IndexInfo = { name: string; global: boolean; status: string; hash: KeyAttr; range: KeyAttr | null; projection: Projection; include: string[] };
export type Encryption = { kind: "owned" | "aws" | "customer"; key: string | null };
export type Stream = { enabled: boolean; view: string | null; arn: string | null };

type Schema = { AttributeName: string; KeyType: "HASH" | "RANGE" }[];
type Idx = { IndexName: string; KeySchema: Schema; IndexStatus?: string; Projection?: { ProjectionType?: Projection; NonKeyAttributes?: string[] } };
/** The members of DescribeTable's `Table` the pages read. */
export type Described = {
  Table?: {
    TableName?: string;
    TableArn?: string;
    TableStatus?: string;
    KeySchema?: Schema;
    AttributeDefinitions?: { AttributeName: string; AttributeType: KeyType }[];
    GlobalSecondaryIndexes?: Idx[];
    LocalSecondaryIndexes?: Idx[];
    BillingModeSummary?: { BillingMode?: string };
    ProvisionedThroughput?: { ReadCapacityUnits?: number; WriteCapacityUnits?: number };
    ItemCount?: number;
    TableSizeBytes?: number;
    CreationDateTime?: number | string;
    DeletionProtectionEnabled?: boolean;
    StreamSpecification?: { StreamEnabled?: boolean; StreamViewType?: string };
    LatestStreamArn?: string;
    SSEDescription?: { Status?: string; SSEType?: string; KMSMasterKeyArn?: string };
  };
};

export type TableInfo = {
  name: string;
  arn: string;
  status: string;
  hash: KeyAttr;
  range: KeyAttr | null;
  mode: "PAY_PER_REQUEST" | "PROVISIONED";
  read: number | null;
  write: number | null;
  items: number;
  bytes: number;
  createdMs: number | null;
  protect: boolean;
  stream: Stream;
  encryption: Encryption;
  global: IndexInfo[];
  local: IndexInfo[];
};

const keyOf = (types: Map<string, KeyType>, s: Schema | undefined, role: "HASH" | "RANGE"): KeyAttr | null => {
  const k = s?.find((x) => x.KeyType === role);
  return k ? { name: k.AttributeName, type: types.get(k.AttributeName) ?? "S" } : null;
};

/** The table DescribeTable described (null when the answer has none). */
export function tableOf(d: Described | undefined): TableInfo | null {
  const t = d?.Table;
  if (!t?.TableName) return null;
  const types = new Map((t.AttributeDefinitions ?? []).map((a) => [a.AttributeName, a.AttributeType]));
  const index = (global: boolean) => (i: Idx): IndexInfo => ({
    name: i.IndexName,
    global,
    // A local index has no status of its own: it is as active as its table.
    status: i.IndexStatus ?? "ACTIVE",
    hash: keyOf(types, i.KeySchema, "HASH") ?? { name: "?", type: "S" },
    range: keyOf(types, i.KeySchema, "RANGE"),
    projection: i.Projection?.ProjectionType ?? "ALL",
    include: i.Projection?.NonKeyAttributes ?? [],
  });
  const created = t.CreationDateTime === undefined ? null : typeof t.CreationDateTime === "number" ? Math.round(t.CreationDateTime * 1000) : timeMs(t.CreationDateTime);
  const sse = t.SSEDescription;
  return {
    name: t.TableName,
    arn: t.TableArn ?? "",
    status: t.TableStatus ?? "ACTIVE",
    hash: keyOf(types, t.KeySchema, "HASH") ?? { name: "?", type: "S" },
    range: keyOf(types, t.KeySchema, "RANGE"),
    // DynamoDB reports a billing mode summary for an on-demand table; a provisioned one has none.
    mode: t.BillingModeSummary?.BillingMode === "PAY_PER_REQUEST" ? "PAY_PER_REQUEST" : "PROVISIONED",
    read: t.ProvisionedThroughput?.ReadCapacityUnits || null,
    write: t.ProvisionedThroughput?.WriteCapacityUnits || null,
    items: t.ItemCount ?? 0,
    bytes: t.TableSizeBytes ?? 0,
    createdMs: created,
    protect: t.DeletionProtectionEnabled ?? false,
    stream: { enabled: t.StreamSpecification?.StreamEnabled ?? false, view: t.StreamSpecification?.StreamViewType ?? null, arn: t.LatestStreamArn ?? null },
    encryption: sse?.Status === "ENABLED" ? { kind: sse.KMSMasterKeyArn ? "customer" : "aws", key: sse.KMSMasterKeyArn ?? null } : { kind: "owned", key: null },
    global: (t.GlobalSecondaryIndexes ?? []).map(index(true)),
    local: (t.LocalSecondaryIndexes ?? []).map(index(false)),
  };
}

export const TYPE_WORDS: Record<KeyType, string> = { S: "String", N: "Number", B: "Binary" };
export const keyWords = (k: KeyAttr | null): string => (k ? `${k.name} (${TYPE_WORDS[k.type]})` : "–");

export const modeWords = (t: Pick<TableInfo, "mode">): string => (t.mode === "PAY_PER_REQUEST" ? "On-demand" : "Provisioned");
export const unitsWords = (t: Pick<TableInfo, "mode" | "read" | "write">): string | null => (t.mode === "PROVISIONED" && t.read !== null ? `${t.read} read, ${t.write ?? 0} write units` : null);

/** What encryption a table has, in the words of its page. */
export const encryptionWords = (e: Encryption): string => (e.kind === "owned" ? "Owned by Amazon DynamoDB" : e.kind === "aws" ? "AWS managed key" : `Customer managed key ${e.key ?? ""}`.trim());

/** `One global secondary index`, `Two global secondary indexes`. */
const COUNTS = ["No", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten"];
export const indexCountWords = (n: number): string => `${COUNTS[n] ?? number(n)} global secondary ${n === 1 ? "index" : "indexes"}`;

/** Under the name: `On-demand capacity, one global secondary index.` */
export function headerWords(t: TableInfo): string {
  const capacity = t.mode === "PAY_PER_REQUEST" ? "On-demand capacity" : "Provisioned capacity";
  const n = t.global.length;
  return n === 0 ? `${capacity}.` : `${capacity}, ${indexCountWords(n).toLowerCase()}.`;
}

/** A GSI's projection as the page says it: `ALL`, `KEYS_ONLY`, `INCLUDE (a, b)`. */
export const projectionWords = (i: IndexInfo): string => (i.projection === "INCLUDE" ? `INCLUDE (${i.include.join(", ")})` : i.projection);

/** The rows of the list of tables. Cells: Status, Partition key, Sort key, Items, Size, Created, Attributes, Global indexes, Local indexes,
 * Capacity mode, Read units, Write units. */
export type TableRow = { name: string; status: string; hash: KeyAttr; range: KeyAttr | null; items: number; indexes: number; mode: "PAY_PER_REQUEST" | "PROVISIONED"; read: number | null; write: number | null; createdMs: number | null };

const json = <T,>(v: string | null | undefined, fallback: T): T => {
  try {
    return v ? (JSON.parse(v) as T) : fallback;
  } catch {
    return fallback;
  }
};

export function listRows(rows: readonly ResourceRow[]): TableRow[] {
  return rows.map((r) => {
    const c = r.cells;
    const defs = json<{ AttributeName: string; AttributeType: KeyType }[]>(c[6], []);
    const type = (n: string | null | undefined): KeyType => defs.find((a) => a.AttributeName === n)?.AttributeType ?? "S";
    const count = (v: string | null | undefined) => (Number.isFinite(Number(v)) && v ? Number(v) : null);
    return {
      name: r.name,
      status: c[0] ?? "",
      hash: { name: c[1] ?? "?", type: type(c[1]) },
      range: c[2] ? { name: c[2], type: type(c[2]) } : null,
      items: count(c[3]) ?? 0,
      indexes: json<unknown[]>(c[7], []).length + json<unknown[]>(c[8], []).length,
      mode: c[9] === "PAY_PER_REQUEST" ? "PAY_PER_REQUEST" : "PROVISIONED",
      read: count(c[10]),
      write: count(c[11]),
      createdMs: c[5] ? timeMs(c[5]) : null,
    };
  });
}

/** `3 tables in us-east-1 hold about 4,462 items. deliveries is ticked. On AWS an item count is refreshed about every six hours; doze-aws counts live.` */
export function listSentence(rows: readonly TableRow[], region: string, ticked: string | null): string {
  const items = rows.reduce((a, r) => a + r.items, 0);
  const head = `${plural(rows.length, "table")} in ${region} ${rows.length === 1 ? "holds" : "hold"} ${items === rows.length ? "" : "about "}${plural(items, "item")}.`.replace("about 1 item", "1 item");
  return `${head}${ticked ? ` ${ticked} is ticked.` : ""} On AWS an item count is refreshed about every six hours; doze-aws counts live.`;
}

/** The rules of a table's name, as CreateTable states them: 3 to 255 of letters, digits, underscore, hyphen and period. */
export const NAME_RULE = /^[A-Za-z0-9_.-]{3,255}$/;
export function nameProblem(name: string): string | null {
  if (!name) return null;
  if (name.length < 3) return "A table name has at least 3 characters.";
  if (name.length > 255) return "A table name has at most 255 characters.";
  return NAME_RULE.test(name) ? null : "Use letters, numbers, underscore, hyphen and period only.";
}

/** The words of a deletion, said once the table is gone. */
export const deletedWords = (name: string, items: number | null): string => (items === null ? `${name} is gone.` : `${name} and its ${plural(items, "item")} are gone.`);

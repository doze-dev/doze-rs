import { describe, expect, test } from "bun:test";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { type Described, headerWords, indexCountWords, listRows, listSentence, nameProblem, tableOf } from "./model";
import { request, START, sourcesOf } from "./request";

const DESCRIBED: Described = {
  Table: {
    TableName: "orders-table",
    TableArn: "arn:aws:dynamodb:us-east-1:111122223333:table/orders-table",
    TableStatus: "ACTIVE",
    KeySchema: [{ AttributeName: "order_id", KeyType: "HASH" }, { AttributeName: "created_at", KeyType: "RANGE" }],
    AttributeDefinitions: [{ AttributeName: "order_id", AttributeType: "S" }, { AttributeName: "created_at", AttributeType: "S" }, { AttributeName: "customer_id", AttributeType: "N" }],
    GlobalSecondaryIndexes: [{ IndexName: "by-customer", IndexStatus: "ACTIVE", KeySchema: [{ AttributeName: "customer_id", KeyType: "HASH" }, { AttributeName: "created_at", KeyType: "RANGE" }], Projection: { ProjectionType: "INCLUDE", NonKeyAttributes: ["total"] } }],
    BillingModeSummary: { BillingMode: "PAY_PER_REQUEST" },
    ItemCount: 1204,
    CreationDateTime: 1_794_000_000.5,
  },
};

describe("a table as DescribeTable says it", () => {
  test("keys carry the types the attribute definitions give them, and the indexes their projection", () => {
    const t = tableOf(DESCRIBED)!;
    expect(t.hash).toEqual({ name: "order_id", type: "S" });
    expect(t.range?.name).toBe("created_at");
    expect(t.global[0]).toMatchObject({ name: "by-customer", hash: { name: "customer_id", type: "N" }, projection: "INCLUDE", include: ["total"], status: "ACTIVE" });
    expect(t.createdMs).toBe(1_794_000_000_500);
    expect(t.mode).toBe("PAY_PER_REQUEST");
    expect(t.encryption.kind).toBe("owned");
  });

  test("a table with no billing mode summary is provisioned, with its units", () => {
    const t = tableOf({ Table: { ...DESCRIBED.Table, BillingModeSummary: undefined, ProvisionedThroughput: { ReadCapacityUnits: 5, WriteCapacityUnits: 7 } } })!;
    expect([t.mode, t.read, t.write]).toEqual(["PROVISIONED", 5, 7]);
  });

  test("an SSE description with a key is a customer managed key, without one the AWS managed key", () => {
    expect(tableOf({ Table: { ...DESCRIBED.Table, SSEDescription: { Status: "ENABLED", SSEType: "KMS" } } })!.encryption.kind).toBe("aws");
    expect(tableOf({ Table: { ...DESCRIBED.Table, SSEDescription: { Status: "ENABLED", SSEType: "KMS", KMSMasterKeyArn: "arn:aws:kms:x" } } })!.encryption).toEqual({ kind: "customer", key: "arn:aws:kms:x" });
  });

  test("the header says the capacity and the indexes", () => {
    expect(headerWords(tableOf(DESCRIBED)!)).toBe("On-demand capacity, one global secondary index.");
    expect(indexCountWords(2)).toBe("Two global secondary indexes");
    expect(headerWords({ ...tableOf(DESCRIBED)!, global: [], mode: "PROVISIONED" })).toBe("Provisioned capacity.");
  });

  test("the table and each index are sources of a query, with their own keys", () => {
    const s = sourcesOf(tableOf(DESCRIBED)!);
    expect(s.map((x) => x.index)).toEqual(["", "by-customer"]);
    expect(s[1]!.keys).toEqual([{ name: "customer_id", role: "HASH", type: "N" }, { name: "created_at", role: "RANGE", type: "S" }]);
  });
});

describe("the list of tables", () => {
  const row = (name: string, cells: (string | null)[]): ResourceRow => ({ name, label: name, id: name, cells, link: null });
  const defs = JSON.stringify([{ AttributeName: "id", AttributeType: "N" }]);
  const rows = listRows([
    row("deliveries", ["ACTIVE", "id", null, "318", "0", null, defs, "[]", "[]", null, "5", "5"]),
    row("orders", ["ACTIVE", "id", "ts", "4", "0", null, defs, '[{"IndexName":"a"}]', '[{"IndexName":"b"}]', "PAY_PER_REQUEST", null, null]),
  ]);

  test("rows read the key types, the indexes and the capacity from the columns", () => {
    expect(rows[0]).toMatchObject({ hash: { name: "id", type: "N" }, range: null, items: 318, indexes: 0, mode: "PROVISIONED", read: 5 });
    expect(rows[1]).toMatchObject({ range: { name: "ts", type: "S" }, indexes: 2, mode: "PAY_PER_REQUEST", read: null });
  });

  test("the sentence counts the items and names the ticked table", () => {
    expect(listSentence(rows, "us-east-1", "deliveries")).toBe("2 tables in us-east-1 hold about 322 items. deliveries is ticked. On AWS an item count is refreshed about every six hours; doze-aws counts live.");
    expect(listSentence(rows.slice(0, 1), "us-east-1", null)).toStartWith("1 table in us-east-1 holds about 318 items.");
  });
});

describe("a table's name", () => {
  test("is 3 to 255 of letters, digits, underscore, hyphen and period", () => {
    expect(nameProblem("")).toBeNull();
    expect(nameProblem("ab")).toContain("at least 3");
    expect(nameProblem("orders table")).toContain("letters");
    expect(nameProblem("orders-table.v2_x")).toBeNull();
    expect(nameProblem("a".repeat(256))).toContain("at most 255");
  });
});

describe("the call an explore form makes", () => {
  const source = sourcesOf(tableOf(DESCRIBED)!)[1]!;
  test("a query is by key alone: a filter typed for a scan is not sent with it", () => {
    const r = request({ ...START, mode: "query", index: "by-customer", pk: "7", filter: "#s = :s", names: '{"#s": "status"}', values: '{":s": {"S": "x"}}' }, source);
    expect(r).toMatchObject({ op: "Query", input: { IndexName: "by-customer", KeyConditionExpression: "#pk = :pk", ExpressionAttributeValues: { ":pk": { N: "7" } } } });
    expect(JSON.stringify(r)).not.toContain("FilterExpression");
    expect(JSON.stringify(r)).not.toContain('":s"');
  });
  test("a scan carries its filter and its names and values", () => {
    const r = request({ ...START, filter: "#s = :s", names: '{"#s": "status"}', values: '{":s": {"S": "x"}}' }, source);
    expect(r).toMatchObject({ op: "Scan", input: { FilterExpression: "#s = :s", ExpressionAttributeNames: { "#s": "status" } } });
  });
});

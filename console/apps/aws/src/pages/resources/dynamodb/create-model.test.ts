import { describe, expect, test } from "bun:test";
import { newRow } from "@doze/ui/components/app/key-values";
import { blankForm, createInput, problems, units, whyOff } from "./create-model";
import { blankIndex, createIndexInput, deleteIndexInput, includeList, indexProblems, indexWhyOff } from "./index-model";
import { type Described, tableOf } from "./model";

describe("Create table", () => {
  const f = { ...blankForm(), name: "stock-moves", pk: "sku", sk: "moved_at" };
  test("the default settings are on-demand with the keys given", () => {
    expect(createInput(f)).toEqual({
      TableName: "stock-moves",
      AttributeDefinitions: [{ AttributeName: "sku", AttributeType: "S" }, { AttributeName: "moved_at", AttributeType: "S" }],
      KeySchema: [{ AttributeName: "sku", KeyType: "HASH" }, { AttributeName: "moved_at", KeyType: "RANGE" }],
      BillingMode: "PAY_PER_REQUEST",
    });
    expect(createInput({ ...f, sk: "" })!.KeySchema).toEqual([{ AttributeName: "sku", KeyType: "HASH" }]);
    expect(createInput({ ...f, pk: "" })).toBeNull();
  });
  test("customized settings add the capacity, the key and the tags; default settings leave them out", () => {
    const custom = { ...f, custom: true, mode: "PROVISIONED" as const, read: "20", write: "10", encryption: "customer" as const, key: "alias/larder-app", tags: [newRow("app", "larder"), newRow()] };
    const input = createInput(custom)!;
    expect(input.BillingMode).toBe("PROVISIONED");
    expect(input.ProvisionedThroughput).toEqual({ ReadCapacityUnits: 20, WriteCapacityUnits: 10 });
    expect(input.SSESpecification).toEqual({ Enabled: true, SSEType: "KMS", KMSMasterKeyId: "alias/larder-app" });
    expect(input.Tags).toEqual([{ Key: "app", Value: "larder" }]);
    expect(createInput({ ...custom, custom: false })).toEqual(createInput(f));
  });
  test("what is wrong is said, and the button says why it is off", () => {
    expect(problems({ ...f, name: "a b" }).name).toContain("letters");
    expect(problems({ ...f, sk: "sku" }).sk).toContain("another attribute");
    expect(problems({ ...f, custom: true, mode: "PROVISIONED", read: "0" }).read).toContain("at least 1");
    expect(problems({ ...f, custom: true, encryption: "customer" }).key).toBe("Choose the key.");
    expect(whyOff(blankForm())).toBe("Give the table a name.");
    expect(whyOff({ ...blankForm(), name: "abc" })).toBe("Give the table a partition key.");
    expect(whyOff(f)).toBeNull();
    expect(units("5")).toBe(5);
    expect(units("1.5")).toBeNull();
  });
});

describe("Create global secondary index", () => {
  const table = tableOf({
    Table: {
      TableName: "orders-table",
      KeySchema: [{ AttributeName: "order_id", KeyType: "HASH" }, { AttributeName: "created_at", KeyType: "RANGE" }],
      AttributeDefinitions: [{ AttributeName: "order_id", AttributeType: "S" }, { AttributeName: "created_at", AttributeType: "S" }, { AttributeName: "customer_id", AttributeType: "S" }],
      GlobalSecondaryIndexes: [{ IndexName: "by-customer", KeySchema: [{ AttributeName: "customer_id", KeyType: "HASH" }] }],
      BillingModeSummary: { BillingMode: "PAY_PER_REQUEST" },
    },
  } as Described)!;
  const f = { ...blankIndex(), name: "by-status", pk: "status", sk: "created_at" };
  test("only the attributes no key defines yet join the table's definitions", () => {
    const input = createIndexInput(table, f)!;
    expect(input.AttributeDefinitions).toEqual([{ AttributeName: "status", AttributeType: "S" }]);
    expect(input.GlobalSecondaryIndexUpdates).toEqual([{ Create: { IndexName: "by-status", KeySchema: [{ AttributeName: "status", KeyType: "HASH" }, { AttributeName: "created_at", KeyType: "RANGE" }], Projection: { ProjectionType: "ALL" } } }]);
  });
  test("INCLUDE names its attributes; a provisioned table gives the index its own capacity", () => {
    const input = createIndexInput({ ...table, mode: "PROVISIONED" }, { ...f, projection: "INCLUDE", include: "total, status total", read: "3", write: "4" })!;
    const create = (input.GlobalSecondaryIndexUpdates as { Create: Record<string, unknown> }[])[0]!.Create;
    expect(create.Projection).toEqual({ ProjectionType: "INCLUDE", NonKeyAttributes: ["total", "status"] });
    expect(create.ProvisionedThroughput).toEqual({ ReadCapacityUnits: 3, WriteCapacityUnits: 4 });
    expect(includeList("a, b  a")).toEqual(["a", "b"]);
  });
  test("a name in use, a key of another type than the table defines, and an empty INCLUDE are refused", () => {
    expect(indexProblems(table, { ...f, name: "by-customer" }).name).toContain("already an index");
    expect(indexProblems(table, { ...f, pk: "customer_id", pkType: "N" }).pk).toContain("a String");
    expect(indexProblems(table, { ...f, projection: "INCLUDE" }).include).toBe("Name 1 to 20 attributes.");
    expect(indexWhyOff(table, blankIndex())).toBe("Give the index a name.");
    expect(indexWhyOff(table, f)).toBeNull();
  });
  test("a drop names the index and nothing else", () => {
    expect(deleteIndexInput("orders-table", "by-customer")).toEqual({ TableName: "orders-table", GlobalSecondaryIndexUpdates: [{ Delete: { IndexName: "by-customer" } }] });
  });
});

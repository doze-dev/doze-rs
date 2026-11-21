import { describe, expect, test } from "bun:test";
import { partiqlWords, tableOf, whereNames } from "./partiql-words";

const UPDATE = `UPDATE "orders-table" SET status = 'cancelled' WHERE order_id = 'ord-1' AND created_at = '2026-11-08T08:01:00Z'`;

describe("a refused PartiQL statement", () => {
  test("names its table and the attributes it compares", () => {
    expect(tableOf(UPDATE)).toBe("orders-table");
    expect(tableOf(`DELETE FROM "deliveries" WHERE id = 'x'`)).toBe("deliveries");
    expect(tableOf(`INSERT INTO "t" VALUE {'a': 1}`)).toBe("t");
    expect(tableOf(`SELECT a, b FROM "t" WHERE a = 1`)).toBe("t");
    expect(tableOf("nonsense")).toBeNull();
    expect(whereNames(UPDATE)).toEqual(["order_id", "created_at"]);
    expect(whereNames(`SELECT * FROM "t"`)).toEqual([]);
  });
  test("a write that found no item says which key was not there", () => {
    expect(partiqlWords("ConditionalCheckFailedException", UPDATE)).toBe("No item in orders-table has that order_id and created_at, so there was nothing to update.");
    expect(partiqlWords("ConditionalCheckFailedException", `DELETE FROM "t" WHERE id = 'x'`)).toBe("No item in t has that id, so there was nothing to delete.");
  });
  test("the other common refusals say what to do", () => {
    expect(partiqlWords("DuplicateItemException", `INSERT INTO "t" VALUE {'a': 1}`)).toContain("t already has an item with that key");
    expect(partiqlWords("ResourceNotFoundException", `SELECT * FROM "ghost"`)).toContain("ghost is not a table");
    expect(partiqlWords("ValidationException", "SELEC")).toContain("double quotes");
    expect(partiqlWords("ThrottlingException", UPDATE)).toBeNull();
  });
});

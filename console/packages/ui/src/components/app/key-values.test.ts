import { describe, expect, test } from "bun:test";
import { filled, newRow, pairsOf, rowsFrom, rowsValid, validateRows } from "./key-values";

describe("key-value rows", () => {
  test("rows have ids of their own", () => expect(newRow().id).not.toBe(newRow().id));
  test("an empty key is named, a filled row is clean, a blank row is not wrong", () => {
    const [a, b, blank] = [newRow("", "x"), newRow("env", "dev"), newRow()];
    const e = validateRows([a, b, blank]);
    expect(e.get(a.id)?.key).toBe("Enter a key.");
    expect(e.get(a.id)?.live).toBeUndefined();
    expect(e.has(b.id)).toBe(false);
    expect(e.has(blank.id)).toBe(false);
  });
  test("a duplicate key is named on both rows; keys are case-sensitive unless said otherwise", () => {
    const rows = [newRow("env", "a"), newRow("env", "b"), newRow("Env", "c")];
    const e = validateRows(rows);
    expect(e.get(rows[0]!.id)?.key).toBe("Another row uses this key.");
    expect(e.get(rows[1]!.id)?.key).toBe("Another row uses this key.");
    expect(e.get(rows[1]!.id)?.live).toBe(true);
    expect(e.has(rows[2]!.id)).toBe(false);
    expect(validateRows(rows, { caseSensitive: false }).get(rows[2]!.id)?.key).toBe("Another row uses this key.");
  });
  test("limits", () => {
    const r = newRow("k".repeat(5), "v".repeat(5));
    expect(validateRows([r], { maxKey: 4 }).get(r.id)?.key).toBe("A key is at most 4 characters.");
    expect(validateRows([r], { maxValue: 4 }).get(r.id)?.value).toBe("A value is at most 4 characters.");
    expect(validateRows([newRow("a", "")], { emptyValue: false }).size).toBe(1);
  });
  test("valid means no problem and no more rows than allowed", () => {
    expect(rowsValid([newRow("a", "1")])).toBe(true);
    expect(rowsValid([newRow("a", "1"), newRow("b", "2")], { maxRows: 1 })).toBe(false);
    expect(rowsValid([])).toBe(true);
  });
  test("conversion and dropping blank rows", () => {
    const rows = rowsFrom([{ key: "a", value: "1" }]);
    expect(pairsOf(rows)).toEqual([{ key: "a", value: "1" }]);
    expect(filled([newRow(), newRow("a", ""), newRow("", "x")]).length).toBe(2);
  });
});

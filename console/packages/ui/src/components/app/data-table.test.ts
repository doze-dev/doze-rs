import { describe, expect, test } from "bun:test";
import { boxHeight, columnWidths } from "./data-table";

describe("columnWidths", () => {
  const cols = [
    { size: 200, min: 120, header: 60 },
    { size: 100, header: 60 },
    { header: 60, min: 300 },
    { size: 300, min: 100, header: 120 },
  ];

  test("a table that fits keeps every size, and the rest column its least", () => {
    expect(columnWidths(cols, 2000)).toEqual({ widths: [200, 100, undefined, 300], minWidth: 900 });
    expect(columnWidths(cols, 0)).toEqual({ widths: [200, 100, undefined, 300], minWidth: 900 });
  });

  test("in a narrow box the columns with a min give up width in proportion to what they can spare", () => {
    // 100 too wide; the first can spare 80, the last 180 (its header is 120): they give 30 and 69 (floored).
    expect(columnWidths(cols, 800)).toEqual({ widths: [169, 100, undefined, 230], minWidth: 799 });
  });

  test("never below a min or a header: past that the table scrolls", () => {
    expect(columnWidths(cols, 400)).toEqual({ widths: [120, 100, undefined, 120], minWidth: 640 });
  });

  test("a table with no rest column has no least width of its own", () => {
    expect(columnWidths([{ size: 100, min: 50, header: 10 }], 80).widths).toEqual([80]);
    expect(columnWidths([{ size: 100, header: 10 }], 80).minWidth).toBeUndefined();
  });
});

describe("boxHeight", () => {
  test("a box of N rows ends where the Nth row ends: the header, N rows and the two borders", () => {
    expect(boxHeight(10, 36)).toBe(40 + 360 + 2);
    expect(boxHeight(10, 40)).toBe(40 + 400 + 2);
  });

  test("at least one row, and a whole number of them", () => {
    expect(boxHeight(0, 36)).toBe(40 + 36 + 2);
    expect(boxHeight(2.7, 36)).toBe(40 + 72 + 2);
  });
});

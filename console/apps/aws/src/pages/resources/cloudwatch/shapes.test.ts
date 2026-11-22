import { describe, expect, test } from "bun:test";
import { slicePath, slices, sparkPath } from "./shapes";

describe("a number widget's sparkline", () => {
  test("runs from the left edge to the right, the lowest value at the bottom and the highest at the top", () => {
    expect(sparkPath([{ t: 0, v: 0 }, { t: 10, v: 10 }], 100, 20, 2)).toBe("M2.0 18.0L98.0 2.0");
  });
  test("a gap ends the run, a flat line is in the middle, and fewer than two values is no line", () => {
    expect(sparkPath([{ t: 0, v: 1 }, { t: 5, v: null }, { t: 10, v: 3 }, { t: 15, v: 4 }], 100, 20, 0)).toContain("M");
    expect(sparkPath([{ t: 0, v: 1 }, { t: 5, v: null }, { t: 10, v: 3 }, { t: 15, v: 4 }], 100, 20, 0).match(/M/g)).toHaveLength(2);
    expect(sparkPath([{ t: 0, v: 5 }, { t: 10, v: 5 }], 100, 20, 2)).toBe("M2.0 10.0L98.0 10.0");
    expect(sparkPath([{ t: 0, v: 5 }], 100, 20)).toBe("");
    expect(sparkPath([{ t: 0, v: null }, { t: 1, v: null }], 100, 20)).toBe("");
  });
});

describe("a pie widget", () => {
  test("shares are turns that add up to one, in order", () => {
    expect(slices([1, 3])).toEqual([{ from: 0, to: 0.25 }, { from: 0.25, to: 1 }]);
    expect(slices([null, 2, -1])).toEqual([{ from: 0, to: 0 }, { from: 0, to: 1 }, { from: 1, to: 1 }]);
    expect(slices([0, 0])).toEqual([]);
    expect(slices([])).toEqual([]);
  });
  test("a slice is an arc from the centre; more than half a turn takes the large arc; a whole turn is two arcs", () => {
    expect(slicePath(50, 50, 40, 0, 0.25)).toBe("M50 50L50.00 10.00A40 40 0 0 1 90.00 50.00Z");
    expect(slicePath(50, 50, 40, 0, 0.75)).toContain("0 1 1");
    expect(slicePath(50, 50, 40, 0, 1).match(/A/g)).toHaveLength(2);
  });
});

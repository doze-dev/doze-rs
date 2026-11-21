import { describe, expect, test } from "bun:test";
import { areaPath, axisNumber, linear, linePath, nearest, niceTicks, tickLabel, timeStep, timeTicks, valueExtent } from "./chart-scale";

const H = 3_600_000;
const T0 = Date.UTC(2026, 10, 8, 5, 20); // 05:20 UTC

describe("scales", () => {
  test("linear maps the domain onto the range, inverted for a y axis", () => {
    const y = linear(0, 40, 200, 0);
    expect(y(0)).toBe(200);
    expect(y(40)).toBe(0);
    expect(y(10)).toBe(150);
  });
  test("a one-value domain maps to the middle", () => expect(linear(5, 5, 0, 100)(5)).toBe(50));
});

describe("niceTicks", () => {
  test("round steps that cover the data", () => {
    expect(niceTicks(0, 37, 5)).toEqual([0, 10, 20, 30, 40]);
    expect(niceTicks(0, 1, 5)).toEqual([0, 0.25, 0.5, 0.75, 1].map((x) => x) as number[]);
  });
  test("a flat line still gets a grid", () => {
    const t = niceTicks(3, 3);
    expect(t[0]).toBeLessThanOrEqual(0);
    expect(t[t.length - 1]).toBeGreaterThanOrEqual(3);
    expect(niceTicks(0, 0).length).toBeGreaterThan(1);
  });
  test("not numbers: a safe default", () => expect(niceTicks(Number.NaN, 1)).toEqual([0, 1]));
});

describe("time axis", () => {
  test("the step is the smallest that gives few enough ticks", () => {
    expect(timeStep(3 * H, 6)).toBe(30 * 60_000);
    expect(timeStep(24 * H, 6)).toBe(6 * H);
    expect(timeStep(1000 * 24 * H, 6)).toBe(7 * 24 * H);
  });
  test("ticks sit on UTC multiples of the step, inside the range", () => {
    const { step, ticks } = timeTicks(T0, T0 + 3 * H, 6);
    expect(step).toBe(30 * 60_000);
    expect(ticks.every((t) => t % step === 0 && t >= T0 && t <= T0 + 3 * H)).toBe(true);
    expect(tickLabel(ticks[0]!, step)).toBe("05:30");
  });
  test("a day is named where it starts, and a day-long step names days", () => {
    expect(tickLabel(Date.UTC(2026, 10, 9, 0, 0), H)).toBe("Nov 9");
    expect(tickLabel(Date.UTC(2026, 10, 9, 6, 0), 24 * H)).toBe("Nov 9");
  });
});

describe("nearest", () => {
  const pts = [10, 20, 30, 40].map((t) => ({ t, v: t }));
  test("the closest point in time", () => {
    expect(nearest(pts, 11)).toBe(0);
    expect(nearest(pts, 26)).toBe(2);
    expect(nearest(pts, 999)).toBe(3);
    expect(nearest(pts, -5)).toBe(0);
  });
  test("a tie goes to the earlier point; none gives -1", () => {
    expect(nearest(pts, 25)).toBe(1);
    expect(nearest([], 3)).toBe(-1);
  });
});

describe("paths", () => {
  const x = (t: number) => t;
  const y = (v: number) => 100 - v;
  test("a null breaks the line", () => {
    const d = linePath([{ t: 0, v: 1 }, { t: 1, v: 2 }, { t: 2, v: null }, { t: 3, v: 4 }], x, y);
    expect(d).toBe("M0.0 99.0L1.0 98.0M3.0 96.0");
  });
  test("an area per run, closed to the base; a lone point has none", () => {
    expect(areaPath([{ t: 0, v: 1 }, { t: 1, v: 2 }], x, y, 100)).toBe("M0.0 100L0.0 99.0L1.0 98.0L1.0 100Z");
    expect(areaPath([{ t: 0, v: 1 }], x, y, 100)).toBe("");
  });
});

describe("extent and numbers", () => {
  test("counts start at zero; values that go negative do not", () => {
    expect(valueExtent([{ points: [{ t: 0, v: 5 }, { t: 1, v: 9 }] }])).toEqual({ min: 0, max: 9 });
    expect(valueExtent([{ points: [{ t: 0, v: -5 }, { t: 1, v: 9 }] }])).toEqual({ min: -5, max: 9 });
    expect(valueExtent([{ points: [{ t: 0, v: null }] }])).toBeNull();
    expect(valueExtent([{ points: [{ t: 0, v: 5 }] }], [80])).toEqual({ min: 0, max: 80 });
  });
  test("axis numbers", () => {
    expect(axisNumber(1204)).toBe("1,204");
    expect(axisNumber(12.3456)).toBe("12.3");
    expect(axisNumber(32_000)).toBe("32k");
    expect(axisNumber(1_500_000)).toBe("1.5M");
  });
});

import { isolatedPoints } from "./chart-scale";
describe("isolatedPoints", () => {
  test("a value between gaps, or alone, has no line and is a dot", () => {
    expect(isolatedPoints([{ t: 0, v: 1 }, { t: 1, v: 2 }, { t: 2, v: null }, { t: 3, v: 4 }, { t: 4, v: null }])).toEqual([{ t: 3, v: 4 }]);
    expect(isolatedPoints([{ t: 0, v: 5 }])).toEqual([{ t: 0, v: 5 }]);
    expect(isolatedPoints([{ t: 0, v: 1 }, { t: 1, v: 2 }])).toEqual([]);
  });
});

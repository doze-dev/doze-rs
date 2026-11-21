import { describe, expect, test } from "bun:test";
import { HOUR, lastHour, linePath, mean, milliseconds, total } from "./monitor-model";

const pts = [{ t: 1000, v: 2 }, { t: 2000, v: null }, { t: 3000, v: 4 }];

describe("the figures of the Monitor tab", () => {
  test("sums, means and the last hour", () => {
    expect(total(pts)).toBe(6);
    expect(mean(pts)).toBe(3);
    expect(total([])).toBeNull();
    expect(lastHour([{ t: 10, v: 5 }, { t: 3 * HOUR, v: 7 }], 3 * HOUR + 1)).toBe(7);
  });
  test("durations in the unit that reads best", () => {
    expect(milliseconds(41.4)).toBe("41 ms");
    expect(milliseconds(0.584)).toBe("0.58 ms");
    expect(milliseconds(1250)).toBe("1.3 s");
    expect(milliseconds(12_000)).toBe("12 s");
  });
  test("a line along the points, flat for none", () => {
    expect(linePath([], 0, 10, 100, 40)).toBe("M0 37 L100 37");
    expect(linePath([{ t: 0, v: 1 }, { t: 10, v: 1 }], 0, 10, 100, 40)).toBe("M0.0 3.0 L100.0 3.0");
  });
});

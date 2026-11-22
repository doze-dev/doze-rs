import { describe, expect, test } from "bun:test";
import { DEFAULT_ZONE, bounds, clock, day, minutes, rangeWords, spanLine, spanWords } from "./range";

const T = Date.UTC(2026, 10, 8, 8, 14, 3, 211);

describe("a range", () => {
  test("a relative one counts back from now", () => expect(bounds({ mode: "relative", id: "1h" }, T)).toEqual({ startTime: T - 3_600_000, endTime: T }));
  test("all time has no bounds", () => expect(bounds({ mode: "relative", id: "all" }, T)).toEqual({}));
  test("an absolute one has the ends it was given", () => {
    expect(bounds({ mode: "absolute", from: 1, to: 2 }, T)).toEqual({ startTime: 1, endTime: 2 });
    expect(bounds({ mode: "absolute", from: null, to: 2 }, T)).toEqual({ endTime: 2 });
  });
  test("is said as a label and as a phrase", () => {
    expect(rangeWords({ mode: "relative", id: "1h" })).toBe("Last 1 hour");
    expect(spanWords({ mode: "relative", id: "1h" })).toBe("the last hour");
    expect(spanWords({ mode: "relative", id: "15m" })).toBe("the last 15 minutes");
    expect(spanWords({ mode: "relative", id: "all" })).toBe("the whole group");
  });
});

describe("times in a zone", () => {
  test("a stream's events open in the person's own zone", () => expect(DEFAULT_ZONE).toBe("local"));
  test("UTC", () => {
    expect(clock(T, "utc")).toBe("08:14:03.211");
    expect(day(T, "utc")).toBe("2026-11-08");
    expect(minutes(T, "utc")).toBe("2026-11-08 08:14");
  });
  test("the span line gives the end by its time alone on the same day", () => {
    expect(spanLine(5, T - 3_600_000, T, "utc")).toBe("5 events, 2026-11-08 07:14 to 08:14 UTC");
    expect(spanLine(1, T - 86_400_000, T, "utc")).toBe("1 event, 2026-11-07 08:14 to 2026-11-08 08:14 UTC");
  });
});

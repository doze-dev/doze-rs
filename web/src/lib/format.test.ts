import { describe, expect, test } from "bun:test";
import { clockUtc, duration, hhmmUtc, number, plural } from "./format";
import { intervalMs, parseInterval } from "./refresh";
import { parseRecents, pushRecent } from "./recents";

describe("format", () => {
  test("times are UTC and absolute", () => {
    expect(clockUtc(Date.UTC(2026, 10, 13, 4, 5, 9))).toBe("04:05:09");
    expect(hhmmUtc(Date.UTC(2026, 10, 13, 23, 59, 59))).toBe("23:59");
  });
  test("numbers group by three whatever the locale", () => {
    expect(number(0)).toBe("0");
    expect(number(999)).toBe("999");
    expect(number(1000)).toBe("1,000");
    expect(number(1234567)).toBe("1,234,567");
    expect(number(-12345)).toBe("-12,345");
  });
  test("durations name the two largest units", () => {
    expect(duration(42_000)).toBe("42 s");
    expect(duration(125_000)).toBe("2 min");
    expect(duration(2 * 3600_000 + 5 * 60_000)).toBe("2 h 5 min");
    expect(duration(3 * 86400_000 + 4 * 3600_000)).toBe("3 d 4 h");
    expect(duration(-5)).toBe("0 s");
  });
  test("plural", () => {
    expect(plural(1, "topic")).toBe("1 topic");
    expect(plural(1200, "topic")).toBe("1,200 topics");
  });
});

describe("refresh interval", () => {
  test("a stored value is one of the choices, or Off", () => {
    expect(parseInterval("10s")).toBe("10s");
    expect(parseInterval("1m")).toBe("1m");
    expect(parseInterval("2s")).toBe("off");
    expect(parseInterval(null)).toBe("off");
    expect(parseInterval("<script>")).toBe("off");
  });
  test("no interval is under 5 s", () => {
    for (const v of ["off", "5s", "10s", "30s", "1m"] as const) {
      const ms = intervalMs(v);
      expect(ms === 0 || ms >= 5000).toBe(true);
    }
  });
});

describe("recents", () => {
  test("newest first, five at most, no repeats", () => {
    let list = parseRecents(null);
    for (const n of ["a", "b", "c", "d", "e", "f", "c"]) list = pushRecent(list, { kind: "topic", label: n, to: `/topics/${n}` });
    expect(list.map((r) => r.label)).toEqual(["c", "f", "e", "d", "b"]);
  });
  test("a hand-edited store is cleaned", () => {
    expect(parseRecents("not json")).toEqual([]);
    expect(parseRecents(JSON.stringify([{ kind: "topic", label: "x", to: "javascript:alert(1)" }, { kind: "page", label: "Home", to: "/" }]))).toEqual([{ kind: "page", label: "Home", to: "/" }]);
  });
});

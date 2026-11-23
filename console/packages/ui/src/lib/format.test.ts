import { describe, expect, test } from "bun:test";
import { bytes, clockMsUtc, clockUtc, dateTimeUtc, duration, hhmmUtc, micros, number, plural, pluralWord } from "./format";
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
  test("pluralWord", () => {
    expect(pluralWord("policy")).toBe("policies");
    expect(pluralWord("topic")).toBe("topics");
    expect(pluralWord("key")).toBe("keys");
    expect(pluralWord("bus")).toBe("buses");
    expect(pluralWord("batch")).toBe("batches");
    expect(pluralWord("log group")).toBe("log groups");
    expect(plural(2, "policy")).toBe("2 policies");
  });
  test("plural", () => {
    expect(plural(1, "topic")).toBe("1 topic");
    expect(plural(1200, "topic")).toBe("1,200 topics");
  });
  test("a call's time has its milliseconds, and a moment has its day", () => {
    expect(clockMsUtc(Date.UTC(2026, 10, 20, 8, 14, 22, 904))).toBe("08:14:22.904");
    expect(clockMsUtc(Date.UTC(2026, 10, 20, 8, 14, 22, 7))).toBe("08:14:22.007");
    expect(dateTimeUtc(Date.UTC(2026, 0, 2, 3, 4, 5))).toBe("2026-01-02 03:04:05 UTC");
  });
  test("how long a call took, in three figures at most", () => {
    expect(micros(0)).toBe("0 µs");
    expect(micros(412)).toBe("412 µs");
    expect(micros(2_449)).toBe("2.4 ms");
    expect(micros(19_400)).toBe("19 ms");
    expect(micros(999_400)).toBe("999 ms");
    expect(micros(1_250_000)).toBe("1.25 s");
    expect(micros(-3)).toBe("0 µs");
  });
  test("sizes are in binary units", () => {
    expect(bytes(0)).toBe("0 B");
    expect(bytes(512)).toBe("512 B");
    expect(bytes(1536)).toBe("1.5 KiB");
    expect(bytes(12 * 1024 * 1024)).toBe("12 MiB");
    expect(bytes(3 * 1024 ** 3)).toBe("3.0 GiB");
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

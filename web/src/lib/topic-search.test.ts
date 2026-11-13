import { describe, expect, test } from "bun:test";
import { bytes, span, stamp, timeoutText } from "./kafka";
import { messagesQuery, messagesSearch, partitionsOf, recordAt, topicsSearch, whole } from "./topic-search";

describe("the Topics search params", () => {
  test("a hand-edited address keeps only what can be used", () => {
    expect(messagesSearch({ partition: "2,0,x,2", offset: "-1", isolation: "dirty", record: "1:16050", mode: "tail", limit: 500 })).toEqual({
      dialog: undefined,
      partition: "0,2",
      offset: undefined,
      time: undefined,
      key: undefined,
      isolation: undefined,
      limit: undefined,
      record: "1:16050",
      mode: "tail",
    });
    expect(messagesSearch({ partition: 1, offset: 12 }).partition).toBe("1");
    expect(messagesSearch({ record: "1:x" }).record).toBeUndefined();
    expect(topicsSearch({ internal: true, q: "ord" })).toMatchObject({ internal: true, q: "ord" });
    expect(topicsSearch({ internal: "no" }).internal).toBeUndefined();
    expect(whole("007")).toBe(7);
    expect(whole("1e3")).toBeUndefined();
  });

  test("the API query repeats a partition per value, as the classic page reads it", () => {
    expect(messagesQuery({ partition: "0,2", isolation: "read_committed", key: "a b" })).toBe("partition=0&partition=2&key=a+b&isolation=read_committed");
    expect(partitionsOf(undefined)).toEqual([]);
    expect(recordAt("3:9")).toEqual({ partition: 3, offset: 9 });
  });
});

describe("the server's formatting, in TypeScript", () => {
  test("sizes, spans and times say what format.rs says", () => {
    expect(bytes(512)).toBe("512 B");
    expect(bytes(8_388_608)).toBe("8.0 MiB");
    expect(bytes(41_628_876)).toBe("39.7 MiB");
    expect(span(41)).toBe("41 s");
    expect(span(3 * 3600 + 5 * 60)).toBe("3 h 5 min");
    expect(span(-3)).toBe("0 s");
    expect(timeoutText(60_000)).toBe("60 s");
    expect(timeoutText(900_000)).toBe("15 min");
    const day = Date.UTC(2026, 10, 13, 14, 32, 7);
    expect(stamp(day, day + 1000)).toBe("14:32:07");
    expect(stamp(day, day + 86_400_000)).toBe("Nov 13 14:32");
  });
});

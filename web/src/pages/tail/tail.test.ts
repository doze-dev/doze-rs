import { describe, expect, test } from "bun:test";
import type { RecordView } from "@/api/types/RecordView";
import type { Shown } from "@/api/types/Shown";
import type { TailRecord } from "@/api/types/TailRecord";
import { askQuery, filterChips, following, recordOf, tailSearch, validPath } from "./search";
import { HUES, commands, hold, hueOf, huesOf, lineOf, prepend, rateText, rates, rowKey } from "./stream";

const text = (t: string, more: Partial<Shown> = {}): Shown => ({ kind: "text", bytes: t.length, text: t, clipped: false, preview: t.slice(0, 47), schema: null, ...more });
const rec = (topic: string, partition: number, offset: number): TailRecord => ({ topic, partition, offset, timestamp_ms: 1_700_000_000_000 + offset, key: text(`k${offset}`), value: text("{}", { kind: "json" }), headers: [], headers_total: 0, standing: "committed" });

describe("the address", () => {
  test("keeps names, patterns, the start and the filters the page can use, and nothing else", () => {
    const s = tailSearch({ topic: ["orders", "orders", "bad name"], pattern: "pay.*", from: "last", last: "900", partition: ["2", "0", "x"], key: " ord-1 ", header_value: "x", field: "a.b", equals: 7, record: "orders.eu:0:12", isolation: "nope" });
    expect(s.topic).toEqual(["orders"]);
    expect(s.pattern).toEqual(["pay.*"]);
    expect(s.from).toBe("last");
    expect(s.last).toBe(200);
    expect(s.partition).toEqual([0, 2]);
    expect(s.key).toBe("ord-1");
    expect(s.header_value).toBeUndefined();
    expect(s.equals).toBe("7");
    expect(s.isolation).toBeUndefined();
    expect(recordOf(s.record)).toEqual({ topic: "orders.eu", partition: 0, offset: 12 });
    expect(following(s)).toBe(true);
    expect(following(tailSearch({}))).toBe(false);
  });

  test("the ask is the server's names, and a filter's chip turns it off", () => {
    const s = tailSearch({ topic: ["a", "b"], pattern: "c.*", from: "last", partition: 1, value: "paid", header: "trace" });
    expect(askQuery(s)).toBe("topic=a&topic=b&pattern=c.*&from=last&last=10&partition=1&value=paid&header=trace");
    const chips = filterChips(s);
    expect(chips.map((c) => c.label)).toEqual(["value contains paid", "header trace", "partition 1"]);
    expect(chips.find((c) => c.id === "p1")!.off).toEqual({ partition: undefined });
    expect(askQuery(tailSearch({ topic: "a" }))).toBe("topic=a");
  });

  test("a JSON path is what the server reads", () => {
    for (const ok of ["customer.id", "items[0].sku", "$.a.b", "a[1][2]", ""]) expect(validPath(ok)).toBe(true);
    for (const bad of ["a..b", "a[x]", ".a", "a b"]) expect(validPath(bad)).toBe(false);
  });
});

describe("the rows", () => {
  test("a batch goes on top, no row twice, no row rewritten, the window cut at 200", () => {
    let rows = prepend([], [rec("o", 0, 2), rec("o", 0, 1)]);
    expect(rows.map(rowKey)).toEqual(["o:0:2", "o:0:1"]);
    expect(rows.every((r) => r.fresh)).toBe(true);
    const before = rows[1];
    rows = prepend(rows, [rec("p", 0, 9), rec("o", 0, 2)]);
    expect(rows.map(rowKey)).toEqual(["p:0:9", "o:0:2", "o:0:1"]);
    expect(rows[0]!.fresh).toBe(true);
    expect(rows[1]!.fresh).toBeUndefined();
    expect(rows[2]).not.toBe(before);
    expect(prepend(rows, [])).toBe(rows);
    const many = Array.from({ length: 300 }, (_, i) => rec("o", 1, 300 - i));
    expect(prepend(rows, many)).toHaveLength(200);
  });

  test("held rows wait newest first, none twice, at most 500", () => {
    let h = hold([], [rec("o", 0, 1)]);
    h = hold(h, [rec("o", 0, 3), rec("o", 0, 2), rec("o", 0, 1)]);
    expect(h.map(rowKey)).toEqual(["o:0:3", "o:0:2", "o:0:1"]);
    expect(hold([], Array.from({ length: 900 }, (_, i) => rec("o", 0, i)))).toHaveLength(500);
  });
});

describe("rates and colours", () => {
  test("records a second per topic over the last 10 s, never a window shorter than two paces", () => {
    const now = 100_000;
    const r = rates(
      [
        { at: now - 20_000, counts: [{ topic: "o", scanned: 999, matched: 0 }] },
        { at: now - 5_000, counts: [{ topic: "o", scanned: 30, matched: 10 }] },
        { at: now - 1_000, counts: [{ topic: "o", scanned: 20, matched: 0 }, { topic: "p", scanned: 5, matched: 5 }] },
      ],
      now,
      3_000,
    );
    expect(r.get("o")).toEqual({ scanned: 5, matched: 1 });
    expect(r.get("p")).toEqual({ scanned: 0.5, matched: 0.5 });
    expect(rates([{ at: now - 15_000, counts: [{ topic: "o", scanned: 30, matched: 0 }] }], now, 10_000).get("o")!.scanned).toBe(1.5);
    expect([0, 0.04, 0.45, 12.4, 999.6, 1234].map(rateText)).toEqual(["0", "0", "0.5", "12", "1000", "1.2k"]);
  });

  test("a topic's colour is its name's, and eight topics of a tail have eight colours", () => {
    expect(hueOf("orders")).toBe(hueOf("orders"));
    const names = ["orders", "payments", "shipments", "orders.eu", "orders.us", "audit", "clicks", "dlq"];
    const m = huesOf(names);
    expect(new Set(m.values()).size).toBe(HUES);
    for (const h of m.values()) expect(h >= 1 && h <= HUES).toBe(true);
    expect(huesOf(["orders"]).get("orders")).toBe(hueOf("orders"));
  });

  test("a cell is one line: the schema's preview, or the text with its breaks as spaces", () => {
    expect(lineOf(text('{\n  "a": 1\n}'))).toBe('{ "a": 1 }');
    expect(lineOf(text("abc", { clipped: true }))).toBe("abc …");
    expect(lineOf({ ...text(""), kind: "null" })).toBe("null");
    expect(lineOf({ ...text("x"), schema: { id: 1, kind: "AVRO", subject: null, version: null, message: null, json: null, preview: '{"id":7}', error: null, mismatch: null } })).toBe('{"id":7}');
  });
});

describe("copy as a command", () => {
  const view = (over: Partial<RecordView> = {}): RecordView => ({ topic: "orders", partition: 2, offset: 7, last_stable: 8, key: text("ord-1"), value: text(`{"state":"it's paid"}`, { kind: "json" }), headers: [{ key: "trace", value: text("t-1") }], timestamp_ms: 0, log_append_time: false, codec: "none", producer: null, transactional: false, txn: null, standing: "committed", ...over });

  test("kcat and the console producer write the key, the value and the headers again, every word quoted", () => {
    const c = commands(view(), "127.0.0.1:9092");
    if ("why" in c) throw new Error(c.why);
    expect(c.kcat).toBe(`printf '%s' '{"state":"it'\\''s paid"}' | kcat -P -b 127.0.0.1:9092 -t orders -p 2 -k ord-1 -H trace=t-1`);
    expect(c.producer).toBe(`printf '%s\\n' 'trace:t-1\tord-1\t{"state":"it'\\''s paid"}' |\nkafka-console-producer --bootstrap-server 127.0.0.1:9092 --topic orders --property parse.key=true --property parse.headers=true`);
  });

  test("a record that is not plain text says why instead", () => {
    for (const [over, words] of [
      [{ value: { ...text("00 01"), kind: "binary" as const } }, "binary"],
      [{ value: text("abc", { clipped: true }) }, "longer"],
      [{ key: text("a⟨U+0007⟩b") }, "control characters"],
      [{ value: { ...text(""), kind: "null" as const } }, "deletion"],
    ] as const) {
      const c = commands(view(over), "x:1");
      expect("why" in c && c.why).toContain(words);
    }
    const tabbed = commands(view({ value: text("a\tb") }), "x:1");
    expect("kcat" in tabbed && tabbed.producer).toContain("one record a line");
  });
});

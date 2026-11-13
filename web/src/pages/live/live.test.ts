import { describe, expect, test } from "bun:test";
import type { WireRow } from "@/api/types/WireRow";
import { SseParser } from "../../lib/sse";
import { apiQuery, filtered, liveSearch } from "./search";
import { HELD_MOST, hold, merge, newCount, parseSpeed, speedSecs } from "./tail";
import { classWord, clockMs, showsCode, took } from "./words";

function row(id: number, count = 1): WireRow {
  return {
    id,
    at_ms: 1_700_000_000_000 + id,
    last_ms: null,
    count,
    client: "orders",
    tag: null,
    api: "Produce",
    version: 9,
    transactional: false,
    target: "orders-0",
    target_sub: null,
    topics: ["orders"],
    group: null,
    class: "done",
    code: 0,
    code_name: "NONE",
    detail: "1 record",
    took_us: 1500,
    took_sub: null,
    idle: false,
    lab: null,
  };
}

describe("the tape (CON-1254)", () => {
  test("a new row is prepended and marked fresh; an existing row keeps its object", () => {
    const tape = [row(3), row(2), row(1)];
    const m = merge(tape, [row(5), row(4)]);
    expect(m.added).toBe(2);
    expect(m.rows.map((r) => r.id)).toEqual([5, 4, 3, 2, 1]);
    expect(m.rows[0]!.fresh).toBe(true);
    expect(m.rows[2]).toBe(tape[0]!);
    expect(m.rows[4]).toBe(tape[2]!);
  });

  test("a folded repeat that grew changes in place, where it is", () => {
    const tape = [row(3), row(2), row(1)];
    const m = merge(tape, [row(2, 7)]);
    expect(m.added).toBe(0);
    expect(m.rows.map((r) => r.id)).toEqual([3, 2, 1]);
    expect(m.rows[1]!.count).toBe(7);
    expect(m.rows[0]).toBe(tape[0]!);
  });

  test("nothing new is the same array (no render)", () => {
    const tape = [row(1)];
    expect(merge(tape, []).rows).toBe(tape);
  });

  test("the tape is cut at its most, the oldest first", () => {
    const tape = Array.from({ length: 200 }, (_, i) => row(200 - i));
    const m = merge(tape, [row(201), row(202)].reverse());
    expect(m.rows).toHaveLength(200);
    expect(m.rows[0]!.id).toBe(202);
    expect(m.rows.at(-1)!.id).toBe(3);
  });

  test("held rows keep one entry per id, newest first, bounded; the pill counts the ones the tape lacks", () => {
    let h = hold([], [row(4), row(3)]);
    h = hold(h, [row(5), row(3, 2)]);
    expect(h.map((r) => r.id)).toEqual([5, 3, 4]);
    expect(h[1]!.count).toBe(2);
    expect(newCount(h, [row(3)])).toBe(2);
    const many = hold([], Array.from({ length: 900 }, (_, i) => row(900 - i)));
    expect(many).toHaveLength(HELD_MOST);
  });
});

describe("the speed", () => {
  test("Relaxed unless one of the three was chosen", () => {
    expect(parseSpeed(null)).toBe("relaxed");
    expect(parseSpeed("fast")).toBe("relaxed");
    expect(parseSpeed("realtime")).toBe("realtime");
    expect(speedSecs("realtime")).toBe(1);
    expect(speedSecs("relaxed")).toBe(3);
    expect(speedSecs("slow")).toBe(10);
  });
});

describe("the address", () => {
  test("a hostile address is a page with no such filter", () => {
    const s = liveSearch({ client: ["a", "a", "", 3], api: "Produce", refused: "1", idle: "nope", request: "12x", before: "40", tail: true });
    expect(s.client).toEqual(["a", "3"]);
    expect(s.api).toEqual(["Produce"]);
    expect(s.refused).toBe(true);
    expect(s.idle).toBeUndefined();
    expect(s.request).toBeUndefined();
    expect(s.before).toBe(40);
    expect(s.tail).toBe(true);
  });

  test("the filters travel to the server under its own names", () => {
    const s = liveSearch({ q: "ord", client: ["a", "b"], topic: ["orders"], refused: true, idle: true, before: 7, tail: true, request: 3 });
    expect(apiQuery(s)).toBe("q=ord&client=a&client=b&topic=orders&refused=1&idle=0&before=7");
    expect(apiQuery(s, false)).toBe("q=ord&client=a&client=b&topic=orders&refused=1&idle=0");
    expect(filtered(s)).toBe(true);
    expect(filtered(liveSearch({ idle: true, tail: true }))).toBe(false);
  });
});

describe("the words of a row", () => {
  test("as the classic console says them", () => {
    expect(classWord("told", 27)).toBe("Told to rejoin");
    expect(classWord("told", 7)).toBe("Told to retry");
    expect(classWord("denied", 29)).toBe("Denied");
    expect(took(800)).toBe("<1 ms");
    expect(took(1_203_000)).toBe("1,203 ms");
    expect(clockMs(Date.UTC(2026, 10, 13, 22, 14, 22, 904))).toBe("22:14:22.904");
    expect(showsCode({ class: "done", code: 0 })).toBe(false);
    expect(showsCode({ class: "closed", code: 0 })).toBe(false);
    expect(showsCode({ class: "refused", code: 87 })).toBe(true);
  });
});

describe("the stream reader", () => {
  test("events across chunks, comments, ids, CRLF", () => {
    const p = new SseParser();
    expect(p.push(": keep-alive\n\nevent: ready\ndata: {\"a\"")).toEqual([]);
    expect(p.push(":1}\n\nevent: rows\r\nid: 9.4\r\ndata: x\r\ndata: y\r\n\r\n")).toEqual([
      { event: "ready", data: '{"a":1}', id: undefined },
      { event: "rows", data: "x\ny", id: "9.4" },
    ]);
    expect(p.push("data: z\n")).toEqual([]);
    expect(p.push("\n")).toEqual([{ event: "message", data: "z", id: undefined }]);
  });
});

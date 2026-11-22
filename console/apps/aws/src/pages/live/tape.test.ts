import { describe, expect, test } from "bun:test";
import type { CallRecord } from "../../api/types/CallRecord";
import type { CallsPage } from "../../api/types/CallsPage";
import { admit, EMPTY, findCall, newestSeq, reduce, retryDelayMs, type Tape } from "./tape";

const call = (seq: number): CallRecord => ({ seq, at_ms: 1_800_000_000_000 + seq, service: "sqs", api: "SQS", action: "SendMessage", resource: null, member: null, caller: "test", region: "us-east-1", status: 200, error_code: null, outcome: "ok", would_be_denied: false, duration_us: 100, request_id: `r-${seq}`, via: null, sdk: null, message: null, function_error: null, answer_bytes: null });

/** A page of the calls numbered `seqs` (oldest first, as the server sends them). */
function page(seqs: number[], more: Partial<CallsPage> = {}, capacity = 1024): CallsPage {
  const last = seqs[seqs.length - 1] ?? 0;
  return { calls: seqs.map(call), cursor: `7.${last}`, reset: false, missed: 0, more: false, ring: { capacity, held: seqs.length, recorded: last, bytes: 0 }, ...more };
}

const seqs = (t: Tape) => t.calls.map((c) => c.seq);
const loaded = (list: number[], capacity = 1024) => reduce(EMPTY, { type: "loaded", page: page(list, {}, capacity) });

describe("the Live tape", () => {
  test("the first page is shown newest first, and none of it is highlighted", () => {
    const t = loaded([1, 2, 3]);
    expect(seqs(t)).toEqual([3, 2, 1]);
    expect(t.loaded).toBe(true);
    expect(t.cursor).toBe("7.3");
    expect(t.calls.filter((c) => c.seq > t.freshAfter)).toEqual([]);
    expect(t.ring?.capacity).toBe(1024);
  });

  test("an empty first page is a loaded, empty tape", () => {
    const t = loaded([]);
    expect(t.loaded).toBe(true);
    expect(seqs(t)).toEqual([]);
    expect(newestSeq(t)).toBe(0);
  });

  test("calls from the stream go on top, and only they are highlighted", () => {
    const t = reduce(loaded([1, 2, 3]), { type: "calls", page: page([4, 5]) });
    expect(seqs(t)).toEqual([5, 4, 3, 2, 1]);
    expect(t.cursor).toBe("7.5");
    expect(t.calls.filter((c) => c.seq > t.freshAfter).map((c) => c.seq)).toEqual([5, 4]);
    // The next batch moves the highlight on.
    const u = reduce(t, { type: "calls", page: page([6]) });
    expect(u.calls.filter((c) => c.seq > u.freshAfter).map((c) => c.seq)).toEqual([6]);
  });

  test("a call already on the tape is not shown twice", () => {
    const t = reduce(loaded([1, 2, 3]), { type: "calls", page: page([2, 3, 4]) });
    expect(seqs(t)).toEqual([4, 3, 2, 1]);
    // A batch of nothing new changes no row (and keeps the highlight where it was).
    const u = reduce(t, { type: "calls", page: page([3, 4]) });
    expect(u.calls).toBe(t.calls);
    expect(u.freshAfter).toBe(t.freshAfter);
  });

  test("the tape keeps as many calls as the record does", () => {
    const t = reduce(loaded([1, 2, 3], 4), { type: "calls", page: page([4, 5, 6], {}, 4) });
    expect(seqs(t)).toEqual([6, 5, 4, 3]);
  });

  test("while paused the stream's calls are held and counted, and Resume shows them", () => {
    let t = reduce(loaded([1, 2]), { type: "pause" });
    expect(t.paused).toBe(true);
    t = reduce(t, { type: "calls", page: page([3, 4]) });
    t = reduce(t, { type: "calls", page: page([5]) });
    expect(seqs(t)).toEqual([2, 1]);
    expect(t.held.map((c) => c.seq)).toEqual([5, 4, 3]);
    expect(t.cursor).toBe("7.5");
    t = reduce(t, { type: "resume" });
    expect(t.paused).toBe(false);
    expect(seqs(t)).toEqual([5, 4, 3, 2, 1]);
    expect(t.held).toEqual([]);
    // What was held is what is new on screen.
    expect(t.calls.filter((c) => c.seq > t.freshAfter).map((c) => c.seq)).toEqual([5, 4, 3]);
  });

  test("a held call is not held twice, and the hold is bounded: what falls out is counted as missed", () => {
    let t = reduce(loaded([1], 3), { type: "pause" });
    t = reduce(t, { type: "calls", page: page([2, 3], {}, 3) });
    t = reduce(t, { type: "calls", page: page([3, 4, 5], {}, 3) });
    expect(t.held.map((c) => c.seq)).toEqual([5, 4, 3]);
    expect(t.missed).toBe(1);
  });

  test("pausing twice and resuming twice change nothing the second time", () => {
    const paused = reduce(loaded([1]), { type: "pause" });
    expect(reduce(paused, { type: "pause" })).toBe(paused);
    const live = reduce(paused, { type: "resume" });
    expect(live.paused).toBe(false);
    expect(reduce(live, { type: "resume" })).toBe(live);
  });

  test("calls the record dropped before they were sent are counted", () => {
    const t = reduce(loaded([1, 2]), { type: "calls", page: page([40, 41], { missed: 37 }) });
    expect(t.missed).toBe(37);
    expect(seqs(t)).toEqual([41, 40, 2, 1]);
  });

  test("a restart of doze-aws drops the old run's calls", () => {
    const old = reduce(reduce(loaded([10, 11, 12]), { type: "pause" }), { type: "calls", page: page([13]) });
    const t = reduce(old, { type: "ready", ready: { cursor: "9.0", reset: true } });
    expect(seqs(t)).toEqual([]);
    expect(t.held).toEqual([]);
    expect(t.cursor).toBe("9.0");
    // The new run numbers its calls from 1 again: they are not taken for repeats.
    const u = reduce(reduce(t, { type: "resume" }), { type: "calls", page: page([1, 2]) });
    expect(seqs(u)).toEqual([2, 1]);
    // A page that says reset itself does the same.
    const v = reduce(loaded([10, 11]), { type: "calls", page: page([1], { reset: true }) });
    expect(seqs(v)).toEqual([1]);
  });

  test("a stream that opens where the tape ends changes nothing", () => {
    const t = loaded([1, 2]);
    expect(reduce(t, { type: "ready", ready: { cursor: "7.2", reset: false } })).toBe(t);
  });

  test("a call is found by its number, on screen or held", () => {
    let t = reduce(loaded([1, 2]), { type: "pause" });
    t = reduce(t, { type: "calls", page: page([3]) });
    expect(findCall(t, 2)?.request_id).toBe("r-2");
    expect(findCall(t, 3)?.request_id).toBe("r-3");
    expect(findCall(t, 9)).toBeUndefined();
    expect(findCall(t, undefined)).toBeUndefined();
  });
});

test("the stream is tried again after 1 s, 2 s, 5 s, then every 10 s", () => {
  expect([1, 2, 3, 4, 5, 50].map(retryDelayMs)).toEqual([1_000, 2_000, 5_000, 10_000, 10_000, 10_000]);
  expect(retryDelayMs(0)).toBe(1_000);
});

test("past the cap the console's own calls go first, as the record lets them go (ring.rs)", () => {
  const own = (seq: number): CallRecord => ({ ...call(seq), caller: "console" });
  // The same case as the record's own test: two of an app's, then the console reading page after page, in a ring of 8.
  let calls: CallRecord[] = [];
  for (const c of [call(1), call(2), ...Array.from({ length: 18 }, (_, i) => own(i + 3))]) calls = admit(calls, [c], 8);
  expect(calls.map((c) => c.seq)).toEqual([20, 19, 18, 17, 16, 15, 2, 1]);
  // In one batch it is the same.
  expect(admit([], [call(1), call(2), ...Array.from({ length: 18 }, (_, i) => own(i + 3))].reverse(), 8).map((c) => c.seq)).toEqual([20, 19, 18, 17, 16, 15, 2, 1]);
  // Within its share, the oldest goes, whoever made it.
  const apps = Array.from({ length: 8 }, (_, i) => call(i + 1)).reverse();
  expect(admit(apps, [call(10), own(9)], 8).map((c) => c.seq)).toEqual([10, 9, 8, 7, 6, 5, 4, 3]);
});

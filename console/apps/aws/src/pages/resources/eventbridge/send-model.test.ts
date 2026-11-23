import { describe, expect, test } from "bun:test";
import { emptySend, matchHint, putEventsInput, sendProblems, sentWords, wholeEvent } from "./send-model";

describe("the entry", () => {
  const f = emptySend("larder");
  test("is checked on its fields", () => {
    expect(sendProblems(f)).toEqual({});
    expect(sendProblems({ ...f, source: "", detailType: "x".repeat(129), detail: "[]" })).toEqual({ source: "An entry needs a source.", detailType: "A detail type is at most 128 characters.", detail: "The detail is a JSON object." });
    expect(sendProblems({ ...f, detail: "{" }).detail).toContain("Not JSON");
  });
  test("goes as one entry with the detail as compact JSON text", () => {
    expect(putEventsInput(f)).toEqual({ Entries: [{ EventBusName: "larder", Source: "larder.orders", DetailType: "OrderPlaced", Detail: '{"order_id":"o-20931","customer_id":"c-1187","total_cents":4850}' }] });
  });
  test("a whole event for a pattern", () => {
    const e = JSON.parse(wholeEvent(f, "111122223333", "us-east-1", new Date("2026-11-08T08:13:41.123Z"))) as Record<string, unknown>;
    expect(e).toMatchObject({ source: "larder.orders", "detail-type": "OrderPlaced", account: "111122223333", region: "us-east-1", time: "2026-11-08T08:13:41Z", resources: [] });
  });
});

describe("the answer and the hint", () => {
  test("an id, or the entry's error", () => {
    expect(sentWords({ FailedEntryCount: 0, Entries: [{ EventId: "e-1" }] })).toEqual({ ok: true, text: "PutEvents returned the EventId e-1." });
    expect(sentWords({ FailedEntryCount: 1, Entries: [{ ErrorCode: "InternalFailure", ErrorMessage: "x" }] })).toEqual({ ok: false, text: "PutEvents answered InternalFailure: x" });
  });
  test("which rules it matches, and where the others live", () => {
    expect(matchHint("larder", [{ Name: "a" }], ["a"], [])).toBe("This event matches the rule a on larder.");
    expect(matchHint("larder", [{ Name: "a" }, { Name: "b" }], [], [])).toBe("This event matches none of the rules on larder.");
    expect(matchHint("default", [], [], [{ bus: "larder", rule: "order-placed" }])).toBe("The rule order-placed lives on larder, so this event on default matches none of your rules.");
    expect(matchHint("x", [], null, [])).toBe("");
  });
});

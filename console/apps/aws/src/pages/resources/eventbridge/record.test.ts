import { describe, expect, test } from "bun:test";
import type { CallRecord } from "../../../api/types/CallRecord";
import { busCalls, isLook, matchedRules, resultNote, ruleCalls, ruleOf } from "./record";

const call = (seq: number, patch: Partial<CallRecord>): CallRecord => ({ seq, at_ms: seq * 100, service: "eventbridge", api: "EventBridge", action: "PutRule", resource: null, member: null, caller: "k", region: "us-east-1", status: 200, error_code: null, outcome: "ok", would_be_denied: false, duration_us: 1, request_id: `r${seq}`, via: null, sdk: null, ...patch }) as CallRecord;
const by = (name: string) => ({ by: "eventbridge", request_id: null, name: `rule ${name}` });

describe("what the record says of a bus and a rule", () => {
  const put = call(5, { action: "PutEvents", resource: "larder" });
  const delivery = call(6, { service: "sns", action: "Publish", resource: "arn:aws:sns:us-east-1:1:orders-created", via: by("order-placed") });
  const second = call(7, { action: "PutEvents", resource: "larder" });
  const late = call(8, { service: "sqs", action: "SendMessage", resource: "http://x/pick-lists", via: by("payment-settled") });
  const putRule = call(2, { resource: "order-placed" });
  const record = [putRule, put, delivery, second, late];
  test("the rule a call was made for", () => {
    expect(ruleOf(delivery)).toBe("order-placed");
    expect(ruleOf(put)).toBeNull();
  });
  test("a bus page adds the calls that named its rules", () => {
    expect(busCalls([put], record, ["order-placed"]).map((c) => c.seq)).toEqual([5, 2]);
  });
  test("a rule page adds the deliveries it made", () => {
    // The PutEvents it matched is among them; the one the other rule matched is not.
    expect(ruleCalls([putRule], record, "order-placed").map((c) => c.seq)).toEqual([6, 5, 2]);
    expect(ruleCalls([], record, "payment-settled").map((c) => c.seq)).toEqual([8, 7]);
  });
  test("a PutEvents matched the rules that delivered after it and before the next PutEvents", () => {
    expect(matchedRules(put, record)).toEqual(["order-placed"]);
    expect(matchedRules(second, record)).toEqual(["payment-settled"]);
    expect(resultNote(put, record)).toBe("matched order-placed");
    expect(resultNote(put, [put])).toBe("matched no rule");
    expect(resultNote(delivery, record)).toBe("to orders-created");
    expect(resultNote(putRule, record)).toBeNull();
  });
  test("a delivery is one row: the looks the rule took before it are not, unless one failed", () => {
    const look = call(5.5 as number, { service: "sns", action: "GetTopicAttributes", resource: "arn:aws:sns:us-east-1:1:orders-created", via: by("order-placed") });
    const refusedLook = { ...look, seq: 5.6, status: 403 } as CallRecord;
    expect(isLook(look)).toBe(true);
    expect(isLook(refusedLook)).toBe(false);
    expect(isLook(delivery)).toBe(false);
    expect(isLook({ ...look, via: null })).toBe(false);
    expect(ruleCalls([putRule], [...record, look, refusedLook], "order-placed").map((c) => c.seq)).toEqual([6, 5.6, 5, 2]);
  });
  test("a delivery says what its input was made of", () => {
    const t = { Id: "t", Arn: "arn:aws:sns:us-east-1:1:orders-created", InputTransformer: { InputPathsMap: { a: "$.a", b: "$.b" }, InputTemplate: "x" } };
    expect(resultNote(delivery, record, t)).toBe("to orders-created, input transformer, 2 variables");
    expect(resultNote(delivery, record, { Id: "t", Arn: t.Arn })).toBe("to orders-created");
  });
  test("another caller's delivery, or one long after, is not its match", () => {
    expect(matchedRules(put, [put, { ...delivery, caller: "other" }])).toEqual([]);
    expect(matchedRules(put, [put, { ...delivery, at_ms: put.at_ms + 10_000 }])).toEqual([]);
  });
});

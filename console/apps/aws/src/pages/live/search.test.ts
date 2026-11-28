import { describe, expect, test } from "bun:test";
import type { CallRecord } from "../../api/types/CallRecord";
import { filtering, liveSearch, lookOf, matches } from "./search";

const call = (over: Partial<CallRecord>) => ({ outcome: "ok", status: 200, would_be_denied: false, service: "sqs", caller: "k", action: "CreateQueue", resource: null, ...over }) as CallRecord;

describe("the wire's Refused filter", () => {
  test("keeps the calls a service refused (an error under 500), not the ones it failed", () => {
    const f = liveSearch({ outcome: "refused" });
    expect(f.outcome).toBe("refused");
    expect(matches(call({ outcome: "error", status: 400 }), f)).toBe(true);
    expect(matches(call({ outcome: "error", status: 500 }), f)).toBe(false);
    expect(matches(call({}), f)).toBe(false);
  });

  test("Error still keeps every error", () => {
    const f = liveSearch({ outcome: "error" });
    expect(matches(call({ outcome: "error", status: 500 }), f)).toBe(true);
    expect(matches(call({ outcome: "error", status: 400 }), f)).toBe(true);
  });
});

describe("the Needs a look chips as a filter", () => {
  test("a look keeps only the calls of its kind, and is dropped when it is not one of ours", () => {
    const f = liveSearch({ look: "refused" });
    expect(f.look).toBe("refused");
    expect(filtering(f)).toBe(true);
    expect(matches(call({ outcome: "error", status: 400 }), f)).toBe(true);
    expect(matches(call({ outcome: "error", status: 500 }), f)).toBe(false);
    expect(matches(call({ outcome: "error", status: 400, via: { by: "sns", name: null } as CallRecord["via"] }), f)).toBe(false);
    expect(liveSearch({ look: "nonsense" }).look).toBeUndefined();
  });
  test("each kind is told apart by lookOf", () => {
    expect(lookOf(call({ function_error: "Unhandled" }))).toBe("function-error");
    expect(lookOf(call({ action: "Invoke", status: 202, dropped: true }))).toBe("dropped");
    expect(lookOf(call({ would_be_denied: true }))).toBe("would-be-denied");
    expect(lookOf(call({ outcome: "not-built", status: 501 }))).toBe("not-built");
    expect(lookOf(call({ outcome: "denied", status: 403 }))).toBe("denied");
    expect(lookOf(call({ outcome: "error", status: 503 }))).toBe("failed");
    expect(lookOf(call({}))).toBeNull();
  });
});

describe("the Lab on the wire", () => {
  const lab = (id: number, kind: "failed" | "dropped" | "delayed" = "failed") => ({ id, name: `rule ${id}`, effect: kind === "delayed" ? { kind, ms: 250 } : { kind } }) as CallRecord["lab"];
  test("the Lab filter keeps the calls any rule touched, or one rule's", () => {
    const any = liveSearch({ lab: "any" });
    const one = liveSearch({ lab: 3 });
    expect(any.lab).toBe("any");
    expect(one.lab).toBe(3);
    expect(filtering(any) && filtering(one)).toBe(true);
    expect(matches(call({ lab: lab(3) }), any)).toBe(true);
    expect(matches(call({ lab: lab(4) }), any)).toBe(true);
    expect(matches(call({ lab: null }), any)).toBe(false);
    expect(matches(call({ lab: lab(3) }), one)).toBe(true);
    expect(matches(call({ lab: lab(4) }), one)).toBe(false);
    expect(matches(call({ lab: null }), one)).toBe(false);
  });
  test("what is not a rule number is dropped from the address", () => {
    for (const bad of [0, -1, 1.5, "3", "all", null, {}]) expect(liveSearch({ lab: bad }).lab).toBeUndefined();
  });
  test("a call whose connection was closed with no answer is a failure, not a refusal", () => {
    const dropped = call({ outcome: "error", status: 0, lab: lab(1, "dropped") });
    expect(lookOf(dropped)).toBe("failed");
    expect(matches(dropped, liveSearch({ outcome: "refused" }))).toBe(false);
    expect(matches(dropped, liveSearch({ outcome: "error" }))).toBe(true);
  });
});

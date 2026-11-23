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
    expect(lookOf(call({ would_be_denied: true }))).toBe("would-be-denied");
    expect(lookOf(call({ outcome: "not-built", status: 501 }))).toBe("not-built");
    expect(lookOf(call({ outcome: "denied", status: 403 }))).toBe("denied");
    expect(lookOf(call({ outcome: "error", status: 503 }))).toBe("failed");
    expect(lookOf(call({}))).toBeNull();
  });
});

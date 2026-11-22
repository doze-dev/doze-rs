import { describe, expect, test } from "bun:test";
import type { CallRecord } from "../../api/types/CallRecord";
import { liveSearch, matches } from "./search";

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

import { describe, expect, test } from "bun:test";
import type { CallRecord } from "../../api/types/CallRecord";
import { heardSince, probe } from "./check";

const call = (seq: number, at_ms: number) => ({ seq, at_ms }) as CallRecord;

describe("heardSince", () => {
  const home = { recent: [call(3, 3000), call(2, 2000), call(1, 1000)] };
  test("since the start, the newest call", () => {
    expect(heardSince(home, 0)?.seq).toBe(3);
  });
  test("a call before the check began does not tick it", () => {
    expect(heardSince(home, 3000)).toBeNull();
    expect(heardSince(home, 2999)?.seq).toBe(3);
  });
  test("no call, nothing heard", () => {
    expect(heardSince({ recent: [] }, 0)).toBeNull();
  });
});

test("the probe names the address", () => {
  expect(probe("http://127.0.0.1:4566")).toBe("aws sts get-caller-identity \\\n  --endpoint-url http://127.0.0.1:4566");
});

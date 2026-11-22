import { expect, test } from "bun:test";
import type { CallRecord } from "../../api/types/CallRecord";
import { looks } from "./needs-look";

let seq = 0;
const call = (p: Partial<CallRecord>): CallRecord => ({ seq: ++seq, at_ms: 1, service: "sqs", api: "SQS", action: "SendMessage", resource: null, member: null, caller: "AKIA1", region: "us-east-1", status: 200, error_code: null, outcome: "ok", would_be_denied: false, duration_us: 1, request_id: "r", via: null, sdk: null, message: null, function_error: null, answer_bytes: null, summary: null, ...p });

test("one chip a kind, the newest of it, with how many there are; a call that went well has none", () => {
  const calls = [
    call({ action: "Invoke", function_error: "Unhandled" }),
    call({ action: "GetObject", would_be_denied: true }),
    call({ action: "CreateQueue", status: 400, outcome: "error", error_code: "InvalidParameterValue" }),
    call({ action: "CreateQueue", status: 400, outcome: "error", error_code: "InvalidParameterValue" }),
    call({ action: "SelectObjectContent", status: 501, outcome: "not-built", error_code: "NotImplemented" }),
    call({ action: "ListQueues" }),
  ];
  const l = looks(calls);
  expect(l.map((x) => x.word)).toEqual(["FUNCTION ERROR", "WOULD BE DENIED (soft)", "REFUSED", "NOT BUILT"]);
  expect(l[2]?.count).toBe(2);
  expect(l[0]?.tone).toBe("bad");
});

test("a server error is a failure, not a refusal", () => {
  expect(looks([call({ status: 500, outcome: "error" })])[0]?.word).toBe("FAILED");
});

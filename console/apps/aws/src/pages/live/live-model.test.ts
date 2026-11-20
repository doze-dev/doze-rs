import { describe, expect, test } from "bun:test";
import type { CallRecord } from "../../api/types/CallRecord";
import { childrenOf, madeBy, parentOf } from "./cause";
import { flowKeyOf, fromServer, layout, lineWidth, withCalls } from "./flows";
import { beadColour } from "./map";
import { lineId, lines } from "./repeats";
import { nameIn, resourceHref, resourceName } from "./resource";
import { callerOf, filtering, liveSearch, matches } from "./search";

const call = (seq: number, more: Partial<CallRecord> = {}): CallRecord => ({ seq, at_ms: 1_800_000_000_000 + seq, service: "sqs", api: "SQS", action: "SendMessage", resource: "orders", member: "QueueName", caller: "AKIA1", region: "us-east-1", status: 200, error_code: null, outcome: "ok", would_be_denied: false, duration_us: 100, request_id: `r-${seq}`, via: null, ...more });
const via = (by: string, request_id: string | null = null, name: string | null = null) => ({ by, request_id, name });

describe("Live's address", () => {
  test("only known values are kept", () => {
    expect(liveSearch({ call: 4, view: "map", service: "sqs", outcome: "would-be-denied", caller: "AKIA1", q: " orders " })).toEqual({ call: 4, view: "map", service: "sqs", outcome: "would-be-denied", caller: "AKIA1", q: "orders" });
    const none = { call: undefined, view: undefined, service: undefined, outcome: undefined, caller: undefined, q: undefined };
    expect(liveSearch({ call: -1, view: "grid", service: "S Q S", outcome: "fine", caller: "", q: "  " })).toEqual(none);
    expect(liveSearch({ call: "abc" })).toEqual(none);
  });

  test("filters keep the calls that match every one of them", () => {
    const a = call(1);
    const b = call(2, { service: "sns", action: "Publish", resource: "arn:aws:sns:us-east-1:1:placed", outcome: "error", status: 400 });
    const c = call(3, { via: via("sns", "r-2"), caller: "AKIA1", would_be_denied: true });
    expect([a, b, c].filter((x) => matches(x, { service: "sqs" })).map((x) => x.seq)).toEqual([1, 3]);
    expect([a, b, c].filter((x) => matches(x, { outcome: "error" })).map((x) => x.seq)).toEqual([2]);
    expect([a, b, c].filter((x) => matches(x, { outcome: "would-be-denied" })).map((x) => x.seq)).toEqual([3]);
    // A call between services is the service's, and also the key's it was made for.
    expect([a, b, c].filter((x) => matches(x, { caller: "sns" })).map((x) => x.seq)).toEqual([3]);
    expect([a, b, c].filter((x) => matches(x, { caller: "AKIA1" })).map((x) => x.seq)).toEqual([1, 2, 3]);
    expect([a, b, c].filter((x) => matches(x, { q: "PLACED" })).map((x) => x.seq)).toEqual([2]);
    expect([a, b, c].filter((x) => matches(x, { q: "publish", service: "sqs" }))).toEqual([]);
    expect(callerOf(c)).toBe("sns");
    expect(filtering({})).toBe(false);
    expect(filtering({ q: "x" })).toBe(true);
  });
});

describe("repeats", () => {
  test("calls in a row with the same caller, operation, resource and outcome are one line, opened on asking", () => {
    // Newest first: three polls, a different call, two more polls.
    const tape = [call(7, { action: "ReceiveMessage" }), call(6, { action: "ReceiveMessage" }), call(5), call(4, { action: "ReceiveMessage" }), call(3, { action: "ReceiveMessage" }), call(2, { action: "ReceiveMessage" })];
    const shut = lines(tape, new Set());
    expect(shut.map((l) => [l.kind, l.call.seq, l.kind === "run" ? l.calls.length : 1])).toEqual([
      ["run", 7, 2],
      ["call", 5, 1],
      ["run", 4, 3],
    ]);
    // A run is known by its oldest call, which does not change as it grows.
    const opened = lines(tape, new Set([2]));
    expect(opened.map(lineId)).toEqual(["7", "5", "4", "2:3", "2:2"]);
    // An outcome, a resource or a cause that differs breaks the run.
    expect(lines([call(2), call(1, { outcome: "error" })], new Set()).length).toBe(2);
    expect(lines([call(2), call(1, { resource: "other" })], new Set()).length).toBe(2);
    expect(lines([call(2), call(1, { via: via("sns") })], new Set()).length).toBe(2);
  });
});

describe("resource names", () => {
  test("the name is read out of a URL, an ARN or an S3 path; anything else is as it is", () => {
    expect(nameIn("http://127.0.0.1:4566/000000000000/orders")).toBe("orders");
    expect(nameIn("arn:aws:sns:us-east-1:000000000000:placed")).toBe("placed");
    expect(nameIn("arn:aws:lambda:us-east-1:000000000000:function:resize:live")).toBe("resize");
    expect(nameIn("arn:aws:states:us-east-1:000000000000:stateMachine:fulfil")).toBe("fulfil");
    expect(nameIn("arn:aws:iam::000000000000:role/service/ci")).toBe("ci");
    expect(nameIn("arn:aws:kms:us-east-1:000000000000:key/1234-abcd")).toBe("1234-abcd");
    expect(nameIn("alias/orders")).toBe("alias/orders");
    expect(nameIn("/aws/lambda/resize")).toBe("/aws/lambda/resize");
    expect(resourceName({ service: "s3", member: null, resource: "photos/2026/a.jpg" })).toBe("photos");
    expect(resourceName({ service: "sqs", member: null, resource: null })).toBe(null);
    expect(resourceHref({ service: "logs", member: "logGroupName", resource: "/aws/lambda/resize" })).toBe("/resources/logs/%2Faws%2Flambda%2Fresize");
    expect(resourceHref({ service: null, member: "QueueName", resource: "orders" })).toBe(null);
  });
});

describe("the map", () => {
  test("flows from the server and the calls since are counted once, and laid out caller to service", () => {
    const server = fromServer({
      flows: [
        { caller: "AKIA1", internal: false, service: "sns", api: "SNS", calls: 3, ok: 3, error: 0, denied: 0, not_built: 0, would_be_denied: 0, first_ms: 1, last_ms: 2, last_seq: 4 },
        { caller: "sns", internal: true, service: "sqs", api: "SQS", calls: 1, ok: 1, error: 0, denied: 0, not_built: 0, would_be_denied: 0, first_ms: 1, last_ms: 2, last_seq: 3 },
      ],
      capacity: 64,
      dropped: 0,
      through_seq: 4,
    });
    const tape = [call(6, { outcome: "error", status: 400 }), call(5, { via: via("sns", "r-4") }), call(4, { service: "sns" }), call(3, { via: via("sns") })];
    const flows = withCalls(server, tape);
    expect(flows.through).toBe(6);
    expect(flows.byKey.get(flowKeyOf(call(5, { via: via("sns") })))?.calls).toBe(2);
    expect(flows.byKey.get(flowKeyOf(call(6)))?.error).toBe(1);
    expect(flows.byKey.get(flowKeyOf(call(4, { service: "sns" })))?.calls).toBe(3);
    // Nothing new: the same object (the map is not redrawn).
    expect(withCalls(flows, tape)).toBe(flows);

    const map = layout(flows);
    expect(map.callers.map((s) => [s.id, s.internal])).toEqual([
      ["AKIA1", false],
      ["sns", true],
    ]);
    expect(map.services.map((s) => s.id)).toEqual(["sns", "sqs"]);
    expect(map.lines.length).toBe(3);
    expect(map.callers[0]!.y).toBeLessThan(map.callers[1]!.y);
    const few = layout(flows, { callers: 1, services: 1 });
    expect([few.hiddenCallers, few.hiddenServices, few.lines.length]).toEqual([1, 1, 1]);
    expect(lineWidth(1)).toBe(1.5);
    expect(lineWidth(1e9)).toBe(7);
    expect(beadColour({ outcome: "denied" })).toBe("var(--refused)");
  });
});

describe("the cause chain", () => {
  test("a call between services finds the request it was made in, and that request its calls", () => {
    const publish = call(9, { service: "sns", action: "Publish", request_id: "req-p" });
    const sent = call(8, { via: via("sns", "req-p") });
    const ruled = call(7, { service: "lambda", action: "Invoke", via: via("events", null, "rule nightly") });
    const held = [publish, sent, ruled];
    expect(parentOf(sent, held)).toBe(publish);
    expect(parentOf(ruled, held)).toBe(undefined);
    expect(childrenOf(publish, held)).toEqual([sent]);
    expect(childrenOf(sent, held)).toEqual([]);
    const titles = new Map([["sns", "SNS"], ["events", "EventBridge"]]);
    expect(madeBy(sent, titles)).toBe("SNS");
    expect(madeBy(ruled, titles)).toBe("EventBridge, rule nightly");
  });
});

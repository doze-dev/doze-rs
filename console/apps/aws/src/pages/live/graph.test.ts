import { describe, expect, test } from "bun:test";
import type { CallRecord } from "../../api/types/CallRecord";
import type { Wiring } from "../../api/types/Wiring";
import { buildMap, perMinute, routeOf, targetOf, wiresOf } from "./graph";

const NOW = 10_000_000;
let seq = 0;
const call = (p: Partial<CallRecord>): CallRecord => ({ seq: ++seq, at_ms: NOW - 1000, service: "sqs", api: "SQS", action: "SendMessage", resource: "http://h/1/pick-lists", member: "QueueUrl", caller: "AKIA1", region: "us-east-1", status: 200, error_code: null, outcome: "ok", would_be_denied: false, duration_us: 1, request_id: "r", via: null, sdk: null, ...p });
const wiring = (nodes: [string, string?][], edges: [string, string, string, string?][]): Wiring => ({
  nodes: nodes.map(([id, sentence]) => { const [service = "", kind = "", name = ""] = id.split("/"); return { id, service, kind, name, sentence: sentence ?? null }; }),
  edges: edges.map(([from, to, label, note]) => ({ from, to, label, note: note ?? null })),
  looked: [],
  more: false,
  failed: [],
});
const opts = { now: NOW, focus: "all" as const, layout: "flow" as const, callerName: (k: string) => (k === "AKIA1" ? "fulfilment" : k) };

describe("targetOf", () => {
  test("a call names the resource of its kind, an S3 path its bucket, and a call with none its service", () => {
    expect(targetOf(call({}))).toMatchObject({ id: "sqs/queues/pick-lists", kind: "resource" });
    expect(targetOf(call({ service: "s3", member: null, resource: "product-images/a.jpg" }))).toMatchObject({ id: "s3/buckets/product-images" });
    expect(targetOf(call({ service: "eventbridge", resource: null, member: null }))).toMatchObject({ id: "eventbridge", kind: "service" });
    expect(targetOf(call({ service: null }))).toBeNull();
  });
});

describe("buildMap", () => {
  const w = wiring([["sqs/queues/pick-lists", "2 waiting · 1 in flight"], ["sqs/queues/pick-lists-dlq"], ["s3/buckets/receipts"]], [["sqs/queues/pick-lists", "sqs/queues/pick-lists-dlq", "redrive", "after 5 receives"]]);
  const m = buildMap([call({}), call({ action: "ReceiveMessage" })], w, opts);
  test("an app is joined to what it called, and a node holds its live sentence", () => {
    expect(m.nodes.find((n) => n.id === "app|AKIA1")?.name).toBe("fulfilment");
    expect(m.nodes.find((n) => n.id === "sqs/queues/pick-lists")).toMatchObject({ sentence: "2 waiting · 1 in flight", calls: 2, ops: ["SendMessage", "ReceiveMessage"] });
    expect(m.edges.find((e) => e.kind === "call")?.calls).toBe(2);
  });
  test("what a called resource is wired to is drawn with it, in the next column", () => {
    const dlq = m.nodes.find((n) => n.id === "sqs/queues/pick-lists-dlq")!;
    expect(dlq.col).toBe(2);
    expect(m.edges.find((e) => e.kind === "wire")).toMatchObject({ label: "redrive", note: "after 5 receives" });
  });
  test("a resource nothing called and nothing is wired to is not wired", () => {
    expect(m.notWired.map((n) => n.id)).toEqual(["s3/buckets/receipts"]);
    expect(m.nodes.some((n) => n.id === "s3/buckets/receipts")).toBe(false);
  });
  test("focus on the active drops what was quiet for five minutes, and counts it apart", () => {
    const old = call({ at_ms: NOW - 6 * 60_000, service: "sqs", resource: "http://h/1/old-q" });
    const f = buildMap([call({}), old], undefined, { ...opts, focus: "active" });
    expect(f.nodes.some((n) => n.id === "sqs/queues/old-q")).toBe(false);
    expect(f.quiet.map((n) => n.id)).toEqual(["sqs/queues/old-q"]);
  });
  test("a refusal and a would-be-denied are counted on the node", () => {
    const x = buildMap([call({ status: 400, outcome: "error" }), call({ would_be_denied: true })], undefined, opts);
    expect(x.nodes.find((n) => n.id === "sqs/queues/pick-lists")?.seen).toEqual({ wouldBeDenied: 1, refused: 1, error: 0, notBuilt: 0 });
  });
  test("by domain: apps, then the services' groups", () => {
    const d = buildMap([call({}), call({ service: "s3", member: null, resource: "b/k" })], undefined, { ...opts, layout: "domain" });
    expect(d.columns).toBe(3);
    expect(d.nodes.find((n) => n.id === "s3/buckets/b")?.col).toBe(2);
  });
});

test("the route of a call runs from its caller to what it named and on along the wiring", () => {
  const w = wiring([["sqs/queues/q"], ["sqs/queues/q-dlq"]], [["sqs/queues/q", "sqs/queues/q-dlq", "redrive"]]);
  const c = call({ resource: "http://h/1/q" });
  const m = buildMap([c], w, opts);
  const r = routeOf(m, c, "AKIA1");
  expect([...r.nodes].sort()).toEqual(["app|AKIA1", "sqs/queues/q", "sqs/queues/q-dlq"]);
  expect(r.edges.size).toBe(2);
  expect(wiresOf(m, "sqs/queues/q-dlq").fedBy.map((x) => x.node.id)).toEqual(["sqs/queues/q"]);
  expect(wiresOf(m, "sqs/queues/q").callers.map((n) => n.name)).toEqual(["fulfilment"]);
});

test("per-minute counts end at now", () => {
  expect(perMinute([{ at_ms: NOW - 10 }, { at_ms: NOW - 70_000 }, { at_ms: NOW - 61_000 }], NOW, 3)).toEqual([0, 2, 1]);
});

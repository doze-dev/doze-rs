import { describe, expect, test } from "bun:test";
import type { CallRecord } from "../../api/types/CallRecord";
import type { Wiring } from "../../api/types/Wiring";
import { buildMap, drawsEverything, perMinute, routeOf, targetOf, wiresOf } from "./graph";

const NOW = 10_000_000;
let seq = 0;
const call = (p: Partial<CallRecord>): CallRecord => ({ seq: ++seq, at_ms: NOW - 1000, service: "sqs", api: "SQS", action: "SendMessage", resource: "http://h/1/pick-lists", member: "QueueUrl", caller: "AKIA1", region: "us-east-1", status: 200, error_code: null, outcome: "ok", would_be_denied: false, allowed_by: null, duration_us: 1, request_id: "r", via: null, sdk: null, message: null, function_error: null, answer_bytes: null, summary: null, dropped: false, lab: null, ...p });
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
    expect(x.nodes.find((n) => n.id === "sqs/queues/pick-lists")?.seen).toEqual({ wouldBeDenied: 1, refused: 1, error: 0, notBuilt: 0, functionError: 0 });
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

describe("a busy world", () => {
  const busy = Array.from({ length: 30 }, (_, i) => call({ resource: `http://h/1/q${i}`, caller: i % 2 ? "AKIA1" : "AKIA2" }));
  test("draws the busiest flows and counts the rest as quieter, until all flows are asked for", () => {
    const calls = busy.flatMap((c, i) => Array.from({ length: 30 - i }, () => ({ ...c, seq: ++seq })));
    const active = buildMap(calls, undefined, { ...opts, focus: "active" });
    expect(active.nodes.filter((n) => n.kind === "resource").length).toBe(10);
    expect(active.quiet.length).toBe(20);
    expect(active.nodes.some((n) => n.name === "q0")).toBe(true);
    expect(active.nodes.some((n) => n.name === "q29")).toBe(false);
    expect(active.height).toBeLessThan(11 * 78 + 80);
    const all = buildMap(calls, undefined, { ...opts, focus: "all" });
    expect(all.nodes.filter((n) => n.kind === "resource").length).toBe(30);
    expect(all.quiet.length).toBe(0);
  });
  test("a flow is one row: what a resource feeds is level with it, a second thing it feeds takes the row under", () => {
    const w = wiring([["sqs/queues/a"], ["sqs/queues/dlq"], ["lambda/functions/f"]], [["sqs/queues/a", "sqs/queues/dlq", "redrive"], ["sqs/queues/a", "lambda/functions/f", "polling"]]);
    const m = buildMap([call({ resource: "http://h/1/a" })], w, opts);
    const y = (id: string) => m.nodes.find((n) => n.id === id)!.y;
    expect(y("sqs/queues/dlq")).toBe(y("sqs/queues/a"));
    expect(y("lambda/functions/f")).toBeGreaterThan(y("sqs/queues/a"));
    expect(m.nodes.find((n) => n.id === "sqs/queues/dlq")!.col).toBe(2);
  });
  test("an app is level with what it calls, and carries its client", () => {
    const m = buildMap([call({ sdk: "Go v2 1.26.0" })], undefined, opts);
    const app = m.nodes.find((n) => n.kind === "app")!;
    expect(app.sdk).toBe("Go v2");
    expect(app.y).toBe(m.nodes.find((n) => n.kind === "resource")!.y);
  });
});

test("by domain the columns leave room above them for a line that runs over a column, and only then", () => {
  // An app calls a queue (the middle column) and a bucket (the last): its call to the bucket skips the middle column.
  const calls = [call({}), call({ service: "s3", member: null, resource: "b/k" })];
  const over = buildMap(calls, undefined, { ...opts, layout: "domain" });
  const apps = buildMap([call({})], undefined, { ...opts, layout: "domain" });
  expect(over.edges.some((e) => over.nodes.find((n) => n.id === e.to)?.col === 2 && over.nodes.find((n) => n.id === e.from)?.col === 0)).toBe(true);
  expect(over.height).toBeGreaterThan(apps.height);
  expect(Math.min(...over.nodes.map((n) => n.y))).toBeGreaterThan(Math.min(...apps.nodes.map((n) => n.y)));
});

test("by domain a column holds its busiest eight and the rest are quieter", () => {
  const calls = Array.from({ length: 12 }, (_, i) => Array.from({ length: 12 - i }, () => call({ resource: `http://h/1/q${i}` }))).flat();
  const m = buildMap([...calls, call({ service: "s3", member: null, resource: "b/k" })], undefined, { ...opts, focus: "active", layout: "domain" });
  expect(m.nodes.filter((n) => n.service === "sqs").length).toBe(8);
  expect(m.nodes.some((n) => n.service === "s3")).toBe(true);
  expect(m.quiet.length).toBe(4);
});

describe("an idle instance", () => {
  test("draws every flow when Focus was not chosen, nothing is active and quieter flows exist", () => {
    const old = [call({ at_ms: NOW - 20 * 60_000 })];
    const asked = buildMap(old, undefined, { now: NOW, focus: "active", layout: "flow", callerName: (k) => k });
    expect(asked.nodes).toHaveLength(0);
    expect(asked.quiet.length).toBeGreaterThan(0);
    expect(drawsEverything(false, asked)).toBe(true);
    expect(drawsEverything(true, asked)).toBe(false);
    const all = buildMap(old, undefined, { now: NOW, focus: "all", layout: "flow", callerName: (k) => k });
    expect(all.nodes.length).toBeGreaterThan(0);
  });
  test("an instance that never heard a call has nothing to draw instead", () => {
    const asked = buildMap([], undefined, { now: NOW, focus: "active", layout: "flow", callerName: (k) => k });
    expect(drawsEverything(false, asked)).toBe(false);
  });
});

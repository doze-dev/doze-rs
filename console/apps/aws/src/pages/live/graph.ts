// The resource map's model: who called which resource (from the calls the page holds), how the resources are wired to each other
// (from `/api/v1/live/wiring`), and where each node and line goes. Pure: the page draws what this returns.
//
// A call is joined to the resource it named (a queue, a bucket, a function); a call that named none is joined to its service.
// A wired line is configuration (a rule's target, a dead-letter queue, a subscription, a bucket's notification): it is there
// whether or not anything has moved along it. A resource nothing called and nothing is wired to is "not wired".
import type { CallRecord } from "../../api/types/CallRecord";
import type { Wiring } from "../../api/types/Wiring";
import { clientName } from "../../people";
import { callerOf } from "./search";
import { resourceName } from "./resource";

export type NodeKind = "app" | "service" | "resource";

export type Seen = { wouldBeDenied: number; refused: number; error: number; notBuilt: number; functionError: number };

export type MapNode = {
  id: string;
  kind: NodeKind;
  /** `--services` name (a resource's or a service's); empty for an app. */
  service: string;
  /** The Resources kind (`queues`); empty when the call did not say (the Resources space finds it by `member`). */
  resKind: string;
  name: string;
  /** What it holds now, from the services; null when none said. */
  sentence: string | null;
  member: string | null;
  calls: number;
  /** Newest call to or from it, in ms; 0 when none is on the tape. */
  lastMs: number;
  /** Calls in the minute before `now`. */
  lastMinute: number;
  ops: string[];
  /** The client an app calls with (`Go v2`); null for anything else. */
  sdk: string | null;
  seen: Seen;
  col: number;
  y: number;
};

export type MapEdge = { id: string; from: string; to: string; kind: "call" | "wire"; label: string | null; note: string | null; calls: number; lastMs: number };

export type MapModel = { nodes: MapNode[]; edges: MapEdge[]; columns: number; height: number; quiet: MapNode[]; notWired: MapNode[]; more: boolean };

export type Layout = "flow" | "domain";
export type Focus = "active" | "all";

/** How long a node counts as active after its newest call. */
export const ACTIVE_MS = 5 * 60 * 1000;

const KIND_OF_MEMBER: Record<string, { service?: string; kind: string }> = {
  QueueUrl: { kind: "queues" },
  QueueName: { kind: "queues" },
  TopicArn: { kind: "topics" },
  TargetArn: { kind: "topics" },
  TableName: { kind: "tables" },
  FunctionName: { kind: "functions" },
  StreamName: { kind: "streams" },
  StateMachineArn: { kind: "machines" },
  SecretId: { kind: "secrets" },
  LogGroupName: { kind: "groups" },
  EventBusName: { kind: "buses" },
};

const GROUPS: Record<string, string> = { sqs: "Messaging and events", sns: "Messaging and events", eventbridge: "Messaging and events", kinesis: "Messaging and events", scheduler: "Messaging and events", s3: "Storage and compute", dynamodb: "Storage and compute", lambda: "Storage and compute", stepfunctions: "Storage and compute", apigateway: "Storage and compute" };

/** The domain columns, in order. */
const DOMAINS = ["Apps", "Messaging and events", "Storage and compute", "Other"];

/** The domain columns `nodes` need, in order. */
export const domainColumns = (nodes: readonly Pick<MapNode, "kind" | "service">[]): string[] => DOMAINS.filter((d) => nodes.some((n) => domainOf(n) === d));

/** The domain column a node is in under {@link Layout} `domain`: Apps, Messaging and events, Storage and compute, Other. */
export function domainOf(n: Pick<MapNode, "kind" | "service">): string {
  return n.kind === "app" ? "Apps" : (GROUPS[n.service] ?? "Other");
}

/** The node id a call's target is: a resource by `service/kind/name`, else its service. */
export function targetOf(c: Pick<CallRecord, "service" | "resource" | "member">): { id: string; kind: NodeKind; service: string; resKind: string; name: string; member: string | null } | null {
  if (!c.service) return null;
  const name = resourceName(c as CallRecord);
  if (!name) return { id: c.service, kind: "service", service: c.service, resKind: "", name: c.service, member: null };
  const resKind = c.member === null && c.service === "s3" ? "buckets" : (KIND_OF_MEMBER[c.member ?? ""]?.kind ?? "");
  return { id: `${c.service}/${resKind}/${name}`, kind: "resource", service: c.service, resKind, name, member: c.member };
}

const blank = (id: string, kind: NodeKind, service: string, resKind: string, name: string, member: string | null): MapNode => ({ id, kind, service, resKind, name, sentence: null, member, calls: 0, lastMs: 0, lastMinute: 0, ops: [], sdk: null, seen: { wouldBeDenied: 0, refused: 0, error: 0, notBuilt: 0, functionError: 0 }, col: 0, y: 0 });

function touch(n: MapNode, c: CallRecord, now: number) {
  n.calls += 1;
  n.lastMs = Math.max(n.lastMs, c.at_ms);
  if (now - c.at_ms <= 60_000) n.lastMinute += 1;
  if (c.action && !n.ops.includes(c.action)) n.ops.push(c.action);
  if (c.would_be_denied) n.seen.wouldBeDenied += 1;
  if (c.outcome === "error") {
    if (c.status < 500) n.seen.refused += 1;
    else n.seen.error += 1;
  }
  if (c.outcome === "not-built") n.seen.notBuilt += 1;
  if (c.function_error) n.seen.functionError += 1;
}

export const ROW = 78;
/** The most flows (a resource and what it is wired to) drawn while Focus is on the active ones: a busy world is the busiest, and the
 * rest are counted as quieter flows. All flows draws every one. */
export const MOST_FLOWS = 10;
/** The most a domain column holds while Focus is on the active ones. */
export const MOST_PER_COLUMN = 8;
export const TOP = 18;

/** The map of `calls` (any order; this console's own already left out) and the wiring. `callerName` says how an app is shown. */
export function buildMap(calls: readonly CallRecord[], wiring: Wiring | undefined, o: { now: number; focus: Focus; layout: Layout; callerName: (key: string) => string }): MapModel {
  const nodes = new Map<string, MapNode>();
  const edges = new Map<string, MapEdge>();
  const node = (id: string, make: () => MapNode) => nodes.get(id) ?? (nodes.set(id, make()), nodes.get(id)!);
  const edge = (from: string, to: string, kind: "call" | "wire", label: string | null, note: string | null): MapEdge => {
    const id = `${kind}|${from}|${to}|${label ?? ""}|${note ?? ""}`;
    return edges.get(id) ?? (edges.set(id, { id, from, to, kind, label, note, calls: 0, lastMs: 0 }), edges.get(id)!);
  };

  for (const w of wiring?.nodes ?? []) {
    const n = node(w.id, () => blank(w.id, "resource", w.service, w.kind, w.name, null));
    n.sentence = w.sentence;
  }
  for (const w of wiring?.edges ?? []) {
    if (nodes.has(w.from) && nodes.has(w.to)) edge(w.from, w.to, "wire", w.label, w.note);
  }
  // Oldest first, so an app's operations read in the order it made them.
  for (const c of [...calls].sort((a, b) => a.seq - b.seq)) {
    if (c.via) continue;
    const t = targetOf(c);
    if (!t) continue;
    const key = callerOf(c);
    const appId = `app|${key}`;
    const app = node(appId, () => blank(appId, "app", "", "", o.callerName(key), null));
    touch(app, c, o.now);
    if (c.sdk && !app.sdk) app.sdk = clientName(c.sdk);
    const to = node(t.id, () => blank(t.id, t.kind, t.service, t.resKind, t.name, t.member));
    touch(to, c, o.now);
    const e = edge(appId, t.id, "call", null, null);
    e.calls += 1;
    e.lastMs = Math.max(e.lastMs, c.at_ms);
  }

  const all = [...nodes.values()];
  const called = new Set(all.filter((n) => n.calls > 0).map((n) => n.id));
  // Wired to something an app called, however far along the wiring: drawn with it.
  const near = new Set(called);
  for (let grew = true; grew; ) {
    grew = false;
    for (const e of edges.values()) {
      if (e.kind !== "wire") continue;
      if (near.has(e.from) !== near.has(e.to)) {
        near.add(e.from);
        near.add(e.to);
        grew = true;
      }
    }
  }
  const wired = new Set([...edges.values()].filter((e) => e.kind === "wire").flatMap((e) => [e.from, e.to]));
  const notWired = all.filter((n) => n.kind === "resource" && !near.has(n.id) && !wired.has(n.id));
  const active = (n: MapNode) => n.lastMs > 0 && o.now - n.lastMs <= ACTIVE_MS;
  const keep = all.filter((n) => near.has(n.id));
  // A resource is quiet when nothing touched it lately and nothing quiet-or-not that is drawn feeds it from an active line.
  const activeIds = new Set(keep.filter(active).map((n) => n.id));
  for (let grew = true; grew; ) {
    grew = false;
    for (const e of edges.values()) {
      if (e.kind === "wire" && (activeIds.has(e.from) || activeIds.has(e.to)) && (!activeIds.has(e.from) || !activeIds.has(e.to))) {
        activeIds.add(e.from);
        activeIds.add(e.to);
        grew = true;
      }
    }
  }
  const active_ = o.focus === "all" ? keep : keep.filter((n) => activeIds.has(n.id));
  // The flows (a resource and what it is wired to), the busiest first; past MOST_FLOWS the rest are quieter flows.
  const flows = flowsOf(active_.filter((n) => n.kind !== "app"), [...edges.values()]);
  // By flow, the busiest flows; by domain, the busiest of each column (a column is as tall as its own, not the sum of the others').
  const byDomain = (nodes: MapNode[]) => domainColumns(nodes).flatMap((d) => nodes.filter((n) => domainOf(n) === d).sort((a, b) => b.calls - a.calls).slice(0, MOST_PER_COLUMN));
  const kept = o.focus === "all" ? flows : o.layout === "domain" ? [byDomain(flows.flat())] : flows.slice(0, MOST_FLOWS);
  const keptIds = new Set(kept.flat().map((n) => n.id));
  const quiet = [...keep.filter((n) => n.kind !== "app" && !keptIds.has(n.id))];
  const apps = new Set([...edges.values()].filter((e) => e.kind === "call" && keptIds.has(e.to)).map((e) => e.from));
  const shown = active_.filter((n) => (n.kind === "app" ? apps.has(n.id) : keptIds.has(n.id)));
  const shownIds = new Set(shown.map((n) => n.id));
  const drawn = [...edges.values()].filter((e) => shownIds.has(e.from) && shownIds.has(e.to));

  const { columns, height } = place(shown, drawn, kept, o.layout);
  return { nodes: shown, edges: drawn, columns, height, quiet, notWired, more: wiring?.more ?? false };
}

/** The flows among `nodes`: each resource with everything wired to it or from it, the busiest flow first. */
function flowsOf(nodes: MapNode[], edges: MapEdge[]): MapNode[][] {
  const root = new Map(nodes.map((n) => [n.id, n.id]));
  const find = (id: string): string => {
    const r = root.get(id) ?? id;
    return r === id ? id : find(r);
  };
  for (const e of edges) if (e.kind === "wire" && root.has(e.from) && root.has(e.to)) root.set(find(e.from), find(e.to));
  const by = new Map<string, MapNode[]>();
  for (const n of nodes) by.set(find(n.id), [...(by.get(find(n.id)) ?? []), n]);
  const weight = (f: MapNode[]) => f.reduce((sum, n) => sum + n.calls, 0);
  return [...by.values()].sort((a, b) => weight(b) - weight(a) || a[0]!.name.localeCompare(b[0]!.name));
}

/** Sets each node's column and y. Flow: one row for each flow, a resource and what it feeds along the row (a resource one column after
 * the last wired thing that feeds it; a second thing it feeds takes the row under); apps on the left, level with what they call.
 * Domain: Apps, Messaging and events, Storage and compute, Other, the busiest first in a column. */
function place(shown: MapNode[], edges: MapEdge[], flows: MapNode[][], layout: Layout): { columns: number; height: number } {
  if (layout === "domain") {
    const cols = domainColumns(shown);
    for (const n of shown) n.col = cols.indexOf(domainOf(n));
    stack(shown, cols.length);
    const tallest = Math.max(1, ...Array.from({ length: cols.length }, (_, c) => shown.filter((n) => n.col === c).length));
    return { columns: Math.max(1, cols.length), height: TOP * 2 + tallest * ROW };
  }
  const wire = edges.filter((e) => e.kind === "wire");
  const byId = new Map(shown.map((n) => [n.id, n]));
  const row = new Map<string, number>();
  let next = 0;
  const put = (id: string, r: number, col: number): number => {
    const n = byId.get(id);
    if (!n) return r - 1;
    row.set(id, r);
    n.col = col;
    let used = r;
    wire
      .filter((e) => e.from === id && byId.has(e.to) && !row.has(e.to))
      .forEach((e, i) => {
        used = Math.max(used, put(e.to, i === 0 ? r : used + 1, col + 1));
      });
    return used;
  };
  for (const flow of flows) {
    const fed = new Set(wire.filter((e) => flow.some((n) => n.id === e.from)).map((e) => e.to));
    const roots = flow.filter((n) => !fed.has(n.id) && byId.has(n.id));
    for (const r of roots.length > 0 ? roots : flow.slice(0, 1)) if (!row.has(r.id)) next = put(r.id, next, 1) + 1;
  }
  for (const n of shown) {
    if (n.kind !== "app") n.y = TOP + (row.get(n.id) ?? 0) * ROW + ROW / 2;
    if (n.kind === "app") n.col = 0;
  }
  // An app level with what it calls (the middle of their rows), kept a row apart.
  const wants = shown
    .filter((n) => n.kind === "app")
    .map((n) => {
      const ys = edges.filter((e) => e.kind === "call" && e.from === n.id).map((e) => byId.get(e.to)?.y ?? 0);
      return { n, y: ys.length > 0 ? ys.reduce((a, b) => a + b, 0) / ys.length : 0 };
    })
    .sort((a, b) => a.y - b.y);
  let floor = TOP + ROW / 2;
  for (const w of wants) {
    w.n.y = Math.max(w.y, floor);
    floor = w.n.y + ROW;
  }
  const bottom = Math.max(TOP + ROW / 2, ...shown.map((n) => n.y));
  const columns = Math.max(1, ...shown.map((n) => n.col + 1));
  return { columns, height: bottom + ROW / 2 + TOP };
}

function stack(shown: MapNode[], columns: number) {
  const tallest = Math.max(1, ...Array.from({ length: columns }, (_, c) => shown.filter((n) => n.col === c).length));
  for (let c = 0; c < columns; c++) {
    const col = shown.filter((n) => n.col === c).sort((a, b) => b.calls - a.calls || a.name.localeCompare(b.name));
    const off = ((tallest - col.length) * ROW) / 2;
    col.forEach((n, i) => (n.y = TOP + off + i * ROW + ROW / 2));
  }
}

/** The nodes and lines of the route of `call`: its caller to what it named, then on along the wiring from there. */
export function routeOf(m: MapModel, call: CallRecord, callerKey: string): { nodes: Set<string>; edges: Set<string> } {
  const t = targetOf(call);
  const nodes = new Set<string>();
  const edges = new Set<string>();
  if (!t) return { nodes, edges };
  nodes.add(`app|${callerKey}`);
  nodes.add(t.id);
  for (const e of m.edges) if (e.kind === "call" && e.from === `app|${callerKey}` && e.to === t.id) edges.add(e.id);
  const queue = [t.id];
  while (queue.length > 0) {
    const at = queue.pop()!;
    for (const e of m.edges) {
      if (e.kind === "wire" && e.from === at && !nodes.has(e.to)) {
        nodes.add(e.to);
        edges.add(e.id);
        queue.push(e.to);
      }
    }
  }
  return { nodes, edges };
}

/** What a node is wired from and to, for its panel: lines into it (`Fed by`), out of it by label. */
export function wiresOf(m: MapModel | { edges: MapEdge[]; nodes: MapNode[] }, id: string): { fedBy: { node: MapNode; edge: MapEdge }[]; out: { node: MapNode; edge: MapEdge }[]; callers: MapNode[] } {
  const by = new Map(m.nodes.map((n) => [n.id, n]));
  const pick = (e: MapEdge, other: string) => {
    const node = by.get(other);
    return node ? { node, edge: e } : null;
  };
  return {
    fedBy: m.edges.filter((e) => e.kind === "wire" && e.to === id).flatMap((e) => pick(e, e.from) ?? []),
    out: m.edges.filter((e) => e.kind === "wire" && e.from === id).flatMap((e) => pick(e, e.to) ?? []),
    callers: m.edges.filter((e) => e.kind === "call" && e.to === id).flatMap((e) => by.get(e.from) ?? []),
  };
}

/** Per-minute counts of `calls` over the span the page holds, oldest first: `minutes` buckets ending at `now`. */
export function perMinute(calls: readonly { at_ms: number }[], now: number, minutes = 30): number[] {
  const out = new Array<number>(minutes).fill(0);
  for (const c of calls) {
    const ago = Math.floor((now - c.at_ms) / 60_000);
    if (ago >= 0 && ago < minutes) out[minutes - 1 - ago]! += 1;
  }
  return out;
}

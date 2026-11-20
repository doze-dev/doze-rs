// The map's model: who called which service, from `/api/v1/flows` (counted by the server since doze-aws started) plus the
// calls the stream brought since, and where each station and line goes. Pure: the map draws what this returns.
import type { CallRecord } from "../../api/types/CallRecord";
import type { FlowsResponse } from "../../api/types/FlowsResponse";
import { callerOf } from "./search";

export type Flow = {
  key: string;
  /** An access key, or for calls between services the service that made them. */
  caller: string;
  internal: boolean;
  service: string;
  calls: number;
  ok: number;
  error: number;
  denied: number;
  notBuilt: number;
  wouldBeDenied: number;
  lastSeq: number;
};

export type Flows = { byKey: Map<string, Flow>; through: number };

export const flowKey = (caller: string, internal: boolean, service: string) => `${internal ? "via" : "key"}|${caller}|${service}`;

/** The flow a call is counted in. */
export function flowKeyOf(c: CallRecord): string {
  return flowKey(callerOf(c), c.via !== null, c.service ?? "");
}

export function fromServer(r: FlowsResponse): Flows {
  const byKey = new Map<string, Flow>();
  for (const f of r.flows) {
    const service = f.service ?? "";
    const key = flowKey(f.caller, f.internal, service);
    byKey.set(key, { key, caller: f.caller, internal: f.internal, service, calls: f.calls, ok: f.ok, error: f.error, denied: f.denied, notBuilt: f.not_built, wouldBeDenied: f.would_be_denied, lastSeq: f.last_seq });
  }
  return { byKey, through: r.through_seq };
}

/** `flows` with the calls numbered above what it counts added (calls in any order); the same object when there are none. */
export function withCalls(flows: Flows, calls: readonly CallRecord[]): Flows {
  const fresh = calls.filter((c) => c.seq > flows.through);
  if (fresh.length === 0) return flows;
  const byKey = new Map(flows.byKey);
  let through = flows.through;
  for (const c of fresh) {
    const key = flowKeyOf(c);
    const was = byKey.get(key) ?? { key, caller: callerOf(c), internal: c.via !== null, service: c.service ?? "", calls: 0, ok: 0, error: 0, denied: 0, notBuilt: 0, wouldBeDenied: 0, lastSeq: 0 };
    byKey.set(key, {
      ...was,
      calls: was.calls + 1,
      ok: was.ok + (c.outcome === "ok" ? 1 : 0),
      error: was.error + (c.outcome === "error" ? 1 : 0),
      denied: was.denied + (c.outcome === "denied" ? 1 : 0),
      notBuilt: was.notBuilt + (c.outcome === "not-built" ? 1 : 0),
      wouldBeDenied: was.wouldBeDenied + (c.would_be_denied ? 1 : 0),
      lastSeq: Math.max(was.lastSeq, c.seq),
    });
    through = Math.max(through, c.seq);
  }
  return { byKey, through };
}

export type Station = { id: string; internal: boolean; calls: number; error: number; denied: number; y: number };
export type Line = { key: string; from: Station; to: Station; flow: Flow };
export type MapLayout = { callers: Station[]; services: Station[]; lines: Line[]; hiddenCallers: number; hiddenServices: number; height: number };

export const ROW = 54;
export const TOP = 14;

function stations(flows: Flow[], side: (f: Flow) => string, internal: (f: Flow) => boolean, most: number): { shown: Station[]; hidden: number } {
  const by = new Map<string, Station>();
  for (const f of flows) {
    const id = side(f);
    const s = by.get(id) ?? { id, internal: internal(f), calls: 0, error: 0, denied: 0, y: 0 };
    s.calls += f.calls;
    s.error += f.error;
    s.denied += f.denied;
    by.set(id, s);
  }
  const all = [...by.values()].sort((a, b) => b.calls - a.calls || a.id.localeCompare(b.id));
  const shown = all.slice(0, most).sort((a, b) => Number(a.internal) - Number(b.internal) || b.calls - a.calls || a.id.localeCompare(b.id));
  return { shown, hidden: all.length - shown.length };
}

/** Callers on the left, services on the right, each column centred on the taller one; a line for each flow between two shown
 * stations. At most `most` stations a side (the busiest); the rest are counted. */
export function layout(flows: Flows, most = { callers: 8, services: 10 }): MapLayout {
  const all = [...flows.byKey.values()];
  const c = stations(all, (f) => f.caller, (f) => f.internal, most.callers);
  const s = stations(all, (f) => f.service, () => false, most.services);
  const rows = Math.max(c.shown.length, s.shown.length, 1);
  const place = (list: Station[]) => {
    const off = ((rows - list.length) * ROW) / 2;
    list.forEach((st, i) => (st.y = TOP + off + i * ROW + ROW / 2));
  };
  place(c.shown);
  place(s.shown);
  const left = new Map(c.shown.map((st) => [st.id, st]));
  const right = new Map(s.shown.map((st) => [st.id, st]));
  const lines: Line[] = [];
  for (const f of all) {
    const from = left.get(f.caller);
    const to = right.get(f.service);
    if (from && to) lines.push({ key: f.key, from, to, flow: f });
  }
  return { callers: c.shown, services: s.shown, lines, hiddenCallers: c.hidden, hiddenServices: s.hidden, height: TOP * 2 + rows * ROW };
}

/** A line's width by how many calls it carried: 1.5 px for one, up to 7 px. */
export function lineWidth(calls: number): number {
  return Math.min(7, 1.5 + 1.6 * Math.log10(Math.max(1, calls)));
}

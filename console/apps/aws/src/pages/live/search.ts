// Live's address: which view, which call's drawer is open, and the filters of the wire. Everything a person picks is here, so a
// reload or a link sent to someone shows the same thing. Anything that is not one of the known values is dropped: a hand-edited
// address can put no other state on screen.
import type { CallRecord } from "../../api/types/CallRecord";
import type { Outcome } from "../../api/types/Outcome";

export const VIEWS = ["map", "wire", "both"] as const;
export type View = (typeof VIEWS)[number];

/** The outcomes a filter can ask for: the contract's four, and the calls soft IAM let through that it would refuse. */
export const OUTCOME_FILTERS = ["ok", "error", "denied", "not-built", "would-be-denied"] as const;
export type OutcomeFilter = (typeof OUTCOME_FILTERS)[number];

export type LiveSearch = {
  /** The call whose drawer is open. */
  call?: number;
  /** Map, Wire (the table) or both; Both when not given. */
  view?: View;
  /** A service by its `--services` name. */
  service?: string;
  outcome?: OutcomeFilter;
  /** An access key, or for a call between services the service that made it. */
  caller?: string;
  /** Text looked for in the operation and the resource. */
  q?: string;
  /** `shown`: this console's own calls are shown too (hidden otherwise). */
  console?: "shown";
  /** The node of the resource map whose panel is open (`sqs/queues/pick-lists`). */
  node?: string;
  /** `all`: every flow on the map, not only those active in the last five minutes. */
  focus?: "all";
  /** `domain`: the map in columns by domain (Apps, Messaging and events, Storage and compute). */
  layout?: "domain";
  /** `service`: the map of apps and services, not of resources. */
  by?: "service";
};

/** The caller this console's own calls carry (`aws-console/src/resources/bus.rs`). */
export const CONSOLE_CALLER = "console";

/** Made by this console, or by a service on its behalf (both carry its caller). */
export const isConsole = (c: Pick<CallRecord, "caller">): boolean => c.caller === CONSOLE_CALLER;

/** Whether this console's own calls are shown: asked for, or the caller filter names the console. */
export const consoleShown = (f: LiveSearch): boolean => f.console === "shown" || f.caller === CONSOLE_CALLER;

const text = (v: unknown, most = 256): string | undefined => (typeof v === "string" && v.trim() !== "" ? v.trim().slice(0, most) : undefined);

export function liveSearch(s: Record<string, unknown>): LiveSearch {
  const n = s.call;
  // Every key is set, to undefined when the value is not one of ours: the router keeps a raw value the validator leaves out.
  const out: LiveSearch = { call: undefined, view: undefined, service: undefined, outcome: undefined, caller: undefined, q: undefined, console: undefined, node: undefined, focus: undefined, layout: undefined, by: undefined };
  if (typeof n === "number" && Number.isSafeInteger(n) && n > 0) out.call = n;
  if (VIEWS.includes(s.view as View)) out.view = s.view as View;
  const service = text(s.service, 64);
  if (service && /^[a-z0-9-]+$/.test(service)) out.service = service;
  if (OUTCOME_FILTERS.includes(s.outcome as OutcomeFilter)) out.outcome = s.outcome as OutcomeFilter;
  const caller = text(s.caller, 128);
  if (caller) out.caller = caller;
  const q = text(s.q);
  if (q) out.q = q;
  if (s.console === "shown") out.console = "shown";
  const node = text(s.node, 160);
  if (node) out.node = node;
  if (s.focus === "all") out.focus = "all";
  if (s.layout === "domain") out.layout = "domain";
  if (s.by === "service") out.by = "service";
  return out;
}

/** Who a call is from, as Live names it: the access key, or for a call between services the service that made it. */
export function callerOf(c: Pick<CallRecord, "caller" | "via">): string {
  return c.via?.by ?? c.caller ?? "";
}

/** Whether `c` is one the filters keep. */
export function matches(c: CallRecord, f: LiveSearch): boolean {
  if (f.service !== undefined && c.service !== f.service) return false;
  if (f.outcome !== undefined) {
    if (f.outcome === "would-be-denied" ? !c.would_be_denied : c.outcome !== (f.outcome as Outcome)) return false;
  }
  if (f.caller !== undefined && callerOf(c) !== f.caller && c.caller !== f.caller) return false;
  if (f.q !== undefined) {
    const q = f.q.toLowerCase();
    if (!(c.action ?? "").toLowerCase().includes(q) && !(c.resource ?? "").toLowerCase().includes(q)) return false;
  }
  return true;
}

/** Whether any filter is set. */
export function filtering(f: LiveSearch): boolean {
  return f.service !== undefined || f.outcome !== undefined || f.caller !== undefined || f.q !== undefined;
}

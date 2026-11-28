// Live's address: which view, which call's drawer is open, and the filters of the wire. Everything a person picks is here, so a
// reload or a link sent to someone shows the same thing. Anything that is not one of the known values is dropped: a hand-edited
// address can put no other state on screen.
import type { CallRecord } from "../../api/types/CallRecord";
import type { Outcome } from "../../api/types/Outcome";

export const VIEWS = ["map", "wire", "both"] as const;
export type View = (typeof VIEWS)[number];

/** The outcomes a filter can ask for: the contract's four, the calls the service refused (an error that is not the service's
 * failing: the answer was under 500) and the calls soft IAM let through that it would refuse. */
export const OUTCOME_FILTERS = ["ok", "error", "refused", "denied", "not-built", "would-be-denied"] as const;
export type OutcomeFilter = (typeof OUTCOME_FILTERS)[number];

/** The kinds of call that need a look (the chips of Needs a look, which filter the wire): a function that failed, an asynchronous
 * invocation accepted and dropped (the function's queue was full), a call soft IAM let through that AWS would refuse, an operation not
 * built, a denial, a refusal, a failure. */
export const LOOKS = ["function-error", "dropped", "would-be-denied", "not-built", "denied", "refused", "failed"] as const;
export type LookKind = (typeof LOOKS)[number];

/** Which kind of look `c` is, or null for a call that went well. A call one service made to another is not what an app did. */
export function lookOf(c: CallRecord): LookKind | null {
  if (c.via) return null;
  if (c.function_error) return "function-error";
  if (c.dropped) return "dropped";
  if (c.would_be_denied) return "would-be-denied";
  if (c.outcome === "not-built") return "not-built";
  if (c.outcome === "denied") return "denied";
  if (c.outcome === "error") return c.status > 0 && c.status < 500 ? "refused" : "failed";
  return null;
}

export type LiveSearch = {
  /** A chip of Needs a look pressed: only the calls of that kind are on the wire. */
  look?: LookKind;
  /** The call whose drawer is open. */
  call?: number;
  /** Map, Wire (the table) or both; Both when not given. */
  view?: View;
  /** The calls a Lab rule touched: `any` rule, or the rule with this number. */
  lab?: "any" | number;
  /** A service by its `--services` name. */
  service?: string;
  outcome?: OutcomeFilter;
  /** An access key, or for a call between services the service that made it. */
  caller?: string;
  /** Text looked for in the operation and the resource. */
  q?: string;
  /** `shown`: this console's own calls are shown too (hidden otherwise). */
  console?: "shown";
  /** An app by its name (an IAM user's): the calls of the key that is. */
  app?: string;
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
  const out: LiveSearch = { lab: undefined, look: undefined, call: undefined, view: undefined, service: undefined, outcome: undefined, caller: undefined, q: undefined, console: undefined, app: undefined, node: undefined, focus: undefined, layout: undefined, by: undefined };
  if (typeof n === "number" && Number.isSafeInteger(n) && n > 0) out.call = n;
  if (VIEWS.includes(s.view as View)) out.view = s.view as View;
  if (s.lab === "any") out.lab = "any";
  else if (typeof s.lab === "number" && Number.isSafeInteger(s.lab) && s.lab > 0) out.lab = s.lab;
  const service = text(s.service, 64);
  if (service && /^[a-z0-9-]+$/.test(service)) out.service = service;
  if (LOOKS.includes(s.look as LookKind)) out.look = s.look as LookKind;
  if (OUTCOME_FILTERS.includes(s.outcome as OutcomeFilter)) out.outcome = s.outcome as OutcomeFilter;
  const caller = text(s.caller, 128);
  if (caller) out.caller = caller;
  const q = text(s.q);
  if (q) out.q = q;
  const app = text(s.app, 64);
  if (app) out.app = app;
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
  if (f.look !== undefined && lookOf(c) !== f.look) return false;
  if (f.lab !== undefined && (c.lab === null || (f.lab !== "any" && c.lab.id !== f.lab))) return false;
  if (f.outcome !== undefined) {
    if (f.outcome === "would-be-denied" ? !c.would_be_denied : f.outcome === "refused" ? !(c.outcome === "error" && c.status > 0 && c.status < 500) : c.outcome !== (f.outcome as Outcome)) return false;
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
  return f.lab !== undefined || f.look !== undefined || f.service !== undefined || f.outcome !== undefined || f.caller !== undefined || f.q !== undefined || f.app !== undefined;
}

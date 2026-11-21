// What the Live record says of a bus and its rules beyond the resource's own recent calls. A call names a rule without its bus and a PutEvents
// the bus its first entry names, so a bus page also reads the calls that named one of its rules. A delivery a rule made (a Publish to a topic)
// is recorded with the rule that made it (`rule order-placed`) after the PutEvents that caused it has been answered; the record does not tie
// the two, so a PutEvents "matched" the rules that delivered for the same caller right after it and before the next PutEvents.
import { useQuery } from "@tanstack/react-query";
import { PATHS, client } from "../../../api/client";
import type { CallRecord } from "../../../api/types/CallRecord";
import type { CallsPage } from "../../../api/types/CallsPage";

/** The newest calls of the record (the page reads them when it opens and when refreshed). */
export function useRecord() {
  return useQuery({ queryKey: ["eventbridge", "record"], queryFn: ({ signal }) => client.api<CallsPage>(`${PATHS.calls}?limit=1000`, {}, signal), retry: false });
}

/** The rule a call made on EventBridge's behalf, by name; `null` for any other call. */
export const ruleOf = (c: Pick<CallRecord, "via">): string | null => (c.via?.by === "eventbridge" && c.via.name?.startsWith("rule ") ? c.via.name.slice(5) : null);

/** How long after a PutEvents a delivery still belongs to it (ms). */
export const DELIVERS_WITHIN = 3000;

/** The calls to EventBridge that named a rule of the bus (by name) joined to `own`, newest first, each once. */
export function busCalls(own: readonly CallRecord[], record: readonly CallRecord[], rules: readonly string[]): CallRecord[] {
  const names = new Set(rules);
  const seen = new Set(own.map((c) => c.seq));
  const more = record.filter((c) => c.service === "eventbridge" && c.resource !== null && names.has(c.resource) && !seen.has(c.seq));
  return [...own, ...more].sort((a, b) => b.seq - a.seq);
}

/** The deliveries a rule made, and the calls to EventBridge that named it, newest first. */
export function ruleCalls(own: readonly CallRecord[], record: readonly CallRecord[], rule: string): CallRecord[] {
  const seen = new Set(own.map((c) => c.seq));
  const made = record.filter((c) => ruleOf(c) === rule && !seen.has(c.seq));
  return [...own, ...made].sort((a, b) => b.seq - a.seq);
}

/** The rules that delivered right after the PutEvents `call`: same caller, within {@link DELIVERS_WITHIN}, before the next PutEvents. */
export function matchedRules(call: CallRecord, record: readonly CallRecord[]): string[] {
  if (call.action !== "PutEvents") return [];
  const next = record.filter((c) => c.action === "PutEvents" && c.seq > call.seq).reduce((m, c) => Math.min(m, c.seq), Number.POSITIVE_INFINITY);
  const names = record.filter((c) => c.seq > call.seq && c.seq < next && c.at_ms - call.at_ms <= DELIVERS_WITHIN && c.caller === call.caller).map(ruleOf).filter((n): n is string => n !== null);
  return [...new Set(names)];
}

/** What a result says beyond its status: `matched order-placed` for a PutEvents, what a delivery went to for a rule's own calls. */
export function resultNote(call: CallRecord, record: readonly CallRecord[]): string | null {
  if (call.action === "PutEvents" && call.status === 200) {
    const m = matchedRules(call, record);
    return m.length > 0 ? `matched ${m.join(", ")}` : "matched no rule";
  }
  if (ruleOf(call) && call.resource) return `to ${call.resource.split(/[/:]/).pop()}`;
  return null;
}

// Send events: the entry PutEvents takes, the whole event a rule's pattern is tested against, and the words for what the answer holds.
import type { PutEventsAnswer, RuleItem } from "./types";

export type SendForm = { bus: string; source: string; detailType: string; detail: string };
export const emptySend = (bus: string): SendForm => ({ bus, source: "larder.orders", detailType: "OrderPlaced", detail: '{\n  "order_id": "o-20931",\n  "customer_id": "c-1187",\n  "total_cents": 4850\n}' });

/** What is wrong with the entry, said on its field: a source, a detail type of at most 128 characters, and a detail that is a JSON object. */
export function sendProblems(f: SendForm): { source?: string; detailType?: string; detail?: string } {
  const out: { source?: string; detailType?: string; detail?: string } = {};
  if (f.source.trim() === "") out.source = "An entry needs a source.";
  if (f.detailType.trim() === "") out.detailType = "An entry needs a detail type.";
  else if (f.detailType.length > 128) out.detailType = "A detail type is at most 128 characters.";
  try {
    const v = JSON.parse(f.detail) as unknown;
    if (typeof v !== "object" || v === null || Array.isArray(v)) out.detail = "The detail is a JSON object.";
  } catch (e) {
    out.detail = `Not JSON: ${(e as Error).message}`;
  }
  return out;
}

/** PutEvents's input: one entry; the detail goes as JSON text, compact. */
export function putEventsInput(f: SendForm): { Entries: Record<string, string>[] } {
  let detail = f.detail;
  try {
    detail = JSON.stringify(JSON.parse(f.detail));
  } catch {
    /* the problem is said on the field */
  }
  return { Entries: [{ EventBusName: f.bus, Source: f.source, DetailType: f.detailType, Detail: detail }] };
}

/** The event EventBridge hands a rule, for TestEventPattern: what the form holds with the members a pattern may name. */
export function wholeEvent(f: SendForm, account: string, region: string, now = new Date()): string {
  let detail: unknown = {};
  try {
    detail = JSON.parse(f.detail);
  } catch {
    /* a detail that is not JSON matches nothing */
  }
  return JSON.stringify({ version: "0", id: "00000000-0000-4000-8000-000000000000", "detail-type": f.detailType, source: f.source, account, time: now.toISOString().replace(/\.\d+Z$/, "Z"), region, resources: [], detail });
}

/** What PutEvents answered, in words: the event's id, or the entry's error. */
export function sentWords(a: PutEventsAnswer): { ok: boolean; text: string } {
  const e = a.Entries?.[0];
  if (e?.EventId) return { ok: true, text: `PutEvents returned the EventId ${e.EventId}.` };
  return { ok: false, text: `PutEvents answered ${e?.ErrorCode ?? "an error"}${e?.ErrorMessage ? `: ${e.ErrorMessage}` : ""}` };
}

/** The hint under the detail: which rules of the bus the event matches, or where the rules it might match live. */
export function matchHint(bus: string, rulesOn: readonly RuleItem[], matched: readonly string[] | null, elsewhere: readonly { bus: string; rule: string }[]): string {
  if (matched === null) return "";
  if (matched.length > 0) return `This event matches ${matched.length === 1 ? "the rule" : "the rules"} ${matched.join(", ")} on ${bus}.`;
  if (rulesOn.length > 0) return `This event matches none of the ${rulesOn.length === 1 ? "rule" : "rules"} on ${bus}.`;
  const other = elsewhere[0];
  return other ? `The rule ${other.rule} lives on ${other.bus}, so this event on ${bus} matches none of your rules.` : `${bus} has no rules, so this event matches none.`;
}

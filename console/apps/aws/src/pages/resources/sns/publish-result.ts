import { matches, policyWords } from "./filter-match";
import { b, type Part } from "./sentences";
import type { Subscription } from "./sub-form-model";
import { endpointName, type SubRow } from "./sub-model";

// What the last Publish of a topic answered, kept for the topic's page the publish page opens (the answer is not in the topic's
// attributes: it is a MessageId, and for a FIFO topic a SequenceNumber).
export type Published = { messageId: string; sequenceNumber: string | null; group: string | null; dedup: string | null; attributes: Record<string, { type: "String" | "String.Array" | "Number" | "Binary"; value: string }>; custom: boolean; at: number };

const LAST = new Map<string, Published>();

export const remember = (topic: string, p: Published): void => void LAST.set(topic, p);
export const lastPublished = (topic: string): Published | null => LAST.get(topic) ?? null;

/** `The message went to 1 of 1 confirmed subscription: orders-events.fifo. It matched the filter policy on event.` `subs` are the topic's
 * confirmed subscriptions, `details` their settings in the same order (empty when they were not read). */
export function publishedSentence(p: Published, subs: readonly SubRow[], details: readonly Subscription[]): Part[] {
  const confirmed = subs.filter((s) => !s.pending);
  if (confirmed.length === 0) return ["The topic has no confirmed subscription, so the message reached nobody."];
  if (details.length !== confirmed.length) return [`The message was accepted for ${confirmed.length} confirmed ${confirmed.length === 1 ? "subscription" : "subscriptions"}.`];
  const verdicts = confirmed.map((s, i) => {
    const filter = details[i]!.filter;
    if (!filter) return { s, v: "match" as const, filter: null };
    if (details[i]!.scope === "MessageBody") return { s, v: "unknown" as const, filter };
    return { s, v: p.custom ? ("unknown" as const) : matches(filter, p.attributes), filter };
  });
  const hit = verdicts.filter((x) => x.v === "match");
  const unsure = verdicts.filter((x) => x.v === "unknown").length;
  const names = hit.map((x) => endpointName(x.s.protocol, x.s.endpoint));
  const out: Part[] = [p.group ? `The message in group ${p.group} went to ` : "The message went to ", b(`${hit.length} of ${confirmed.length}`), ` confirmed ${confirmed.length === 1 ? "subscription" : "subscriptions"}`];
  if (names.length > 0) out.push(": ", ...names.flatMap((n, i) => (i === 0 ? [b(n)] : [i === names.length - 1 ? " and " : ", ", b(n)])));
  out.push(".");
  const filtered = hit.filter((x) => x.filter);
  if (filtered.length > 0) out.push(` It matched the filter policy ${filtered.length === 1 ? `(${policyWords(filtered[0]!.filter!)})` : "of each"}.`);
  if (unsure > 0) out.push(` ${unsure === 1 ? "One subscription has" : `${unsure} subscriptions have`} a filter policy this page does not evaluate, so the service decided it.`);
  return out;
}

// The sentences of the SNS pages that the page builds from what it has read (the server's sentence says the counts of one topic; these
// join a list's rows with the subscriptions that wait for their endpoint). A sentence is parts: plain words and the names and numbers
// the board sets in bold. Pure.
import type { SubRow } from "./sub-model";
import { endpointName } from "./sub-model";

export type Part = string | { b: string };
export const b = (text: string): Part => ({ b: text });
/** The parts as one string (a test, a title). */
export const plain = (parts: readonly Part[]): string => parts.map((p) => (typeof p === "string" ? p : p.b)).join("");

const noun = (n: number, one: string, many: string): string => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

const BOTH: Record<number, string> = { 1: "", 2: "both" };

/** `the webhook X` for an http endpoint; `the stored email X` for an email address. */
function pendingWho(s: SubRow): Part[] {
  const url = s.protocol === "http" || s.protocol === "https";
  const mail = s.protocol === "email" || s.protocol === "email-json";
  return url ? ["the webhook ", b(s.endpoint)] : mail ? ["the stored email ", b(s.endpoint)] : ["the ", s.protocol, " endpoint ", b(endpointName(s.protocol, s.endpoint))];
}

/** Webhooks first, then addresses: the order a person thinks of what is waiting. */
const byKind = (rows: SubRow[]): SubRow[] => [...rows].sort((a, b) => Number(a.protocol.startsWith("email")) - Number(b.protocol.startsWith("email")));

const join = (lists: Part[][], last = " and "): Part[] => lists.flatMap((l, i) => (i === 0 ? l : [i === lists.length - 1 ? last : ", ", ...l]));

/** Where the pending ones are: `, both on alerts`. */
function where(pending: readonly SubRow[]): Part[] {
  const topics = [...new Set(pending.map((s) => s.topic))];
  if (topics.length > 1) return [" on ", ...join(topics.map((t) => [b(t)]))];
  const all = pending.length === 1 ? "" : (BOTH[pending.length] ?? "all");
  return [all ? `, ${all} on ` : " on ", b(topics[0] ?? "")];
}

/** The topics list: `3 topics and 7 subscriptions in us-east-1. 2 are pending, both on alerts: …. Last publish: orders-created at 08:13:58.` */
export function topicsSentence(topics: number, subs: readonly SubRow[], region: string, lastPublish: { topic: string; at: string } | null): Part[] {
  if (topics === 0) return [`No topics in ${region}: ListTopics returned an empty list. A publish needs a topic, and a topic needs a subscription before anything is delivered.`];
  const pending = byKind(subs.filter((s) => s.pending));
  const out: Part[] = [b(noun(topics, "topic", "topics")), " and ", b(noun(subs.length, "subscription", "subscriptions")), ` in ${region}.`];
  if (pending.length > 0) {
    out.push(" ", b(String(pending.length)), pending.length === 1 ? " is pending" : " are pending", ...where(pending), ": ", ...join(pending.map(pendingWho)));
    if (pending.some((s) => s.protocol === "email" || s.protocol === "email-json")) out.push(", which stays pending");
    out.push(".");
  }
  if (lastPublish) out.push(" Last publish: ", b(lastPublish.topic), ` at ${lastPublish.at}.`);
  return out;
}

/** The subscriptions list: `7 subscriptions, 5 confirmed and 2 pending, both on alerts: …`. */
export function subscriptionsSentence(subs: readonly SubRow[]): Part[] {
  const pending = byKind(subs.filter((s) => s.pending));
  const out: Part[] = [b(noun(subs.length, "subscription", "subscriptions")), ", "];
  if (pending.length === 0) return [...out, subs.length === 1 ? "confirmed." : "all confirmed."];
  out.push(`${subs.length - pending.length} confirmed and ${pending.length} pending`, ...where(pending), ": ");
  const why = pending.map((s): Part[] => (s.protocol === "email" || s.protocol === "email-json" ? [b(s.endpoint), " is stored and stays pending because doze-aws sends no email"] : [...pendingWho(s), " is waiting for its confirmation"]));
  return [...out, ...join(why, ", and "), "."];
}

/** After a delete: `2 topics left in us-east-1. A publish to orders-created now gets NotFound.` */
export function topicsLeftSentence(left: number, region: string, gone: string): Part[] {
  return [b(`${noun(left, "topic", "topics")} left`), ` in ${region}. A publish to `, b(gone), " now gets NotFound: the topic does not exist."];
}

/** `3 confirmed subscriptions: invoice-queue, …` is the server's; this is the clause the page adds once it has read each subscription. */
export function rawWords(raw: readonly boolean[], protocols: readonly string[]): string | null {
  if (raw.length === 0 || raw.length !== protocols.length) return null;
  const all = raw.every((r) => !r);
  const none = raw.every((r) => r);
  if (!all && !none) return null;
  const n = raw.length;
  const every = n === 1 ? "it" : n === 2 ? "both" : `all ${["zero", "one", "two", "three", "four", "five"][n] ?? n}`;
  const who = protocols.every((p) => p === "sqs") ? (n === 1 ? "the queue receives" : "each queue receives") : n === 1 ? "it receives" : "each receives";
  return all ? `Raw message delivery is off on ${every}, so ${who} the full JSON envelope.` : `Raw message delivery is on for ${every}, so ${who} the bare message body.`;
}

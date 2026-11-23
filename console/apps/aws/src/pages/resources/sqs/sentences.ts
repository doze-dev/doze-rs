// The sentence under a queue's name when the page knows more than the server's sentence does: what it was just made, emptied,
// or what it is the dead-letter queue of (the source's own limit). Pure.
import type { Wired } from "../../../api/types/Wired";
import { messages } from "./queue-model";

/** `pick-lists-dlq ... It is the dead-letter queue of pick-lists, which sends a message here once its receive count exceeds 5.` */
export function sourceSentence(server: string | null, sources: readonly string[], maxReceives: number | null): string | null {
  if (!server || sources.length !== 1 || maxReceives === null) return server;
  const tail = `dead-letter queue of ${sources[0]}.`;
  return server.endsWith(tail) ? `${server.slice(0, -1)}, which sends a message here once its receive count exceeds ${maxReceives}.` : server;
}

/** A queue just made: nothing in it yet, and what its first message needs. */
export function madeSentence(name: string, fifo: boolean): string {
  return fifo ? `${name} is empty. Every message you send needs a message group ID; messages in one group are delivered in the order they were sent.` : `${name} is empty. Send it a message, or point an app at its URL.`;
}

/** A queue just purged. */
export const purgedSentence = (name: string, available: number, inFlight: number): string => `${name} was purged: ${messages(available)} waiting and ${inFlight} in flight. The queue and its settings are unchanged.`;

/** What flows into a queue (the rest of its Wired to strip flows out of it). */
export const UPSTREAM = ["Fed by", "Queues that use it"];

/** A chip's words, as the board sets them: `payment-settled rule, bus larder`, `alias/larder-app key`. */
export const chipWords = (w: Pick<Wired, "label" | "to">): string => (w.label === "Fed by" ? ruleWords(w.to.name) : w.label === "KMS key" ? `${w.to.name} key` : w.to.name);
const ruleWords = (n: string): string => (n.includes("/") ? `${n.slice(n.indexOf("/") + 1)} rule, bus ${n.slice(0, n.indexOf("/"))}` : `${n} rule`);


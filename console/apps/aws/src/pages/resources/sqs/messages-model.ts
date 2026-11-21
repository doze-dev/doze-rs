// What a poll returned and what the page says about it (sqs__queue-messages-polled, message-detail, message-deleted). Pure.
import { bytes, dateTimeUtc } from "@doze/ui/lib/format";
import { messages } from "./queue-model";

export type Message = {
  MessageId: string;
  ReceiptHandle: string;
  Body: string;
  MD5OfBody?: string;
  Attributes?: Record<string, string>;
  MessageAttributes?: Record<string, { DataType: string; StringValue?: string; BinaryValue?: string }>;
};

/** A poll that hides nothing from the queue's consumers (a visibility timeout of 0) and never waits. */
export const POLL = { MessageSystemAttributeNames: ["All"], MessageAttributeNames: ["All"] };
export const MOST = 10;

export const receives = (m: Message): number => Number(m.Attributes?.ApproximateReceiveCount ?? 0);
export const sizeOf = (m: Message): string => bytes(new TextEncoder().encode(m.Body).length);
export function sentOf(m: Message): string {
  const at = Number(m.Attributes?.SentTimestamp);
  return Number.isFinite(at) && at > 0 ? dateTimeUtc(at) : "–";
}

/** The maximum count as the call takes it: 1 to 10, or the words of what is wrong. */
export function countOf(text: string): number | string {
  const n = Number(text);
  return Number.isInteger(n) && n >= 1 && n <= MOST ? n : "The most a poll returns is 10, and at least 1.";
}

/** The messages that are near the dead-letter queue's limit: each with the words that say how near. */
export function nearLimit(ms: readonly Message[], dlq: string | null, max: number | null, queue: string): string[] {
  if (!dlq || max === null) return [];
  return ms.filter((m) => receives(m) >= Math.max(2, max - 2)).map((m) => `${m.MessageId.slice(0, 8)}... has been received ${receives(m)} times. ${queue} moves a message to ${dlq} once its receive count exceeds ${max}. The console's own polling counts as a receive.`);
}

/** `The poll returned all 3 waiting messages. 1 more is in flight with another consumer.` */
export function polledSentence(got: number, waiting: number, inFlight: number): string {
  const all = got > 0 && got >= waiting ? "all " : "";
  return `The poll returned ${got === 0 ? "no messages" : `${all}${messages(got).replace(/^1 message$/, "1 waiting message").replace(/ messages$/, " waiting messages")}`}.${inFlight > 0 ? ` ${inFlight === 1 ? "1 more is" : `${inFlight} more are`} in flight with another consumer.` : ""}`;
}

/** After a delete: what is left of the poll. */
export function deletedSentence(n: number, queue: string, left: number): string {
  return `Deleted ${messages(n)} from ${queue}. It is gone for good${left > 0 ? `; the ${left === 1 ? "other message" : `other ${left} messages`} from this poll ${left === 1 ? "is" : "are"} untouched` : ""}.`;
}

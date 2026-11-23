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
/** `53 bytes`, then `1.2 KiB` once it is a kibibyte or more. */
export function sizeOf(m: Message): string {
  const n = new TextEncoder().encode(m.Body).length;
  return n < 1024 ? `${n} ${n === 1 ? "byte" : "bytes"}` : bytes(n);
}
export function sentOf(m: Message): string {
  const at = Number(m.Attributes?.SentTimestamp);
  return Number.isFinite(at) && at > 0 ? dateTimeUtc(at) : "–";
}

/** The maximum count as the call takes it: 1 to 10, or the words of what is wrong. */
export function countOf(text: string): number | string {
  const n = Number(text);
  return Number.isInteger(n) && n >= 1 && n <= MOST ? n : "The most a poll returns is 10, and at least 1.";
}

/** The one warning for the messages that are near the dead-letter queue's limit: a bold lead that says which and how near, then what
 * happens at the limit. Null when none is near. */
export function nearLimit(ms: readonly Message[], dlq: string | null, max: number | null, queue: string): { lead: string; text: string } | null {
  if (!dlq || max === null) return null;
  const near = ms.filter((m) => receives(m) >= Math.max(2, max - 2));
  if (near.length === 0) return null;
  const lead = near.length === 1 ? `${near[0]!.MessageId.slice(0, 8)}... has been received ${receives(near[0]!)} times.` : `${near.length} messages are close to the limit: ${near.map((m) => `${m.MessageId.slice(0, 8)}... (${receives(m)} times)`).join(", ")}.`;
  return { lead, text: `${queue} moves a message to ${dlq} once its receive count exceeds ${max}. The console's own polling counts as a receive.` };
}

/** `The poll returned all 3 waiting messages. 1 more is in flight with another consumer.` */
export function polledSentence(got: number, waiting: number, inFlight: number): string {
  const all = got > 0 && got >= waiting ? "all " : "";
  return `The poll returned ${got === 0 ? "no messages" : `${all}${messages(got).replace(/^1 message$/, "1 waiting message").replace(/ messages$/, " waiting messages")}`}.${inFlight > 0 ? ` ${inFlight === 1 ? "1 more is" : `${inFlight} more are`} in flight with another consumer.` : ""}`;
}

/** What is left of a poll once some of its messages were deleted: `2 messages are left from the last poll.` */
export const leftSentence = (left: number): string => (left === 0 ? "No message is left from the last poll." : `${messages(left)} ${left === 1 ? "is" : "are"} left from the last poll.`);

const WORDS = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];

/** After a delete, the banner: what went, and what the poll still holds (`the other two messages from this poll are untouched`). */
export function deletedBanner(n: number, queue: string, left: number): { title: string; returned: string } {
  const rest = left > 0 ? `; the other ${left === 1 ? "message" : `${WORDS[left] ?? left} messages`} from this poll ${left === 1 ? "is" : "are"} untouched` : "";
  return { title: `Deleted ${messages(n)} from ${queue}.`, returned: `${n === 1 ? "It is" : "They are"} gone for good${rest}.` };
}

/** The note under the table after a delete: how near the message was to the dead-letter queue, and where the receipt handle came from. */
export function deletedNote(received: number | null, dlq: string | null): string {
  const near = received !== null && dlq ? `The deleted message was received ${received} times and would have gone to ${dlq} on its next failed receives. ` : "";
  return `${near}Deleting needs the receipt handle from its most recent receive, which this poll supplied.`;
}

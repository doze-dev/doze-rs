// A queue as its page reads it: the console's detail fields, typed. Pure, so the sentences and rules built on it are tested.
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { seconds } from "../model";

export type Encryption = "sqs" | "kms" | "none";

export type Queue = {
  name: string;
  fifo: boolean;
  url: string;
  arn: string;
  available: number;
  inFlight: number;
  delayed: number;
  dlq: string | null;
  maxReceives: number | null;
  visibility: number;
  retention: number;
  delay: number;
  maxSize: number;
  wait: number;
  dedup: boolean;
  sse: Encryption;
  kms: string | null;
  reuse: number | null;
  scope: string | null;
  throughput: string | null;
  created: string | null;
  modified: string | null;
  policy: string | null;
  allow: string | null;
};

const num = (v: string | null | undefined, fallback = 0): number => {
  const n = Number(v);
  return v !== null && v !== undefined && v !== "" && Number.isFinite(n) ? n : fallback;
};

/** The queue a detail describes. A field the service did not give is its default (the numbers) or none. */
export function queueOf(d: ResourceDetail): Queue {
  const f = (label: string): string | null => {
    const v = d.fields.find((x) => x.label === label)?.value;
    return v === undefined || v === "" ? null : v;
  };
  const kms = f("KMS key");
  const managed = f("SQS-managed encryption") === "Enabled";
  const asNum = (label: string, fallback: number) => num(f(label), fallback);
  return {
    name: d.name,
    fifo: f("Type") === "FIFO",
    url: f("URL") ?? d.id,
    arn: f("ARN") ?? "",
    available: asNum("Messages available", 0),
    inFlight: asNum("Messages in flight", 0),
    delayed: asNum("Messages delayed", 0),
    dlq: f("Dead-letter queue"),
    maxReceives: f("Maximum receives") === null ? null : asNum("Maximum receives", 0),
    visibility: asNum("Visibility timeout", 30),
    retention: asNum("Message retention period", 345_600),
    delay: asNum("Delivery delay", 0),
    maxSize: asNum("Maximum message size", 1_048_576),
    wait: asNum("Receive message wait time", 0),
    dedup: f("Content-based deduplication") === "Enabled",
    // The service reports SSE-SQS as on whether or not a KMS key is set; the key says which one the queue uses.
    sse: kms ? "kms" : managed ? "sqs" : "none",
    kms,
    reuse: f("Data key reuse period") === null ? null : asNum("Data key reuse period", 300),
    scope: f("Deduplication scope"),
    throughput: f("FIFO throughput limit"),
    created: f("Created"),
    modified: f("Last modified"),
    policy: f("Access policy"),
    allow: f("Redrive allow policy"),
  };
}

/** `1,024 KiB`: a size in bytes the way the form takes it. */
export const kib = (bytes: number): string => `${(bytes / 1024).toLocaleString("en-US", { maximumFractionDigits: 1 })} KiB`;

export const messages = (n: number): string => `${n.toLocaleString("en-US")} ${n === 1 ? "message" : "messages"}`;

/** What a purge left: `All 3 messages are gone.`, `The 1 message is gone.`, `No message was left to remove.` */
export const goneWords = (n: number): string => (n === 0 ? "No message was left to remove." : n === 1 ? "The 1 message is gone." : `All ${messages(n)} are gone.`);

/** What the encryption setting is, in words. */
export function encryptionWords(q: Pick<Queue, "sse" | "kms">): string {
  return q.sse === "kms" ? `Enabled with an AWS Key Management Service key (SSE-KMS), ${q.kms}.` : q.sse === "sqs" ? "Enabled with an Amazon SQS key (SSE-SQS)." : "Disabled.";
}

/** The Encryption column of the list: on when SSE-SQS is or a KMS key is set. */
export const encrypted = (managed: string | null, kms: string | null): boolean => managed === "Enabled" || Boolean(kms);

/** The sentence under a queue's name: the server's, with where its failures go told in full once the dead-letter queue has been read
 * (`pick-lists-dlq, which holds 1 now`). */
export function queueSentence(server: string | null, q: Pick<Queue, "name" | "dlq" | "maxReceives">, dlqHolds: number | null): string | null {
  if (!server) return null;
  if (!q.dlq || q.maxReceives === null || dlqHolds === null) return server;
  const tail = ` A message received more than ${q.maxReceives} times moves to ${q.dlq}.`;
  if (!server.endsWith(tail)) return server;
  return `${server.slice(0, -tail.length)} A message received more than ${q.maxReceives} times moves to ${q.dlq}, which holds ${dlqHolds} now.`;
}

/** The list's sentence once a queue was deleted: `6 queues remain, holding 56 messages waiting and 5 in flight.` */
export function remainWords(sentence: string | null): string | null {
  if (!sentence) return null;
  return sentence.replace(/^(\d[\d,]*) (queues?) holds?\b/, (_, n: string, q: string) => `${n} ${q} ${n === "1" ? "remains" : "remain"}, holding`);
}

/** How long SQS keeps a message, as the form's unit and amount (`4 days`): the largest whole unit. */
export function inUnits(secs: number): { value: number; unit: Unit } {
  for (const [size, unit] of [[86_400, "days"], [3_600, "hours"], [60, "minutes"]] as const) if (secs >= size && secs % size === 0) return { value: secs / size, unit };
  return { value: secs, unit: "seconds" };
}

export type Unit = "seconds" | "minutes" | "hours" | "days";
export const UNIT_SECONDS: Record<Unit, number> = { seconds: 1, minutes: 60, hours: 3_600, days: 86_400 };

/** The seconds a time the form holds comes to; `null` when it is not a number. */
export function toSeconds(value: string, unit: Unit): number | null {
  const n = Number(value.trim());
  return value.trim() !== "" && Number.isFinite(n) && n >= 0 ? Math.round(n * UNIT_SECONDS[unit]) : null;
}

export const wait = (secs: number): string => seconds(String(secs));

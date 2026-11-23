// A topic as its page reads it: the console's detail fields, typed. Pure, so the readings built on it are tested.
import type { ResourceDetail } from "../../../api/types/ResourceDetail";

export type Feedback = { success: string | null; rate: number | null; failure: string | null };
/** The protocols whose delivery status a topic logs (doze-aws keeps these three; Firehose and mobile push are not here). */
export const LOGGED = ["HTTP", "Lambda", "SQS"] as const;
export type Logged = (typeof LOGGED)[number];

export type Topic = {
  name: string;
  arn: string;
  fifo: boolean;
  display: string | null;
  owner: string | null;
  confirmed: number;
  pending: number;
  dedup: boolean;
  scope: "Topic" | "MessageGroup";
  /** Bytes; the service's default (256 KiB) when the topic sets none. */
  maxSize: number;
  maxSizeSet: boolean;
  kms: string | null;
  tracing: boolean;
  /** The ArchivePolicy JSON, as stored. */
  archive: string | null;
  policy: string | null;
  /** The delivery policy in force (the defaults with the topic's own laid over them), and the topic's own. */
  effective: string | null;
  own: string | null;
  feedback: Record<Logged, Feedback>;
};

export const DEFAULT_MAX_SIZE = 262_144;

const num = (v: string | null): number | null => {
  if (v === null || v === "") return null;
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
};

/** The topic a detail describes. A field the service did not give is its default, or none. */
export function topicOf(d: ResourceDetail): Topic {
  const f = (label: string): string | null => {
    const v = d.fields.find((x) => x.label === label)?.value;
    return v === undefined || v === "" ? null : v;
  };
  const feedback = (p: Logged): Feedback => ({ success: f(`${p}SuccessFeedbackRoleArn`), rate: num(f(`${p}SuccessFeedbackSampleRate`)), failure: f(`${p}FailureFeedbackRoleArn`) });
  const size = num(f("Maximum message size"));
  return {
    name: d.name,
    arn: f("ARN") ?? d.id,
    fifo: f("Type") === "FIFO",
    display: f("Display name"),
    owner: f("Owner"),
    confirmed: num(f("Confirmed subscriptions")) ?? 0,
    pending: num(f("Pending subscriptions")) ?? 0,
    dedup: f("Content-based deduplication") === "Enabled",
    scope: f("FIFO throughput scope") === "MessageGroup" ? "MessageGroup" : "Topic",
    maxSize: size ?? DEFAULT_MAX_SIZE,
    maxSizeSet: size !== null,
    kms: f("KMS key"),
    tracing: f("Active tracing") === "Active",
    archive: f("Archive policy"),
    policy: f("Access policy"),
    effective: f("Delivery policy"),
    own: f("Own delivery policy"),
    feedback: { HTTP: feedback("HTTP"), Lambda: feedback("Lambda"), SQS: feedback("SQS") },
  };
}

const parse = (text: string | null): Record<string, unknown> | null => {
  if (!text) return null;
  try {
    const v: unknown = JSON.parse(text);
    return typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
  } catch {
    return null;
  }
};

export type PolicyRow = { attribute: string; value: string; constraint: string };

const at = (doc: Record<string, unknown> | null, ...path: string[]): unknown => path.reduce<unknown>((o, k) => (typeof o === "object" && o !== null ? (o as Record<string, unknown>)[k] : undefined), doc);

/** The retry policy's attributes, with each value and the rule it follows: what the delivery-policy tab sets out. */
export function deliveryRows(effective: string | null): PolicyRow[] {
  const doc = parse(effective);
  const retry = (k: string): unknown => at(doc, "http", "defaultHealthyRetryPolicy", k);
  const n = (k: string): string => (typeof retry(k) === "number" ? String(retry(k)) : "–");
  const secs = (k: string): string => (typeof retry(k) === "number" ? `${retry(k)} seconds` : "–");
  const rate = at(doc, "http", "defaultThrottlePolicy", "maxReceivesPerSecond");
  const type = at(doc, "http", "defaultRequestPolicy", "headerContentType");
  return [
    { attribute: "numRetries", value: n("numRetries"), constraint: "0 to 100; the total number of retries, including immediate, pre-backoff, backoff, and post-backoff retries" },
    { attribute: "numNoDelayRetries", value: n("numNoDelayRetries"), constraint: "0 or greater" },
    { attribute: "minDelayTarget", value: secs("minDelayTarget"), constraint: "1 to the maximum delay" },
    { attribute: "numMinDelayRetries", value: n("numMinDelayRetries"), constraint: "0 or greater; the pre-backoff phase" },
    { attribute: "maxDelayTarget", value: secs("maxDelayTarget"), constraint: "The minimum delay to 3,600" },
    { attribute: "numMaxDelayRetries", value: n("numMaxDelayRetries"), constraint: "0 or greater; the post-backoff phase" },
    { attribute: "backoffFunction", value: typeof retry("backoffFunction") === "string" ? String(retry("backoffFunction")) : "–", constraint: "arithmetic, exponential, geometric or linear" },
    { attribute: "maxReceivesPerSecond", value: typeof rate === "number" ? String(rate) : "No throttling", constraint: "1 or greater; the maximum average number of deliveries per second, per subscription" },
    { attribute: "headerContentType", value: typeof type === "string" ? type : "–", constraint: "application/json or text/plain when the policy is defined on the topic" },
  ];
}

/** Whether the topic sets a delivery policy of its own. */
export const hasOwnDelivery = (own: string | null): boolean => parse(own) !== null && Object.keys(parse(own) ?? {}).length > 0;

/** Whether delivery to the protocol is logged: a role set is the switch (the role itself gates nothing). */
export const logged = (fb: Feedback): boolean => fb.success !== null || fb.failure !== null;

/** `role/sns-delivery-logs` for a role's ARN. */
export const roleName = (arn: string): string => (arn.includes(":role/") ? `role/${arn.slice(arn.indexOf(":role/") + 6)}` : arn);

/** The archive policy as a status and a retention: `active, 7 days`. Empty `{}` is inactive. */
export function archiveOf(text: string | null): { active: boolean; days: number | null } {
  const doc = parse(text);
  const days = Number(doc?.MessageRetentionPeriod);
  const active = doc !== null && Number.isFinite(days) && days > 0;
  return { active, days: active ? days : null };
}

/** `7 days`, `1 day`. */
export const daysWords = (n: number): string => `${n.toLocaleString("en-US")} ${n === 1 ? "day" : "days"}`;

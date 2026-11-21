// What a refused CreateQueue says of the queue that is already there: the attribute it names, and what that queue has.
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import type { ValueFormat } from "../../../api/types/ValueFormat";
import { shown } from "../model";

/** The attributes CreateQueue compares (`differs_from` in crates/aws-sqs/src/attrs.rs), and the detail field that holds each. */
const FIELDS: Record<string, [label: string, format: ValueFormat]> = {
  VisibilityTimeout: ["Visibility timeout", "seconds"],
  MessageRetentionPeriod: ["Message retention period", "seconds"],
  DelaySeconds: ["Delivery delay", "seconds"],
  ReceiveMessageWaitTimeSeconds: ["Receive message wait time", "seconds"],
  MaximumMessageSize: ["Maximum message size", "bytes"],
  ContentBasedDeduplication: ["Content-based deduplication", "text"],
  KmsMasterKeyId: ["KMS key", "mono"],
  KmsDataKeyReusePeriodSeconds: ["Data key reuse period", "seconds"],
  DeduplicationScope: ["Deduplication scope", "mono"],
  FifoThroughputLimit: ["FIFO throughput limit", "mono"],
  Policy: ["Access policy", "json"],
  RedrivePolicy: ["Dead-letter queue", "mono"],
  RedriveAllowPolicy: ["Redrive allow policy", "json"],
};

/** The attribute a QueueNameExists refusal names (`... a different value for attribute VisibilityTimeout`). */
export function mismatched(message: string): string | null {
  return /different value for attribute (\w+)/.exec(message)?.[1] ?? null;
}

/** `VisibilityTimeout 30 seconds`: the attribute and what the existing queue has, as its page says it. */
export function whatItHas(attribute: string, existing: ResourceDetail): { name: string; value: string } | null {
  const [label, format] = FIELDS[attribute] ?? [];
  if (!label || !format) return null;
  const v = existing.fields.find((f) => f.label === label)?.value;
  return { name: attribute, value: v ? shown(v, format) : "not set" };
}

/** Whether the code is the one a name taken with other attributes gets (the JSON protocol's name, and the query-compatible one). */
export const nameTaken = (code: string | undefined): boolean => code === "QueueNameExists" || code === "QueueAlreadyExists";

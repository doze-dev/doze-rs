// Create topic and Edit topic: the form's state, the attributes it sends, and what a change of each setting is. Pure.
// SetTopicAttributes takes one attribute a call, so an edit is the list of the attributes that changed; CreateTopic takes them all
// at once, and only those that differ from the service's defaults are sent.
import { filled, type KeyValueRow, rowsFrom } from "@doze/ui/components/app/key-values";
import type { Tag } from "../../../kit";
import { archiveOf, DEFAULT_MAX_SIZE, LOGGED, type Logged, type Topic } from "./topic-model";

export type Retry = { numRetries: string; numNoDelayRetries: string; minDelayTarget: string; numMinDelayRetries: string; maxDelayTarget: string; numMaxDelayRetries: string; backoffFunction: string; maxReceives: string };
export type LogForm = { on: boolean; rate: string; success: string; failure: string };

export type TopicForm = {
  fifo: boolean;
  name: string;
  display: string;
  dedup: boolean;
  scope: "Topic" | "MessageGroup";
  maxKib: string;
  sse: boolean;
  kms: string;
  /** The JSON of the access policy; empty leaves the service's default. */
  policy: string;
  retry: Retry;
  logging: Record<Logged, LogForm>;
  tracing: boolean;
  /** Days an archive policy keeps messages (a FIFO topic's); empty is none. */
  archive: string;
  tags: KeyValueRow[];
};

export const DEFAULT_RETRY: Retry = { numRetries: "3", numNoDelayRetries: "0", minDelayTarget: "20", numMinDelayRetries: "0", maxDelayTarget: "20", numMaxDelayRetries: "0", backoffFunction: "linear", maxReceives: "" };
const NO_LOG: LogForm = { on: false, rate: "100", success: "", failure: "" };

export const emptyTopicForm = (): TopicForm => ({ fifo: false, name: "", display: "", dedup: false, scope: "Topic", maxKib: "256", sse: false, kms: "", policy: "", retry: { ...DEFAULT_RETRY }, logging: { HTTP: { ...NO_LOG }, Lambda: { ...NO_LOG }, SQS: { ...NO_LOG } }, tracing: false, archive: "", tags: [] });

/** The policy SNS gives a new topic: any principal of the owner's account may use the account-level actions (as `GetTopicAttributes`
 * reads it back from doze-aws, crates/aws-sns/src/topic_attrs.rs). */
export function defaultPolicy(name: string, region: string, account: string): string {
  return JSON.stringify(
    {
      Version: "2008-10-17",
      Id: "__default_policy_ID",
      Statement: [{ Sid: "__default_statement_ID", Effect: "Allow", Principal: { AWS: "*" }, Action: ["SNS:Subscribe", "SNS:ListSubscriptionsByTopic", "SNS:DeleteTopic", "SNS:GetTopicAttributes", "SNS:Publish", "SNS:RemovePermission", "SNS:AddPermission", "SNS:SetTopicAttributes"], Resource: `arn:aws:sns:${region}:${account}:${name || "<topic-name>"}`, Condition: { StringEquals: { "AWS:SourceOwner": account } } }],
    },
    null,
    2,
  );
}

const norm = (text: string): string => {
  try {
    return JSON.stringify(JSON.parse(text));
  } catch {
    return text.trim();
  }
};

/** The retry settings of an effective delivery policy (JSON text); the defaults for what it does not name. */
export function retryOf(effective: string | null): Retry {
  try {
    const http = (JSON.parse(effective ?? "{}") as { http?: { defaultHealthyRetryPolicy?: Record<string, unknown>; defaultThrottlePolicy?: { maxReceivesPerSecond?: unknown } } }).http;
    const r = http?.defaultHealthyRetryPolicy ?? {};
    const pick = (k: keyof Retry): string => (r[k] === undefined ? DEFAULT_RETRY[k] : String(r[k]));
    const rate = http?.defaultThrottlePolicy?.maxReceivesPerSecond;
    return { numRetries: pick("numRetries"), numNoDelayRetries: pick("numNoDelayRetries"), minDelayTarget: pick("minDelayTarget"), numMinDelayRetries: pick("numMinDelayRetries"), maxDelayTarget: pick("maxDelayTarget"), numMaxDelayRetries: pick("numMaxDelayRetries"), backoffFunction: pick("backoffFunction"), maxReceives: rate === undefined ? "" : String(rate) };
  } catch {
    return { ...DEFAULT_RETRY };
  }
}

export const sameRetry = (a: Retry, b: Retry): boolean => (Object.keys(a) as (keyof Retry)[]).every((k) => a[k].trim() === b[k].trim());

/** The DeliveryPolicy JSON for these settings. */
export function deliveryPolicy(r: Retry): string {
  const n = (v: string): number => Number(v);
  const http: Record<string, unknown> = { defaultHealthyRetryPolicy: { minDelayTarget: n(r.minDelayTarget), maxDelayTarget: n(r.maxDelayTarget), numRetries: n(r.numRetries), numMaxDelayRetries: n(r.numMaxDelayRetries), numNoDelayRetries: n(r.numNoDelayRetries), numMinDelayRetries: n(r.numMinDelayRetries), backoffFunction: r.backoffFunction } };
  if (r.maxReceives.trim() !== "") http.defaultThrottlePolicy = { maxReceivesPerSecond: n(r.maxReceives) };
  return JSON.stringify({ http });
}

/** The form of an existing topic, for its edit page. */
export function formOfTopic(t: Topic, tags: readonly Tag[], region: string, account: string): TopicForm {
  const log = (p: Logged): LogForm => {
    const fb = t.feedback[p];
    return { on: fb.success !== null || fb.failure !== null, rate: fb.rate === null ? "100" : String(fb.rate), success: fb.success ?? "", failure: fb.failure ?? "" };
  };
  return {
    fifo: t.fifo,
    name: t.name,
    display: t.display ?? "",
    dedup: t.dedup,
    scope: t.scope,
    maxKib: String(t.maxSize / 1024),
    sse: t.kms !== null,
    kms: t.kms ?? "",
    policy: t.policy ? JSON.stringify(JSON.parse(t.policy), null, 2) : defaultPolicy(t.name, region, account),
    retry: retryOf(t.effective),
    logging: { HTTP: log("HTTP"), Lambda: log("Lambda"), SQS: log("SQS") },
    tracing: t.tracing,
    archive: String(archiveOf(t.archive).days ?? ""),
    tags: rowsFrom(tags),
  };
}

/** Whether the access policy typed is not a JSON object yet: none of it is put in a call (a half-typed policy is no member of one), and the
 * form cannot be saved until it is. */
export function policyBroken(f: Pick<TopicForm, "policy">): boolean {
  if (f.policy.trim() === "") return false;
  try {
    const doc: unknown = JSON.parse(f.policy);
    return typeof doc !== "object" || doc === null || Array.isArray(doc);
  } catch {
    return true;
  }
}

export type Problems = Partial<Record<"name" | "maxKib" | "kms" | "policy" | "retry" | "logging" | "archive" | "display", string>>;

const whole = (v: string, min: number, max: number): boolean => /^\d+$/.test(v.trim()) && Number(v) >= min && Number(v) <= max;

/** What the form can see wrong before the service is asked. A FIFO name without `.fifo` is not one of them on create: the service
 * refuses it, and the page shows why (sns__topic-create-refused). */
export function problems(f: TopicForm, create: boolean): Problems {
  const p: Problems = {};
  if (create) {
    if (f.name.trim() === "") p.name = "Enter a name.";
    else if (!/^[A-Za-z0-9_-]+(\.fifo)?$/.test(f.name) || f.name.length > 256) p.name = "1 to 256 characters: letters, numbers, underscores and hyphens.";
  }
  if (f.display.length > 100) p.display = "A display name is up to 100 characters.";
  if (!whole(f.maxKib, 1, 1024)) p.maxKib = "A whole number of KiB, 1 to 1,024.";
  if (f.sse && f.kms.trim() === "") p.kms = "Choose a KMS key, or turn encryption off.";
  if (f.policy.trim() !== "") {
    try {
      const doc: unknown = JSON.parse(f.policy);
      if (typeof doc !== "object" || doc === null || Array.isArray(doc)) p.policy = "A policy is a JSON object.";
    } catch (e) {
      p.policy = `Not JSON: ${(e as Error).message}`;
    }
  }
  const r = f.retry;
  const max = Number(r.maxDelayTarget);
  if (!whole(r.numRetries, 0, 100)) p.retry = "numRetries is 0 to 100.";
  else if (![r.numNoDelayRetries, r.numMinDelayRetries, r.numMaxDelayRetries].every((v) => whole(v, 0, 1_000_000))) p.retry = "The retry counts are whole numbers, 0 or greater.";
  else if (!whole(r.maxDelayTarget, 1, 3600) || !whole(r.minDelayTarget, 1, Number.isFinite(max) ? max : 3600)) p.retry = "minDelayTarget is 1 to the maximum delay, and maxDelayTarget is up to 3,600 seconds.";
  else if (r.maxReceives.trim() !== "" && !whole(r.maxReceives, 1, 1_000_000)) p.retry = "maxReceivesPerSecond is 1 or greater, or empty for no limit.";
  for (const l of LOGGED) {
    const g = f.logging[l];
    if (g.on && !whole(g.rate, 0, 100)) p.logging = "A success sample rate is 0 to 100.";
    else if (g.on && g.success.trim() === "" && g.failure.trim() === "") p.logging = `Choose the IAM role ${l} deliveries are logged with.`;
  }
  if (f.fifo && f.archive.trim() !== "" && !whole(f.archive, 1, 365)) p.archive = "A retention of 1 to 365 days, or empty for none.";
  return p;
}

type Attrs = Record<string, string>;

const logAttrs = (f: TopicForm): Attrs => {
  const out: Attrs = {};
  for (const l of LOGGED) {
    const g = f.logging[l];
    if (!g.on) continue;
    if (g.success.trim()) out[`${l}SuccessFeedbackRoleArn`] = g.success.trim();
    if (g.success.trim() && g.rate.trim() !== "") out[`${l}SuccessFeedbackSampleRate`] = g.rate.trim();
    if (g.failure.trim()) out[`${l}FailureFeedbackRoleArn`] = g.failure.trim();
  }
  return out;
};

/** What CreateTopic takes: the name, the attributes that differ from the service's defaults, and the tags. */
export function createInput(f: TopicForm, region: string, account: string): Record<string, unknown> {
  const attrs: Attrs = {};
  if (f.fifo) attrs.FifoTopic = "true";
  if (f.fifo && f.dedup) attrs.ContentBasedDeduplication = "true";
  if (f.fifo && f.scope === "MessageGroup") attrs.FifoThroughputScope = "MessageGroup";
  if (f.display.trim()) attrs.DisplayName = f.display.trim();
  if (Number(f.maxKib) * 1024 !== DEFAULT_MAX_SIZE && /^\d+$/.test(f.maxKib.trim())) attrs.MaximumMessageSize = String(Number(f.maxKib) * 1024);
  if (f.sse && f.kms.trim()) attrs.KmsMasterKeyId = f.kms.trim();
  if (f.policy.trim() && !policyBroken(f) && norm(f.policy) !== norm(defaultPolicy(f.name, region, account))) attrs.Policy = norm(f.policy);
  if (!sameRetry(f.retry, DEFAULT_RETRY)) attrs.DeliveryPolicy = deliveryPolicy(f.retry);
  Object.assign(attrs, logAttrs(f));
  if (f.tracing) attrs.TracingConfig = "Active";
  const tags = filled(f.tags).map((t) => ({ Key: t.key, Value: t.value }));
  return { Name: f.name.trim(), ...(Object.keys(attrs).length > 0 ? { Attributes: attrs } : {}), ...(tags.length > 0 ? { Tags: tags } : {}) };
}

export type Change = { name: string; value: string };

/** The attributes an edit changes, each one call of SetTopicAttributes; an attribute turned off is set to the empty value, which
 * removes it. */
export function changes(f: TopicForm, t: Topic, region: string, account: string): Change[] {
  const out: Change[] = [];
  const set = (name: string, value: string) => out.push({ name, value });
  if (f.display.trim() !== (t.display ?? "")) set("DisplayName", f.display.trim());
  if (t.fifo && f.dedup !== t.dedup) set("ContentBasedDeduplication", String(f.dedup));
  if (t.fifo && f.scope !== t.scope) set("FifoThroughputScope", f.scope);
  if (/^\d+$/.test(f.maxKib.trim()) && Number(f.maxKib) * 1024 !== t.maxSize) set("MaximumMessageSize", String(Number(f.maxKib) * 1024));
  const kms = f.sse ? f.kms.trim() : "";
  if (kms !== (t.kms ?? "")) set("KmsMasterKeyId", kms);
  const was = t.policy ?? defaultPolicy(t.name, region, account);
  if (f.policy.trim() !== "" && !policyBroken(f) && norm(f.policy) !== norm(was)) set("Policy", norm(f.policy));
  if (!sameRetry(f.retry, retryOf(t.effective))) set("DeliveryPolicy", deliveryPolicy(f.retry));
  for (const l of LOGGED) {
    const g = f.logging[l];
    const fb = t.feedback[l];
    const want = { success: g.on ? g.success.trim() : "", failure: g.on ? g.failure.trim() : "", rate: g.on && g.success.trim() ? g.rate.trim() : "" };
    if (want.success !== (fb.success ?? "")) set(`${l}SuccessFeedbackRoleArn`, want.success);
    if (want.rate !== (fb.rate === null ? "" : String(fb.rate))) set(`${l}SuccessFeedbackSampleRate`, want.rate);
    if (want.failure !== (fb.failure ?? "")) set(`${l}FailureFeedbackRoleArn`, want.failure);
  }
  if (f.tracing !== t.tracing) set("TracingConfig", f.tracing ? "Active" : "PassThrough");
  if (t.fifo) {
    const was = archiveOf(t.archive).days;
    const now = f.archive.trim() === "" ? null : Number(f.archive);
    if (now !== was) set("ArchivePolicy", now === null ? "{}" : JSON.stringify({ MessageRetentionPeriod: String(now) }));
  }
  return out;
}

/** What each setting an edit changes does, in words: `the maximum message size goes to 512 KiB`. A protocol's logging attributes are one phrase. */
export function changePhrases(c: readonly Change[]): string[] {
  const out: string[] = [];
  const add = (p: string) => out.includes(p) || out.push(p);
  for (const x of c) {
    if (x.name === "MaximumMessageSize") add(`the maximum message size goes to ${Number(x.value) / 1024} KiB`);
    else if (x.name === "DisplayName") add(x.value ? `the display name becomes ${x.value}` : "the display name is removed");
    else if (x.name === "ContentBasedDeduplication") add(`content-based deduplication is turned ${x.value === "true" ? "on" : "off"}`);
    else if (x.name === "FifoThroughputScope") add(`the throughput scope becomes ${x.value}`);
    else if (x.name === "KmsMasterKeyId") add(x.value ? `the encryption key becomes ${x.value}` : "encryption is turned off");
    else if (x.name === "Policy") add("the access policy is replaced");
    else if (x.name === "DeliveryPolicy") add("the delivery retry policy changes");
    else if (x.name === "TracingConfig") add(`active tracing is turned ${x.value === "Active" ? "on" : "off"}`);
    else if (x.name === "ArchivePolicy") add(x.value === "{}" ? "the archive policy is removed" : "the archive policy changes");
    else if (x.name.endsWith("FeedbackRoleArn") || x.name.endsWith("FeedbackSampleRate")) add("delivery status logging changes");
    else add(`${x.name} changes`);
  }
  return out;
}

/** Whether the policy text is the service's default for this topic (or empty, which leaves the default). */
export function isDefaultPolicy(text: string, standard: string): boolean {
  return text.trim() === "" || norm(text) === norm(standard);
}

// The queue form of Create queue and Edit queue, pure: what the fields hold, what is wrong with them, and the attributes a save
// sends (only those that differ from what the queue is, or from SQS's defaults for a new one). Limits are SQS's own
// (the CreateQueue and SetQueueAttributes documentation, models/sqs.json).
import { type KeyValueRow, newRow } from "@doze/ui/components/app/key-values";
import { inUnits, type Queue, toSeconds, type Unit } from "./queue-model";

export type Time = { value: string; unit: Unit };
export type Who = "owner" | "accounts" | "everyone";
export type AllowMode = "allowAll" | "byQueue" | "denyAll";

export type QueueForm = {
  fifo: boolean;
  name: string;
  visibility: Time;
  retention: Time;
  delay: Time;
  wait: Time;
  maxKib: string;
  dedup: boolean;
  highThroughput: boolean;
  scope: "queue" | "messageGroup";
  limit: "perQueue" | "perMessageGroupId";
  policyMode: "basic" | "advanced";
  send: Who;
  receive: Who;
  principals: string;
  policy: string;
  allowOn: boolean;
  allowMode: AllowMode;
  allowArns: string;
  sse: "sqs" | "kms" | "none";
  kms: string;
  reuse: Time;
  dlqOn: boolean;
  dlq: string;
  maxReceives: string;
  tags: KeyValueRow[];
};

/** SQS's limits, in the unit each setting is sent in. */
export const LIMITS = {
  visibility: { min: 0, max: 43_200 },
  retention: { min: 60, max: 1_209_600 },
  delay: { min: 0, max: 900 },
  wait: { min: 0, max: 20 },
  maxKib: { min: 1, max: 1_024 },
  maxReceives: { min: 1, max: 1_000 },
  reuse: { min: 60, max: 86_400 },
} as const;

export const DEFAULTS = { visibility: 30, retention: 345_600, delay: 0, wait: 0, maxKib: 1_024, reuse: 300 } as const;

const time = (secs: number): Time => {
  const u = inUnits(secs);
  return { value: String(u.value), unit: u.unit };
};

export function emptyForm(): QueueForm {
  return {
    fifo: false,
    name: "",
    visibility: time(DEFAULTS.visibility),
    retention: time(DEFAULTS.retention),
    delay: time(DEFAULTS.delay),
    wait: time(DEFAULTS.wait),
    maxKib: String(DEFAULTS.maxKib),
    dedup: false,
    highThroughput: false,
    scope: "queue",
    limit: "perQueue",
    policyMode: "basic",
    send: "owner",
    receive: "owner",
    principals: "",
    policy: "",
    allowOn: false,
    allowMode: "allowAll",
    allowArns: "",
    sse: "sqs",
    kms: "",
    reuse: time(DEFAULTS.reuse),
    dlqOn: false,
    dlq: "",
    maxReceives: "5",
    tags: [],
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

/** The form of an existing queue, for Edit: what it holds now (the Policy and the redrive policies as the service gave them). */
export function formOf(q: Queue, tags: readonly { key: string; value: string }[], dlqArn: string | null): QueueForm {
  const allow = parse(q.allow);
  const mode = allow?.redrivePermission;
  const form = emptyForm();
  return {
    ...form,
    fifo: q.fifo,
    name: q.name,
    visibility: time(q.visibility),
    retention: time(q.retention),
    delay: time(q.delay),
    wait: time(q.wait),
    maxKib: String(Math.round(q.maxSize / 1024)),
    dedup: q.dedup,
    highThroughput: q.scope === "messageGroup" && q.throughput === "perMessageGroupId",
    scope: q.scope === "messageGroup" ? "messageGroup" : "queue",
    limit: q.throughput === "perMessageGroupId" ? "perMessageGroupId" : "perQueue",
    policyMode: "advanced",
    policy: q.policy ? JSON.stringify(parse(q.policy) ?? q.policy, null, 2) : "",
    allowOn: allow !== null,
    allowMode: mode === "byQueue" || mode === "denyAll" ? mode : "allowAll",
    allowArns: Array.isArray(allow?.sourceQueueArns) ? (allow.sourceQueueArns as string[]).join("\n") : "",
    sse: q.sse,
    kms: q.kms ?? "",
    reuse: time(q.reuse ?? DEFAULTS.reuse),
    dlqOn: q.dlq !== null,
    dlq: dlqArn ?? "",
    maxReceives: String(q.maxReceives ?? 5),
    tags: tags.map((t) => ({ ...newRow(), key: t.key, value: t.value })),
  };
}

export type Problems = Partial<Record<"name" | "visibility" | "retention" | "delay" | "wait" | "maxKib" | "maxReceives" | "reuse" | "dlq" | "kms" | "policy" | "allow", string>>;

const NAME = /^[A-Za-z0-9_-]+$/;

/** The words of a name's rule, or null when it holds. */
export function nameProblem(name: string, fifo: boolean): string | null {
  if (name === "") return "Give the queue a name.";
  const base = fifo ? name.replace(/\.fifo$/, "") : name;
  if (fifo && !name.endsWith(".fifo")) return "A FIFO queue's name must end with .fifo.";
  if (!NAME.test(base)) return fifo ? "Use letters, digits, hyphens and underscores, then .fifo." : "Use letters, digits, hyphens and underscores only.";
  if (name.length > 80) return "A name is at most 80 characters, .fifo included.";
  return null;
}

const range = (what: string, n: number | null, min: number, max: number, unit: string): string | null => (n === null ? `${what} is a number.` : n < min || n > max ? `${what} must be ${min.toLocaleString("en-US")} to ${max.toLocaleString("en-US")} ${unit}.` : null);

/** What is wrong with the fields now, field by field (a field that is fine is not in it). */
export function problems(f: QueueForm, existing: { create: boolean }): Problems {
  const out: Problems = {};
  const set = (k: keyof Problems, v: string | null) => {
    if (v) out[k] = v;
  };
  if (existing.create) set("name", nameProblem(f.name, f.fifo));
  set("visibility", range("The visibility timeout", toSeconds(f.visibility.value, f.visibility.unit), LIMITS.visibility.min, LIMITS.visibility.max, "seconds (12 hours)"));
  set("retention", range("The retention period", toSeconds(f.retention.value, f.retention.unit), LIMITS.retention.min, LIMITS.retention.max, "seconds (1 minute to 14 days)"));
  set("delay", range("The delivery delay", toSeconds(f.delay.value, f.delay.unit), LIMITS.delay.min, LIMITS.delay.max, "seconds (15 minutes)"));
  set("wait", range("The receive wait time", toSeconds(f.wait.value, f.wait.unit), LIMITS.wait.min, LIMITS.wait.max, "seconds"));
  const kib = Number(f.maxKib);
  set("maxKib", range("The maximum message size", f.maxKib.trim() === "" || !Number.isInteger(kib) ? null : kib, LIMITS.maxKib.min, LIMITS.maxKib.max, "KiB"));
  if (f.dlqOn) {
    const n = Number(f.maxReceives);
    set("maxReceives", range("The maximum receives", f.maxReceives.trim() === "" || !Number.isInteger(n) ? null : n, LIMITS.maxReceives.min, LIMITS.maxReceives.max, ""));
    if (!f.dlq) out.dlq = "Choose the queue that receives the messages that keep failing.";
  }
  if (f.sse === "kms") {
    if (!f.kms.trim()) out.kms = "Choose a KMS key, or enter its alias or ARN.";
    set("reuse", range("The data key reuse period", toSeconds(f.reuse.value, f.reuse.unit), LIMITS.reuse.min, LIMITS.reuse.max, "seconds (1 minute to 24 hours)"));
  }
  if (f.policyMode === "advanced" && f.policy.trim() !== "" && parse(f.policy) === null) out.policy = "The access policy is not a JSON object.";
  if (f.allowOn && f.allowMode === "byQueue") {
    const arns = lines(f.allowArns);
    if (arns.length === 0) out.allow = "Name at least one source queue by ARN.";
    else if (arns.length > 10) out.allow = "Name at most 10 source queues; more than that needs Allow all.";
  }
  return out;
}

/** The non-empty lines of a text, trimmed (principals and ARNs are one a line, or comma-separated). */
export const lines = (text: string): string[] => text.split(/[\n,]/).map((s) => s.trim()).filter(Boolean);

const principalArn = (p: string): string => (/^\d{12}$/.test(p) ? `arn:aws:iam::${p}:root` : p);

/** The access policy the Basic choices write, or null when both are the queue owner (the default: no statement needed). */
export function basicPolicy(f: Pick<QueueForm, "send" | "receive" | "principals">, queueArn: string): string | null {
  const statement = (sid: string, who: Who, actions: string[]) => ({
    Sid: sid,
    Effect: "Allow",
    Principal: who === "everyone" ? "*" : { AWS: lines(f.principals).map(principalArn) },
    Action: actions.length === 1 ? actions[0] : actions,
    Resource: queueArn,
  });
  const statements = [f.send !== "owner" && statement("SendMessage", f.send, ["sqs:SendMessage"]), f.receive !== "owner" && statement("ReceiveMessage", f.receive, ["sqs:ReceiveMessage", "sqs:DeleteMessage", "sqs:GetQueueAttributes"])].filter(Boolean);
  if (statements.length === 0) return null;
  return JSON.stringify({ Version: "2012-10-17", Id: "__default_policy_ID", Statement: statements }, null, 2);
}

/** The Policy attribute the form means: the Basic choices' policy, or the JSON typed in Advanced; null for none. */
export function policyOf(f: QueueForm, queueArn: string): string | null {
  if (f.policyMode === "basic") return basicPolicy(f, queueArn);
  return f.policy.trim() === "" ? null : f.policy.trim();
}

/** `RedriveAllowPolicy`, as SQS keeps it. */
export function allowPolicy(f: Pick<QueueForm, "allowOn" | "allowMode" | "allowArns">): string | null {
  if (!f.allowOn) return null;
  return JSON.stringify(f.allowMode === "byQueue" ? { redrivePermission: "byQueue", sourceQueueArns: lines(f.allowArns) } : { redrivePermission: f.allowMode });
}

/** The same JSON written compactly, so two spellings of one policy are one. */
const same = (a: string | null, b: string | null): boolean => {
  const norm = (t: string | null) => {
    const v = parse(t);
    return v === null ? (t ?? "") : JSON.stringify(v);
  };
  return norm(a) === norm(b);
};

export type Base = { create: true } | { create: false; queue: Queue; dlqArn: string | null };

/** The attributes a save sends: every one that differs from the queue as it is (Edit), or from SQS's defaults (Create). */
export function attributes(f: QueueForm, base: Base, queueArn: string): Record<string, string> {
  const out: Record<string, string> = {};
  const was = base.create ? { visibility: DEFAULTS.visibility, retention: DEFAULTS.retention, delay: DEFAULTS.delay, wait: DEFAULTS.wait, maxSize: DEFAULTS.maxKib * 1024, dedup: false, sse: "sqs" as const, kms: "", reuse: DEFAULTS.reuse, scope: "queue", throughput: "perQueue", policy: null as string | null, allow: null as string | null, dlqArn: null as string | null, max: null as number | null } : { visibility: base.queue.visibility, retention: base.queue.retention, delay: base.queue.delay, wait: base.queue.wait, maxSize: base.queue.maxSize, dedup: base.queue.dedup, sse: base.queue.sse, kms: base.queue.kms ?? "", reuse: base.queue.reuse ?? DEFAULTS.reuse, scope: base.queue.scope ?? "queue", throughput: base.queue.throughput ?? "perQueue", policy: base.queue.policy, allow: base.queue.allow, dlqArn: base.dlqArn, max: base.queue.maxReceives };
  const put = (name: string, now: number | null, before: number) => {
    if (now !== null && now !== before) out[name] = String(now);
  };
  put("VisibilityTimeout", toSeconds(f.visibility.value, f.visibility.unit), was.visibility);
  put("MessageRetentionPeriod", toSeconds(f.retention.value, f.retention.unit), was.retention);
  put("DelaySeconds", toSeconds(f.delay.value, f.delay.unit), was.delay);
  put("ReceiveMessageWaitTimeSeconds", toSeconds(f.wait.value, f.wait.unit), was.wait);
  const kib = Number(f.maxKib);
  if (Number.isInteger(kib) && kib * 1024 !== was.maxSize) out.MaximumMessageSize = String(kib * 1024);
  if (base.create && f.fifo) out.FifoQueue = "true";
  if (f.fifo) {
    if (f.dedup !== was.dedup) out.ContentBasedDeduplication = String(f.dedup);
    const scope = f.highThroughput ? "messageGroup" : f.scope;
    const limit = f.highThroughput ? "perMessageGroupId" : f.limit;
    if (scope !== was.scope) out.DeduplicationScope = scope;
    if (limit !== was.throughput) out.FifoThroughputLimit = limit;
  }
  const policy = policyOf(f, queueArn);
  if (!same(policy, was.policy)) out.Policy = policy ?? "";
  const allow = allowPolicy(f);
  if (!same(allow, was.allow)) out.RedriveAllowPolicy = allow ?? "";
  if (f.sse !== was.sse || (f.sse === "kms" && f.kms.trim() !== was.kms)) {
    if (f.sse === "kms") out.KmsMasterKeyId = f.kms.trim();
    else if (f.sse === "sqs") Object.assign(out, was.sse === "kms" ? { KmsMasterKeyId: "", SqsManagedSseEnabled: "true" } : { SqsManagedSseEnabled: "true" });
    else Object.assign(out, was.sse === "kms" ? { KmsMasterKeyId: "", SqsManagedSseEnabled: "false" } : { SqsManagedSseEnabled: "false" });
  }
  if (f.sse === "kms") put("KmsDataKeyReusePeriodSeconds", toSeconds(f.reuse.value, f.reuse.unit), was.reuse);
  const target = f.dlqOn ? f.dlq : null;
  const max = f.dlqOn ? Number(f.maxReceives) : null;
  if (target !== was.dlqArn || max !== was.max) out.RedrivePolicy = target ? JSON.stringify({ deadLetterTargetArn: target, maxReceiveCount: max }) : "";
  return out;
}

/** The tags of the form as CreateQueue takes them. */
export function tagMap(rows: readonly KeyValueRow[]): Record<string, string> {
  return Object.fromEntries(rows.filter((r) => r.key.trim() !== "").map((r) => [r.key.trim(), r.value]));
}

/** CreateQueue's input from a new queue's form. */
export function createInput(f: QueueForm, queueArn: string): Record<string, unknown> {
  const attrs = attributes(f, { create: true }, queueArn);
  const tags = tagMap(f.tags);
  return { QueueName: f.name, ...(Object.keys(attrs).length > 0 ? { Attributes: attrs } : {}), ...(Object.keys(tags).length > 0 ? { tags } : {}) };
}

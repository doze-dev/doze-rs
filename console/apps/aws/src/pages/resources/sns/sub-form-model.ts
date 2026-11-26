// Create subscription and Edit subscription: the form's state and the calls it makes. Pure.
// Subscribe takes the attributes with it; SetSubscriptionAttributes sets one attribute a call, so an edit is the list of those that changed.
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { inlineJson } from "./filter-match";
import { protocolOf, type Protocol } from "./protocols";
import { tail } from "./sub-model";

export type Scope = "MessageAttributes" | "MessageBody";

/** `filter` is the filter policy JSON; empty is none. */
export type SubForm = { topic: string; protocol: Protocol; endpoint: string; raw: boolean; filter: string; scope: Scope; redriveOn: boolean; dlq: string };

export const emptySubForm = (topic = ""): SubForm => ({ topic, protocol: "sqs", endpoint: "", raw: false, filter: "", scope: "MessageAttributes", redriveOn: false, dlq: "" });

/** A subscription as its page reads it. */
export type Subscription = {
  name: string;
  arn: string | null;
  topicArn: string;
  protocol: string;
  endpoint: string;
  pending: boolean;
  raw: boolean;
  filter: string | null;
  scope: Scope;
  redrive: string | null;
  owner: string | null;
};

export function subscriptionOf(d: ResourceDetail): Subscription {
  const f = (label: string): string | null => {
    const v = d.fields.find((x) => x.label === label)?.value;
    return v === undefined || v === "" ? null : v;
  };
  const pending = f("Pending confirmation") === "Yes";
  return { name: d.name, arn: d.id.startsWith("arn:") ? (f("ARN") ?? d.id) : null, topicArn: f("Topic ARN") ?? "", protocol: f("Protocol") ?? "", endpoint: f("Endpoint") ?? "", pending, raw: f("Raw message delivery") === "Enabled", filter: f("Filter policy"), scope: f("Filter policy scope") === "MessageBody" ? "MessageBody" : "MessageAttributes", redrive: f("Redrive policy"), owner: f("Owner") };
}

const norm = (text: string): string => {
  try {
    return JSON.stringify(JSON.parse(text));
  } catch {
    return text.trim();
  }
};

/** The queue or function (ARN) of a redrive policy's dead-letter target. */
export function redriveTarget(policy: string | null): string | null {
  if (!policy) return null;
  try {
    const t = (JSON.parse(policy) as { deadLetterTargetArn?: unknown }).deadLetterTargetArn;
    return typeof t === "string" ? t : null;
  } catch {
    return null;
  }
}

export const redrivePolicy = (arn: string): string => JSON.stringify({ deadLetterTargetArn: arn });

/** The form of an existing subscription, for its edit page. */
export function formOfSubscription(s: Subscription): SubForm {
  return { topic: s.topicArn, protocol: (protocolOf(s.protocol)?.id ?? "sqs") as Protocol, endpoint: s.endpoint, raw: s.raw, filter: s.filter ? inlineJson(s.filter) : "", scope: s.scope, redriveOn: redriveTarget(s.redrive) !== null, dlq: redriveTarget(s.redrive) ?? "" };
}

/** Whether the filter policy typed is not a JSON object yet: none of it is put in a call, and the form cannot be saved until it is. */
export function filterBroken(f: Pick<SubForm, "filter">): boolean {
  if (f.filter.trim() === "") return false;
  try {
    const doc: unknown = JSON.parse(f.filter);
    return typeof doc !== "object" || doc === null || Array.isArray(doc);
  } catch {
    return true;
  }
}

export type Problems = Partial<Record<"topic" | "endpoint" | "filter" | "dlq", string>>;

const ENDPOINT: Partial<Record<Protocol, [RegExp, string]>> = {
  sqs: [/^arn:aws:sqs:[^:]+:\d{12}:.+/, "The endpoint of an Amazon SQS subscription is the ARN of a queue."],
  lambda: [/^arn:aws:lambda:[^:]+:\d{12}:function:.+/, "The endpoint of an AWS Lambda subscription is the ARN of a function."],
  http: [/^http:\/\/.+/, "The endpoint of an HTTP subscription is a URL beginning with http://."],
  https: [/^https:\/\/.+/, "The endpoint of an HTTPS subscription is a URL beginning with https://."],
};

export function problems(f: SubForm, create: boolean): Problems {
  const p: Problems = {};
  if (create) {
    if (!/^arn:aws:sns:[^:]+:\d{12}:.+/.test(f.topic.trim())) p.topic = "Enter the ARN of a topic, or choose one.";
    const rule = ENDPOINT[f.protocol];
    if (f.endpoint.trim() === "") p.endpoint = "Enter the endpoint.";
    else if (rule && !rule[0].test(f.endpoint.trim())) p.endpoint = rule[1];
  }
  if (f.filter.trim() !== "") {
    try {
      const doc: unknown = JSON.parse(f.filter);
      if (typeof doc !== "object" || doc === null || Array.isArray(doc)) p.filter = "A filter policy is a JSON object.";
    } catch (e) {
      p.filter = `Not JSON: ${(e as Error).message}`;
    }
  }
  if (f.redriveOn && !/^arn:aws:sqs:[^:]+:\d{12}:.+/.test(f.dlq.trim())) p.dlq = "Choose the queue, or enter its ARN.";
  return p;
}

/** What Subscribe takes: the three that name the subscription and the attributes that differ from the service's defaults. A subscription
 * with a filter policy also says its scope and whether raw delivery is on, as the board's call does, so the filter is read whole. */
export function subscribeInput(f: SubForm): Record<string, unknown> {
  const attrs: Record<string, string> = {};
  const filtered = f.filter.trim() !== "" && !filterBroken(f);
  const rawable = protocolOf(f.protocol)?.raw === true;
  if (rawable && (f.raw || filtered)) attrs.RawMessageDelivery = String(f.raw);
  if (filtered) {
    attrs.FilterPolicy = norm(f.filter);
    attrs.FilterPolicyScope = f.scope;
  }
  if (f.redriveOn && f.dlq.trim()) attrs.RedrivePolicy = redrivePolicy(f.dlq.trim());
  return { TopicArn: f.topic.trim(), Protocol: f.protocol, Endpoint: f.endpoint.trim(), ReturnSubscriptionArn: true, ...(Object.keys(attrs).length > 0 ? { Attributes: attrs } : {}) };
}

export type Change = { name: string; value: string };

/** The attributes an edit changes, one call of SetSubscriptionAttributes each; a setting turned off is set to the empty value. */
export function changes(f: SubForm, s: Subscription): Change[] {
  const out: Change[] = [];
  if (f.raw !== s.raw) out.push({ name: "RawMessageDelivery", value: String(f.raw) });
  const was = s.filter ? norm(s.filter) : "";
  const typed = f.filter.trim() !== "" && !filterBroken(f);
  // A filter policy that is not JSON yet is no member of the call (an empty one would remove the subscription's).
  const now = typed ? norm(f.filter) : "";
  if (!filterBroken(f) && now !== was) out.push({ name: "FilterPolicy", value: now });
  if (now !== "" && f.scope !== s.scope) out.push({ name: "FilterPolicyScope", value: f.scope });
  const dlq = f.redriveOn ? f.dlq.trim() : "";
  if (dlq !== (redriveTarget(s.redrive) ?? "")) out.push({ name: "RedrivePolicy", value: dlq ? redrivePolicy(dlq) : "" });
  return out;
}

/** `You turned raw message delivery on and added a dead-letter queue. The filter policy is unchanged.` */
export function changeWords(c: readonly Change[]): string {
  if (c.length === 0) return "Nothing has changed.";
  const word = (x: Change): string => {
    if (x.name === "RawMessageDelivery") return `turned raw message delivery ${x.value === "true" ? "on" : "off"}`;
    if (x.name === "FilterPolicy") return x.value ? "set the filter policy" : "removed the filter policy";
    if (x.name === "FilterPolicyScope") return "changed the filter policy scope";
    return x.value ? "set a dead-letter queue" : "removed the dead-letter queue";
  };
  const parts = c.map(word);
  const text = parts.length === 1 ? parts[0]! : `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
  return `You ${text}.`;
}

/** The token of a SubscribeURL (`…/confirm?Action=ConfirmSubscription&TopicArn=…&Token=…`), or `null` when it carries none. */
export function confirmationOf(url: string): { topicArn: string | null; token: string } | null {
  const text = url.trim();
  const q = text.includes("?") ? text.slice(text.indexOf("?") + 1) : "";
  const params = new URLSearchParams(q);
  const token = params.get("Token");
  return token ? { topicArn: params.get("TopicArn"), token } : null;
}

export const queueName = (arn: string): string => tail(arn);

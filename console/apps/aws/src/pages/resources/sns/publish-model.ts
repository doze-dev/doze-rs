// Publish message: the form's state, the call it makes, and what each subscription of the topic is given. Pure.
import { newRow, type KeyValueRow } from "@doze/ui/components/app/key-values";
import type { Attribute } from "./filter-match";

export type AttrType = Attribute["type"];
export type AttrRow = { id: string; type: AttrType; name: string; value: string };

export type PublishForm = {
  subject: string;
  group: string;
  dedup: string;
  structure: "identical" | "custom";
  body: string;
  custom: string;
  attrs: AttrRow[];
};

let counter = 0;
export const newAttr = (name = "", value = "", type: AttrType = "String"): AttrRow => ({ id: `pa${++counter}`, type, name, value });

export const emptyPublish = (): PublishForm => ({ subject: "", group: "", dedup: "", structure: "identical", body: "", custom: '{\n  "default": ""\n}', attrs: [] });

export const bytes = (text: string): number => new TextEncoder().encode(text).length;

/** The message the form sends: the body, or the custom JSON. */
export const messageOf = (f: PublishForm): string => (f.structure === "custom" ? f.custom : f.body);

/** The members `Publish` takes; those left empty are left out. Attributes are sent only for an identical payload (SNS drops them with a
 * custom one). */
export function publishInput(f: PublishForm): Record<string, unknown> {
  const input: Record<string, unknown> = { Message: messageOf(f) };
  if (f.structure === "custom") input.MessageStructure = "json";
  if (f.subject.trim()) input.Subject = f.subject;
  if (f.group.trim()) input.MessageGroupId = f.group.trim();
  if (f.dedup.trim()) input.MessageDeduplicationId = f.dedup.trim();
  const attrs = f.attrs.filter((a) => a.name.trim() !== "");
  if (f.structure === "identical" && attrs.length > 0) {
    input.MessageAttributes = Object.fromEntries(attrs.map((a) => [a.name.trim(), { DataType: a.type, ...(a.type === "Binary" ? { BinaryValue: btoa(a.value) } : { StringValue: a.value }) }]));
  }
  return input;
}

/** What the filter policy preview matches the message's attributes against. */
export const attributesOf = (f: PublishForm): Record<string, Attribute> => Object.fromEntries(f.attrs.filter((a) => a.name.trim() !== "").map((a) => [a.name.trim(), { type: a.type, value: a.value }]));

export type Problems = Partial<Record<"body" | "subject" | "group" | "dedup" | "size" | "custom", string>>;

/** The problems the form can see before the service is asked. A missing deduplication ID is not one of them: the service refuses it,
 * and the page shows its words (sns__publish-fifo-refused). */
export function problems(f: PublishForm, topic: { fifo: boolean; maxSize: number }): Problems {
  const p: Problems = {};
  const message = messageOf(f);
  if (f.structure === "identical" && f.body === "") p.body = "Enter the message.";
  if (f.subject.length >= 100 || /[\r\n]/.test(f.subject)) p.subject = "A subject is less than 100 characters, with no line breaks or control characters.";
  if (f.structure === "custom") {
    try {
      const doc: unknown = JSON.parse(f.custom);
      if (typeof doc !== "object" || doc === null || Array.isArray(doc) || typeof (doc as Record<string, unknown>).default !== "string") p.custom = "The JSON needs a top-level default key whose value is a string.";
    } catch (e) {
      p.custom = `Not JSON: ${(e as Error).message}`;
    }
  }
  if (topic.fifo && f.group.trim() === "") p.group = "Required on a FIFO topic.";
  if (f.group.length > 128) p.group = "Up to 128 letters, numbers and punctuation.";
  if (f.dedup.length > 128) p.dedup = "Up to 128 letters, numbers and punctuation.";
  if (bytes(message) > topic.maxSize) p.size = `The message is ${bytes(message).toLocaleString("en-US")} bytes; this topic takes at most ${topic.maxSize.toLocaleString("en-US")}.`;
  return p;
}

export type Delivery = { endpoint: string; text: string };

/** What a subscription of `protocol` is given: the custom payload's key for the protocol (else `default`), in SNS's envelope unless the
 * subscription is raw. */
export function givenText(f: PublishForm, protocol: string): string {
  if (f.structure === "identical") return "the message";
  try {
    const doc = JSON.parse(f.custom) as Record<string, unknown>;
    return typeof doc[protocol] === "string" ? `the ${protocol} text` : "the default text";
  } catch {
    return "the default text";
  }
}

/** Blank rows of attributes are ignored; this is how many count. */
export const attrCount = (rows: readonly (KeyValueRow | AttrRow)[]): number => rows.filter((r) => ("name" in r ? r.name : r.key).trim() !== "").length;

/** The key a custom payload gives a protocol: SNS names `http`, `https`, `sqs`, `lambda` and the others the same as the protocol. */
export const customKeys = (f: PublishForm): string[] => {
  try {
    return Object.keys(JSON.parse(f.custom) as object);
  } catch {
    return [];
  }
};

export { newRow };

// SendMessage's input from the Send message form (sqs__queue-messages, queue-messages-fifo), pure.
export type AttrType = "String" | "Number" | "Binary";
/** `custom`: the label SQS lets a type carry (`Number.int` is a Number with the custom type `int`). */
export type Attr = { name: string; type: AttrType; custom: string; value: string };
export type DelayUnit = "seconds" | "minutes";
export type SendForm = { body: string; delay: string; delayUnit: DelayUnit; group: string; dedup: string; attrs: Attr[] };

export const EMPTY: SendForm = { body: "", delay: "", delayUnit: "seconds", group: "", dedup: "", attrs: [] };
export const NEW_ATTR: Attr = { name: "", type: "String", custom: "", value: "" };

/** Binary attributes travel base64-encoded in the JSON protocol: the text typed is encoded as UTF-8. */
export function base64(text: string): string {
  let bin = "";
  for (const b of new TextEncoder().encode(text)) bin += String.fromCharCode(b);
  return btoa(bin);
}

/** The delay as the call takes it, in seconds: 0 to 900 (15 minutes), in whole seconds. */
export function delaySeconds(text: string, unit: DelayUnit): number | string {
  const n = Number(text) * (unit === "minutes" ? 60 : 1);
  return Number.isInteger(n) && n >= 0 && n <= 900 ? n : "Delay is 0 to 15 minutes: a whole number of seconds, 0 to 900.";
}

/** SendMessage's input from what the form holds; the words of what is wrong instead, when something is. */
export function sendInput(form: SendForm, fifo: boolean): Record<string, unknown> | string {
  if (!form.body) return "A message needs a body.";
  const input: Record<string, unknown> = { MessageBody: form.body };
  if (fifo) {
    if (!form.group.trim()) return "A FIFO queue's message needs a message group ID.";
    input.MessageGroupId = form.group.trim();
    if (form.dedup.trim()) input.MessageDeduplicationId = form.dedup.trim();
  } else if (form.delay.trim()) {
    const n = delaySeconds(form.delay, form.delayUnit);
    if (typeof n === "string") return n;
    input.DelaySeconds = n;
  }
  const named = form.attrs.filter((a) => a.name.trim());
  if (named.length > 0) {
    const attrs: Record<string, unknown> = {};
    for (const a of named) {
      if (a.type === "Number" && (a.value.trim() === "" || Number.isNaN(Number(a.value)))) return `The attribute ${a.name} is a Number: give it one.`;
      const DataType = a.custom.trim() ? `${a.type}.${a.custom.trim()}` : a.type;
      attrs[a.name.trim()] = a.type === "Binary" ? { DataType, BinaryValue: base64(a.value) } : { DataType, StringValue: a.value };
    }
    input.MessageAttributes = attrs;
  }
  return input;
}

// SendMessage's input from the Send message form (sqs__queue-messages, queue-messages-fifo), pure.
export type AttrType = "String" | "Number" | "Binary";
export type Attr = { name: string; type: AttrType; value: string };
export type SendForm = { body: string; delay: string; group: string; dedup: string; attrs: Attr[] };

export const EMPTY: SendForm = { body: "", delay: "", group: "", dedup: "", attrs: [] };

/** Binary attributes travel base64-encoded in the JSON protocol: the text typed is encoded as UTF-8. */
export function base64(text: string): string {
  let bin = "";
  for (const b of new TextEncoder().encode(text)) bin += String.fromCharCode(b);
  return btoa(bin);
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
    const n = Number(form.delay);
    if (!Number.isInteger(n) || n < 0 || n > 900) return "Delay is a whole number of seconds, 0 to 900.";
    input.DelaySeconds = n;
  }
  const named = form.attrs.filter((a) => a.name.trim());
  if (named.length > 0) {
    const attrs: Record<string, unknown> = {};
    for (const a of named) {
      if (a.type === "Number" && (a.value.trim() === "" || Number.isNaN(Number(a.value)))) return `The attribute ${a.name} is a Number: give it one.`;
      attrs[a.name.trim()] = a.type === "Binary" ? { DataType: "Binary", BinaryValue: base64(a.value) } : { DataType: a.type, StringValue: a.value };
    }
    input.MessageAttributes = attrs;
  }
  return input;
}

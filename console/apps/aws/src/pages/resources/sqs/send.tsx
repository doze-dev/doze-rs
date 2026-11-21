// Send a message to a queue: its body, its attributes, for a FIFO queue its group and deduplication ids, for a standard one a
// delay. SendMessage, as the caller `console`.
import { PlusIcon, SendIcon, XIcon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { type Target, useOp } from "../deep/api";
import { Card, Refused } from "../deep/parts";

type Attr = { name: string; type: "String" | "Number"; value: string };

/** SendMessage's input from what the form holds; the words of what is wrong instead, when something is. */
export function sendInput(form: { body: string; delay: string; group: string; dedup: string; attrs: Attr[] }, fifo: boolean): Record<string, unknown> | string {
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
      attrs[a.name.trim()] = { DataType: a.type, StringValue: a.value };
    }
    input.MessageAttributes = attrs;
  }
  return input;
}

const EMPTY = { body: "", delay: "", group: "", dedup: "", attrs: [] as Attr[] };

export function SendCard({ t, fifo, dedupByContent }: { t: Target; fifo: boolean; dedupByContent: boolean }) {
  const [form, setForm] = useState(EMPTY);
  const send = useOp<{ MessageId: string }>(t, "SendMessage");
  const input = sendInput(form, fifo);
  const wrong = typeof input === "string" && form.body ? input : null;
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (typeof input === "string" || send.isPending) return;
    send.mutate(input, {
      onSuccess: (r) => {
        toast.success("Message sent", { description: r.MessageId });
        setForm((f) => ({ ...EMPTY, group: f.group }));
      },
    });
  };
  const setAttr = (i: number, a: Partial<Attr>) => setForm((f) => ({ ...f, attrs: f.attrs.map((x, j) => (j === i ? { ...x, ...a } : x)) }));
  return (
    <Card title="Send a message" lede={<>Sent with <span className="font-mono">SendMessage</span> as the caller <span className="font-mono">console</span>.</>} testId="send-card">
      <form onSubmit={submit} className="grid gap-5">
        <div className="grid content-start gap-1.5">
          <Label htmlFor="send-body">Body</Label>
          <Textarea id="send-body" value={form.body} onChange={(e) => setForm({ ...form, body: e.target.value })} placeholder='e.g. {"order_id": "ord-0372"}' className="min-h-28 bg-card font-mono text-[13px]" spellCheck={false} data-testid="send-body" />
        </div>
        {fifo ? (
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="grid content-start gap-1.5">
              <Label htmlFor="send-group">Message group ID</Label>
              <Input id="send-group" value={form.group} onChange={(e) => setForm({ ...form, group: e.target.value })} placeholder="e.g. customer-0301" className="bg-card font-mono text-[13px]" data-testid="send-group" />
              <p className="text-[12.5px] text-muted-foreground">Messages of one group are delivered in order, one group at a time.</p>
            </div>
            <div className="grid content-start gap-1.5">
              <Label htmlFor="send-dedup">
                Deduplication ID{dedupByContent && <span className="ml-1.5 font-normal text-muted-foreground">optional</span>}
              </Label>
              <Input id="send-dedup" value={form.dedup} onChange={(e) => setForm({ ...form, dedup: e.target.value })} placeholder="e.g. ord-0372" className="bg-card font-mono text-[13px]" data-testid="send-dedup" />
              <p className="text-[12.5px] text-muted-foreground">{dedupByContent ? "Content-based deduplication is on: without one, the body's SHA-256 is used." : "Required: content-based deduplication is off. A second send with the same ID within 5 minutes is dropped."}</p>
            </div>
          </div>
        ) : (
          <div className="grid max-w-56 gap-1.5">
            <Label htmlFor="send-delay">
              Delay (seconds)<span className="ml-1.5 font-normal text-muted-foreground">optional</span>
            </Label>
            <Input id="send-delay" inputMode="numeric" value={form.delay} onChange={(e) => setForm({ ...form, delay: e.target.value })} placeholder="e.g. 0" className="bg-card font-mono text-[13px]" data-testid="send-delay" />
            <p className="text-[12.5px] text-muted-foreground">0 to 900. Default: the queue's delivery delay.</p>
          </div>
        )}
        <fieldset className="grid gap-2">
          <legend className="mb-1 text-sm font-medium">Message attributes</legend>
          {form.attrs.map((a, i) => (
            <div key={i} className="grid grid-cols-[minmax(0,1fr)_8rem_minmax(0,1fr)_auto] items-center gap-2" data-testid="send-attr">
              <Input aria-label="Attribute name" value={a.name} onChange={(e) => setAttr(i, { name: e.target.value })} placeholder="e.g. source" className="bg-card font-mono text-[13px]" />
              <Select value={a.type} onValueChange={(v) => setAttr(i, { type: v as Attr["type"] })}>
                <SelectTrigger aria-label="Attribute type" className="w-full bg-card text-[13px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="String">String</SelectItem>
                  <SelectItem value="Number">Number</SelectItem>
                </SelectContent>
              </Select>
              <Input aria-label="Attribute value" value={a.value} onChange={(e) => setAttr(i, { value: e.target.value })} placeholder="e.g. checkout" className="bg-card font-mono text-[13px]" />
              <Button type="button" variant="ghost" size="icon" aria-label="Remove attribute" onClick={() => setForm((f) => ({ ...f, attrs: f.attrs.filter((_, j) => j !== i) }))}>
                <XIcon />
              </Button>
            </div>
          ))}
          <div>
            <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => setForm((f) => ({ ...f, attrs: [...f.attrs, { name: "", type: "String", value: "" }] }))} data-testid="send-add-attr">
              <PlusIcon /> Add attribute
            </Button>
          </div>
        </fieldset>
        {wrong && <p className="text-[13px] text-bad">{wrong}</p>}
        <Refused operation="SendMessage" error={send.error} testId="send-error" />
        <div className="flex justify-end border-t pt-5">
          <Button type="submit" disabled={typeof input === "string" || send.isPending} data-testid="send-submit">
            <SendIcon /> {send.isPending ? "Sending…" : "Send message"}
          </Button>
        </div>
      </form>
    </Card>
  );
}

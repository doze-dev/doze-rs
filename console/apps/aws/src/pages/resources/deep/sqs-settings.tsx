// A queue's settings that are changed most: timings, size, dead-letter queue, and for a FIFO queue content-based deduplication.
// SetQueueAttributes with only what changed, as the caller `console`.
import { SaveIcon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Switch } from "@doze/ui/components/ui/switch";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { useOp } from "./api";
import type { DeepProps } from "./pages";
import { Card, Refused } from "./parts";
import { fieldOf } from "./sqs-messages";

type Num = { key: string; label: string; field: string; min: number; max: number; help: string };

const NUMBERS: Num[] = [
  { key: "VisibilityTimeout", label: "Visibility timeout (seconds)", field: "Visibility timeout", min: 0, max: 43_200, help: "How long a received message is hidden from other consumers. 0 to 43,200 (12 hours)." },
  { key: "MessageRetentionPeriod", label: "Message retention period (seconds)", field: "Message retention period", min: 60, max: 1_209_600, help: "How long a message is kept. 60 to 1,209,600 (14 days)." },
  { key: "DelaySeconds", label: "Delivery delay (seconds)", field: "Delivery delay", min: 0, max: 900, help: "How long a new message waits before it can be received. 0 to 900." },
  { key: "ReceiveMessageWaitTimeSeconds", label: "Receive message wait time (seconds)", field: "Receive message wait time", min: 0, max: 20, help: "How long a ReceiveMessage waits for a message (long polling). 0 to 20." },
  { key: "MaximumMessageSize", label: "Maximum message size (bytes)", field: "Maximum message size", min: 1024, max: 1_048_576, help: "1,024 to 1,048,576 (1 MiB)." },
];

type Form = Record<string, string> & { dlq: string; maxReceives: string; dedup: string };

function start(d: ResourceDetail): Form {
  const form: Record<string, string> = {};
  for (const n of NUMBERS) form[n.key] = fieldOf(d, n.field) ?? "";
  return { ...form, dlq: fieldOf(d, "Dead-letter queue") ?? "", maxReceives: fieldOf(d, "Maximum receives") ?? "", dedup: fieldOf(d, "Content-based deduplication") === "Enabled" ? "true" : "false" };
}

/** The attributes that changed between `was` and `now`, as SetQueueAttributes takes them; the words of what is wrong instead. */
export function changed(was: Form, now: Form, arnOf: (queue: string) => string, fifo: boolean): Record<string, string> | string {
  const out: Record<string, string> = {};
  for (const n of NUMBERS) {
    const v = (now[n.key] ?? "").trim();
    if (v === (was[n.key] ?? "")) continue;
    const x = Number(v);
    if (v === "" || !Number.isInteger(x) || x < n.min || x > n.max) return `${n.label}: a whole number from ${n.min.toLocaleString("en")} to ${n.max.toLocaleString("en")}.`;
    out[n.key] = String(x);
  }
  if (now.dlq.trim() !== was.dlq || now.maxReceives.trim() !== was.maxReceives) {
    if (now.dlq.trim() === "") out.RedrivePolicy = "";
    else {
      const n = Number(now.maxReceives);
      if (!Number.isInteger(n) || n < 1 || n > 1000) return "Maximum receives: a whole number from 1 to 1,000.";
      out.RedrivePolicy = JSON.stringify({ deadLetterTargetArn: arnOf(now.dlq.trim()), maxReceiveCount: n });
    }
  }
  if (fifo && now.dedup !== was.dedup) out.ContentBasedDeduplication = now.dedup;
  return out;
}

export function QueueSettings({ d }: DeepProps) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const fifo = d.name.endsWith(".fifo");
  const was = start(d);
  const [form, setForm] = useState<Form>(was);
  const save = useOp(t, "SetQueueAttributes");
  const arn = fieldOf(d, "ARN") ?? "";
  const arnOf = (q: string) => `${arn.slice(0, arn.lastIndexOf(":") + 1)}${q}`;
  const diff = changed(was, form, arnOf, fifo);
  const count = typeof diff === "string" ? 0 : Object.keys(diff).length;
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (typeof diff === "string" || count === 0) return;
    save.mutate({ Attributes: diff }, { onSuccess: () => toast.success(`${d.name} saved`, { description: Object.keys(diff).join(", ") }) });
  };
  const field = (id: string, label: string, help: string, value: string, set: (v: string) => void, placeholder?: string) => (
    <div className="grid content-start gap-1.5" key={id}>
      <Label htmlFor={`set-${id}`}>{label}</Label>
      <Input id={`set-${id}`} value={value} onChange={(e) => set(e.target.value)} placeholder={placeholder} inputMode={id === "dlq" ? undefined : "numeric"} className="bg-card font-mono text-[13px]" spellCheck={false} autoComplete="off" data-testid={`set-${id}`} />
      <p className="text-[12.5px] text-muted-foreground">{help}</p>
    </div>
  );
  return (
    <form onSubmit={submit} className="grid max-w-[880px] gap-6">
      <Card title="Timings and size" lede="Changed with SetQueueAttributes; only what you change is sent.">
        <div className="grid gap-5 sm:grid-cols-2">{NUMBERS.map((n) => field(n.key, n.label, n.help, form[n.key] ?? "", (v) => setForm({ ...form, [n.key]: v })))}</div>
      </Card>
      <Card title="Dead-letter queue" lede="Where a message goes once it has been received more times than the maximum. Empty: none.">
        <div className="grid gap-5 sm:grid-cols-2">
          {field("dlq", "Dead-letter queue", "The name of another queue of this account and region; a FIFO queue's must be FIFO too.", form.dlq, (v) => setForm({ ...form, dlq: v }), "e.g. orders-dlq")}
          {field("maxReceives", "Maximum receives", "1 to 1,000.", form.maxReceives, (v) => setForm({ ...form, maxReceives: v }), "e.g. 5")}
        </div>
      </Card>
      {fifo && (
        <Card title="Deduplication">
          <div className="flex items-start gap-3">
            <Switch id="set-dedup" checked={form.dedup === "true"} onCheckedChange={(on) => setForm({ ...form, dedup: on ? "true" : "false" })} className="mt-0.5" data-testid="set-dedup" />
            <div className="grid gap-1">
              <Label htmlFor="set-dedup">Content-based deduplication</Label>
              <p className="text-[12.5px] text-muted-foreground">A message sent without a deduplication ID is deduplicated by its body's SHA-256.</p>
            </div>
          </div>
        </Card>
      )}
      {typeof diff === "string" && <p className="text-[13px] text-bad" data-testid="settings-wrong">{diff}</p>}
      <Refused operation="SetQueueAttributes" error={save.error} testId="settings-error" />
      <div className="flex items-center justify-end gap-3 border-t pt-5">
        <span className="text-[13px] text-muted-foreground">{count === 0 ? "Nothing changed yet." : `${count} ${count === 1 ? "setting" : "settings"} changed.`}</span>
        <Button type="button" variant="outline" className="bg-card" disabled={count === 0} onClick={() => setForm(was)}>
          Reset
        </Button>
        <Button type="submit" disabled={count === 0 || save.isPending} data-testid="settings-save">
          <SaveIcon /> {save.isPending ? "Saving…" : "Save"}
        </Button>
      </div>
    </form>
  );
}

// The Send message card and its result (sqs__queue-messages, queue-messages-fifo, queue-message-sent): the form, and once a message
// is sent the SendMessage result in its place.
import { PlusIcon, SendIcon } from "lucide-react";
import { Callout } from "@doze/ui/components/app/callout";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { Field } from "../../../kit";
import { Refused } from "../deep/parts";
import type { Queue } from "./queue-model";
import { wait } from "./queue-model";
import { type Attr, type DelayUnit, NEW_ATTR, type SendForm } from "./send";

export type Sent = { MessageId: string; MD5OfMessageBody?: string; MD5OfMessageAttributes?: string; SequenceNumber?: string };

const card = "rounded-[14px] border bg-card p-6 lg:p-7";
const heading = "text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright";
const pill = (text: string) => <span className="ml-1.5 rounded-full bg-muted px-2 py-px text-[11.5px] font-normal text-muted-foreground">{text}</span>;

export function SendCard({ q, form, setForm, problem, error, pending, onSubmit }: { q: Queue; form: SendForm; setForm: (f: SendForm) => void; problem: string | null; error: unknown; pending: boolean; onSubmit: (e: React.FormEvent) => void }) {
  const setAttr = (i: number, a: Partial<Attr>) => setForm({ ...form, attrs: form.attrs.map((x, j) => (j === i ? { ...x, ...a } : x)) });
  return (
    <form onSubmit={onSubmit} className={`${card} grid gap-5`} aria-label="Send message" data-testid="send-card">
      <h2 className={heading}>Send message</h2>
      <Field label="Message body" help={<>Required: 1 character up to <b>1 MiB</b> (1,048,576 bytes), as XML, JSON or unformatted text.</>} testId="field-body">
        {(id) => <Textarea id={id} value={form.body} onChange={(e) => setForm({ ...form, body: e.target.value })} placeholder={q.fifo ? '{"sku":"FV-TOM-V","from":"bay-3","to":"aisle-B3","qty":24}' : '{"pick_list":"PL-4471","order":"ord-20418","lines":3}'} className="min-h-28 bg-background font-mono text-[13px]" spellCheck={false} data-testid="send-body" />}
      </Field>
      {q.fifo ? (
        <div className="grid gap-5 sm:grid-cols-2">
          <Field label="Message group ID" help="Required for a FIFO queue. Messages in the same group are delivered one at a time, in order; different groups can be processed in parallel." testId="field-group">
            {(id) => <Input id={id} value={form.group} onChange={(e) => setForm({ ...form, group: e.target.value })} className="bg-background font-mono text-[13px]" data-testid="send-group" />}
          </Field>
          <Field label={<>Message deduplication ID{pill(q.dedup ? "Optional" : "Required")}</>} help={q.dedup ? "Not needed here: content-based deduplication is on, so SQS hashes the body. A message with the same ID inside 5 minutes is accepted but not delivered again." : "Required: content-based deduplication is off. A message with the same ID inside 5 minutes is accepted but not delivered again."} testId="field-dedup">
            {(id) => <Input id={id} value={form.dedup} onChange={(e) => setForm({ ...form, dedup: e.target.value })} className="bg-background font-mono text-[13px]" data-testid="send-dedup" />}
          </Field>
          <Callout tone="note" className="sm:col-span-2" testId="fifo-timer-note">FIFO queues don't support timers on individual messages, so there is no delivery delay on this form. Set a delay on the queue instead.</Callout>
        </div>
      ) : (
        <Field label="Delivery delay" help={<>Hides this message for the delay after sending. Range <b>0 to 15 minutes</b>. Without a value, the queue's own delay ({wait(q.delay)}) applies.</>} testId="field-delay">
          {(id) => (
            <div className="flex items-center gap-2">
              <Input id={id} inputMode="numeric" value={form.delay} onChange={(e) => setForm({ ...form, delay: e.target.value })} placeholder="0" className="w-32 bg-background font-mono text-[13px]" data-testid="send-delay" />
              <Select value={form.delayUnit} onValueChange={(u) => setForm({ ...form, delayUnit: u as DelayUnit })}>
                <SelectTrigger aria-label="Delay unit" className="w-36 bg-background text-[13px]" data-testid="send-delay-unit"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="seconds">seconds</SelectItem><SelectItem value="minutes">minutes</SelectItem></SelectContent>
              </Select>
            </div>
          )}
        </Field>
      )}
      <fieldset className="grid gap-3">
        <legend className="mb-1 text-sm font-medium">Message attributes <span className="font-normal text-muted-foreground">Optional. Type is String, Number or Binary.</span></legend>
        {form.attrs.map((a, i) => (
          <div key={i} className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_auto] items-center gap-2" data-testid="send-attr">
            <Input aria-label="Attribute name" value={a.name} onChange={(e) => setAttr(i, { name: e.target.value })} placeholder="Name" className="bg-background font-mono text-[13px]" />
            <Select value={a.type} onValueChange={(v) => setAttr(i, { type: v as Attr["type"] })}>
              <SelectTrigger aria-label="Attribute type" className="w-full bg-background text-[13px]"><SelectValue /></SelectTrigger>
              <SelectContent>{["String", "Number", "Binary"].map((x) => <SelectItem key={x} value={x}>{x}</SelectItem>)}</SelectContent>
            </Select>
            <Input aria-label="Custom type" value={a.custom} onChange={(e) => setAttr(i, { custom: e.target.value })} placeholder="Custom type" className="bg-background font-mono text-[13px]" />
            <Input aria-label="Attribute value" value={a.value} onChange={(e) => setAttr(i, { value: e.target.value })} placeholder={a.type === "Binary" ? "Text, sent base64-encoded" : "Value"} className="bg-background font-mono text-[13px]" />
            <Button type="button" variant="ghost" size="sm" onClick={() => setForm({ ...form, attrs: form.attrs.filter((_, j) => j !== i) })} data-testid="send-remove-attr">Remove</Button>
          </div>
        ))}
        <div><Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => setForm({ ...form, attrs: [...form.attrs, NEW_ATTR] })} data-testid="send-add-attr"><PlusIcon /> Add new attribute</Button></div>
      </fieldset>
      {problem && form.body && <p className="text-[13px] text-bad">{problem}</p>}
      <Refused operation="SendMessage" error={error} testId="send-error" />
      <div className="flex justify-end border-t pt-5"><Button type="submit" disabled={problem !== null || pending} data-testid="send-submit"><SendIcon /> {pending ? "Sending…" : "Send message"}</Button></div>
    </form>
  );
}

/** What SendMessage returned, in the form's place; Send another brings the form back. */
export function SendResult({ sent, onAgain }: { sent: Sent; onAgain: () => void }) {
  return (
    <section className={card} aria-label="SendMessage result" data-testid="send-result">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className={heading}>SendMessage result</h2>
          <p className="mt-1 text-[13.5px] text-muted-foreground">You can use the MD5 digest to verify that SQS received the body you sent.</p>
        </div>
        <Button type="button" variant="outline" size="sm" className="bg-card" onClick={onAgain} data-testid="send-another"><SendIcon /> Send another message</Button>
      </div>
      <dl className="mt-4 grid grid-cols-[14rem_minmax(0,1fr)] font-mono text-[13px] [&>*]:border-b [&>*]:py-3.5 [&>:nth-last-child(-n+2)]:border-b-0">
        <dt className="text-muted-foreground">MessageId</dt><dd className="text-bright">{sent.MessageId}</dd>
        <dt className="text-muted-foreground">MD5OfMessageBody</dt><dd className="text-bright">{sent.MD5OfMessageBody ?? "–"}</dd>
        {sent.MD5OfMessageAttributes && (<><dt className="text-muted-foreground">MD5OfMessageAttributes</dt><dd className="text-bright">{sent.MD5OfMessageAttributes}</dd></>)}
        {sent.SequenceNumber && (<><dt className="text-muted-foreground">SequenceNumber</dt><dd className="text-bright">{sent.SequenceNumber}</dd></>)}
      </dl>
    </section>
  );
}

/** The Send card folded to its title once a poll is on the page. */
export function SendFolded({ onShow }: { onShow: () => void }) {
  return (
    <section className={`${card} flex flex-wrap items-center justify-between gap-3 py-4 lg:py-4`} aria-label="Send message" data-testid="send-folded">
      <h2 className={heading}>Send message</h2>
      <Button type="button" variant="outline" size="sm" className="bg-card" onClick={onShow} data-testid="send-show"><SendIcon /> Show the form</Button>
    </section>
  );
}

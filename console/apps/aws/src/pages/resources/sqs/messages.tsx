// Send and receive messages (sqs__queue-messages, queue-messages-fifo, queue-message-sent, queue-messages-polled, message-delete,
// message-deleted): send one with its result said, poll without hiding anything from the queue's consumers (ReceiveMessage with a
// visibility timeout of 0, so each poll still counts as a receive), open a message, delete what was polled.
import { ArrowLeftIcon, InboxIcon, PlusIcon, SendIcon, Trash2Icon, XIcon } from "lucide-react";
import { useState } from "react";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { Field, RuleHelp } from "../../../kit";
import { callOp, useOp } from "../deep/api";
import { Refused } from "../deep/parts";
import { MessageSheet } from "./message-sheet";
import { countOf, deletedSentence, type Message, MOST, nearLimit, polledSentence, POLL, receives, sentOf, sizeOf } from "./messages-model";
import { Block } from "./parts";
import { messages as words, wait } from "./queue-model";
import { type Attr, EMPTY, sendInput } from "./send";
import type { SubProps } from "./sub-pages";

type Sent = { MessageId: string; MD5OfMessageBody?: string; MD5OfMessageAttributes?: string; SequenceNumber?: string };

export function SqsMessages({ d, q, back }: SubProps) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const [form, setForm] = useState(EMPTY);
  const [sent, setSent] = useState<Sent | null>(null);
  const [polled, setPolled] = useState<{ ms: Message[]; waiting: number; inFlight: number } | null>(null);
  const [count, setCount] = useState("10");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<Message | null>(null);
  const [asking, setAsking] = useState<Message[] | null>(null);
  const [deleted, setDeleted] = useState<{ n: number; near: string | null } | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [delError, setDelError] = useState<unknown>(null);
  const send = useOp<Sent>(t, "SendMessage", { keep: false });
  const poll = useOp<{ Messages?: Message[] }>(t, "ReceiveMessage", { keep: true });
  const input = sendInput(form, q.fifo);
  const n = countOf(count);
  const total = q.available + q.inFlight + q.delayed;
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (typeof input === "string" || send.isPending) return;
    setDeleted(null);
    send.mutate(input, { onSuccess: (r) => { setSent(r); setForm((f) => ({ ...EMPTY, group: f.group })); } });
  };
  const run = () => {
    if (typeof n === "string") return;
    setSent(null);
    setDeleted(null);
    poll.mutate({ ...POLL, MaxNumberOfMessages: n }, { onSuccess: (r) => { setPolled({ ms: r.Messages ?? [], waiting: q.available, inFlight: q.inFlight }); setPicked(new Set()); } });
  };
  const remove = async (ms: Message[]) => {
    setDeleting(true);
    setDelError(null);
    try {
      for (const m of ms) await callOp(t, "DeleteMessage", { ReceiptHandle: m.ReceiptHandle });
      const gone = new Set(ms.map((m) => m.MessageId));
      const near = ms.filter((m) => q.dlq && q.maxReceives !== null && receives(m) >= Math.max(2, q.maxReceives - 2)).map((m) => `The deleted message was received ${receives(m)} times and would have gone to ${q.dlq} on its next failed receives.`)[0] ?? null;
      setPolled((p) => (p ? { ...p, ms: p.ms.filter((m) => !gone.has(m.MessageId)) } : p));
      setPicked(new Set());
      setOpen(null);
      setAsking(null);
      setDeleted({ n: ms.length, near });
    } catch (e) {
      setDelError(e);
    } finally {
      setDeleting(false);
    }
  };
  const setAttr = (i: number, a: Partial<Attr>) => setForm((f) => ({ ...f, attrs: f.attrs.map((x, j) => (j === i ? { ...x, ...a } : x)) }));
  const columns: DataColumn<Message>[] = [
    { id: "id", header: "Message ID", size: 330, accessorKey: "MessageId", meta: { mono: true } },
    { id: "body", header: "Body", accessorFn: (m) => m.Body, meta: { mono: true }, cell: (c) => <span className="flex items-center gap-3"><span className="truncate">{c.row.original.Body.replace(/\s+/g, " ").slice(0, 120)}</span><button type="button" className="shrink-0 font-sans text-[13px] font-medium text-link hover:underline" onClick={(e) => { e.stopPropagation(); setOpen(c.row.original); }} data-testid="message-open">More Details</button></span> },
    { id: "sent", header: "Sent date", size: 230, accessorFn: (m) => Number(m.Attributes?.SentTimestamp ?? 0), cell: (c) => sentOf(c.row.original) },
    { id: "size", header: "Size", size: 100, accessorFn: (m) => m.Body.length, meta: { numeric: true }, cell: (c) => sizeOf(c.row.original) },
    { id: "count", header: "Receive count", size: 150, accessorFn: (m) => receives(m), meta: { numeric: true } },
  ];
  const near = polled ? nearLimit(polled.ms, q.dlq, q.maxReceives, q.name) : [];
  return (
    <>
      <PageHeader
        title="Send and receive messages"
        crumbs={[{ label: "Resources", to: "/resources" }, { label: d.group, to: "/resources" }, { label: d.service_title, to: `/resources/${d.service}` }, { label: d.name, to: `/resources/${d.service}/${d.kind}/${encodeURIComponent(d.name)}` }]}
        sub={<>Queue <span className="font-mono text-foreground">{q.name}</span>, {q.fifo ? `a FIFO queue${q.dedup ? " with content-based deduplication on" : ""}` : `a standard queue with ${words(q.available)} waiting`}.{polled ? ` ${polledSentence(polled.ms.length + (deleted?.n ?? 0), polled.waiting, polled.inFlight)}` : ""}</>}
        actions={<Button type="button" variant="outline" className="bg-card" onClick={back} data-testid="back-to-queue"><ArrowLeftIcon /> Back to queue</Button>}
      />
      <div className="grid max-w-[64rem] gap-8" data-testid="messages-page">
        {sent && <ResultBanner title="Message sent." returned={<>SendMessage returned MessageId <span className="font-mono">{sent.MessageId}</span>.</>} onDismiss={() => setSent(null)} testId="sent-banner" />}
        {deleted && <ResultBanner title={deletedSentence(deleted.n, q.name, polled?.ms.length ?? 0)} returned={deleted.near} onDismiss={() => setDeleted(null)} testId="deleted-banner" />}
        <Block title="Send message" testId="send-card">
          <form onSubmit={submit} className="grid gap-5 rounded-[14px] border bg-card p-6">
            <Field label="Message body" help={<>Required: 1 character up to <b>1 MiB</b> (1,048,576 bytes), as XML, JSON or unformatted text.</>} testId="field-body">
              {(id) => <Textarea id={id} value={form.body} onChange={(e) => setForm({ ...form, body: e.target.value })} placeholder='{"pick_list":"PL-4471","order":"ord-20418","lines":3}' className="min-h-28 bg-background font-mono text-[13px]" spellCheck={false} data-testid="send-body" />}
            </Field>
            {q.fifo ? (
              <div className="grid gap-5 sm:grid-cols-2">
                <Field label="Message group ID" help="Required for a FIFO queue. Messages in the same group are delivered one at a time, in order; different groups can be processed in parallel." testId="field-group">
                  {(id) => <Input id={id} value={form.group} onChange={(e) => setForm({ ...form, group: e.target.value })} className="bg-background font-mono text-[13px]" data-testid="send-group" />}
                </Field>
                <Field label="Message deduplication ID" help={q.dedup ? "Not needed here: content-based deduplication is on, so SQS hashes the body. A message with the same ID inside 5 minutes is accepted but not delivered again." : "Required: content-based deduplication is off. A message with the same ID inside 5 minutes is accepted but not delivered again."} testId="field-dedup">
                  {(id) => <Input id={id} value={form.dedup} onChange={(e) => setForm({ ...form, dedup: e.target.value })} className="bg-background font-mono text-[13px]" data-testid="send-dedup" />}
                </Field>
                <p className="text-[13px] text-muted-foreground sm:col-span-2">FIFO queues don't support timers on individual messages, so there is no delivery delay on this form. Set a delay on the queue instead.</p>
              </div>
            ) : (
              <Field label="Delivery delay" help={<>Hides this message for the delay after sending. Range <b>0 to 15 minutes</b>. Without a value, the queue's own delay ({wait(q.delay)}) applies.</>} testId="field-delay">
                {(id) => <div className="flex items-center gap-2"><Input id={id} inputMode="numeric" value={form.delay} onChange={(e) => setForm({ ...form, delay: e.target.value })} className="w-32 bg-background font-mono text-[13px]" data-testid="send-delay" /><span className="text-[13.5px] text-muted-foreground">seconds</span></div>}
              </Field>
            )}
            <fieldset className="grid gap-2">
              <legend className="mb-1 text-sm font-medium">Message attributes <span className="font-normal text-muted-foreground">Optional. Type is String, Number or Binary.</span></legend>
              {form.attrs.map((a, i) => (
                <div key={i} className="grid grid-cols-[8rem_minmax(0,1fr)_minmax(0,1fr)_auto] items-center gap-2" data-testid="send-attr">
                  <Select value={a.type} onValueChange={(v) => setAttr(i, { type: v as Attr["type"] })}>
                    <SelectTrigger aria-label="Attribute type" className="w-full bg-background text-[13px]"><SelectValue /></SelectTrigger>
                    <SelectContent>{["String", "Number", "Binary"].map((x) => <SelectItem key={x} value={x}>{x}</SelectItem>)}</SelectContent>
                  </Select>
                  <Input aria-label="Attribute name" value={a.name} onChange={(e) => setAttr(i, { name: e.target.value })} placeholder="Name" className="bg-background font-mono text-[13px]" />
                  <Input aria-label="Attribute value" value={a.value} onChange={(e) => setAttr(i, { value: e.target.value })} placeholder={a.type === "Binary" ? "Text, sent base64-encoded" : "Value"} className="bg-background font-mono text-[13px]" />
                  <Button type="button" variant="ghost" size="icon" aria-label="Remove attribute" onClick={() => setForm((f) => ({ ...f, attrs: f.attrs.filter((_, j) => j !== i) }))}><XIcon /></Button>
                </div>
              ))}
              <div><Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => setForm((f) => ({ ...f, attrs: [...f.attrs, { name: "", type: "String", value: "" }] }))} data-testid="send-add-attr"><PlusIcon /> Add new attribute</Button></div>
            </fieldset>
            {typeof input === "string" && form.body && <p className="text-[13px] text-bad">{input}</p>}
            <Refused operation="SendMessage" error={send.error} testId="send-error" />
            <div className="flex justify-end border-t pt-5"><Button type="submit" disabled={typeof input === "string" || send.isPending} data-testid="send-submit"><SendIcon /> {send.isPending ? "Sending…" : "Send message"}</Button></div>
          </form>
          {sent && (
            <section className="rounded-[14px] border bg-card p-6" aria-label="SendMessage result" data-testid="send-result">
              <h3 className="text-[15px] font-semibold text-bright">SendMessage result</h3>
              <p className="mt-1 text-[13px] text-muted-foreground">You can use the MD5 digest to verify that SQS received the body you sent.</p>
              <dl className="mt-3 grid grid-cols-[12rem_minmax(0,1fr)] gap-y-2 font-mono text-[13px]">
                <dt className="text-muted-foreground">MessageId</dt><dd className="text-bright">{sent.MessageId}</dd>
                <dt className="text-muted-foreground">MD5OfMessageBody</dt><dd className="text-bright">{sent.MD5OfMessageBody ?? "–"}</dd>
                {sent.MD5OfMessageAttributes && (<><dt className="text-muted-foreground">MD5OfMessageAttributes</dt><dd className="text-bright">{sent.MD5OfMessageAttributes}</dd></>)}
                {sent.SequenceNumber && (<><dt className="text-muted-foreground">SequenceNumber</dt><dd className="text-bright">{sent.SequenceNumber}</dd></>)}
              </dl>
            </section>
          )}
        </Block>
        <Block title="Receive messages" testId="receive-card">
          <div className="grid gap-4 rounded-[14px] border bg-card p-6">
            <Field label="Maximum message count" error={typeof n === "string" ? n : undefined} help={<>1 to {MOST} (MaxNumberOfMessages).</>} testId="field-count">
              {(id) => <Input id={id} inputMode="numeric" value={count} onChange={(e) => setCount(e.target.value)} className="w-28 bg-background font-mono text-[13px]" data-testid="poll-count" />}
            </Field>
            <div><Button type="button" onClick={run} disabled={poll.isPending || typeof n === "string"} data-testid="poll">{poll.isPending ? "Polling…" : "Poll for messages"}</Button></div>
            <RuleHelp>
              {q.fifo ? "A FIFO queue hands out messages group by group; while one is in flight, later messages in its group wait. " : ""}
              Polling counts as receiving.{q.dlq && q.maxReceives !== null ? <> {q.name} moves a message to <span className="font-mono">{q.dlq}</span> once its receive count exceeds {q.maxReceives}, and viewing a message in the console is a receive.</> : null} Messages are not deleted when received; you delete them yourself. This console polls with a visibility timeout of 0, so the queue's consumers still get them.
            </RuleHelp>
            <Refused operation="ReceiveMessage" error={poll.error} testId="poll-error" />
          </div>
          {polled && polled.ms.length > 0 ? (
            <div className="grid gap-3" data-testid="polled">
              <h3 className="text-[17px] font-semibold text-bright">Messages ({polled.ms.length})</h3>
              <Refused operation="DeleteMessage" error={delError} testId="delete-message-error" />
              <SelectableTable aria-label="Messages" data={polled.ms} columns={columns} rowId={(m) => m.MessageId} selected={picked} onSelectedChange={setPicked} noun="message" hint="Choose a message to delete it." maxHeight={440}
                actions={(chosen) => <Button variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => setAsking(chosen)} data-testid="delete-picked"><Trash2Icon /> Delete</Button>} />
              {near.map((w) => <p key={w} className="text-[13px] text-warn" data-testid="near-limit">{w}</p>)}
            </div>
          ) : polled ? (
            <EmptyTab icon={InboxIcon} title="No messages yet" testId="no-messages">{total === 0 ? "Send a message above, then poll. Messages stay in the queue after you receive them until you delete them." : "No message is waiting. A message in flight, or still delayed, is not seen by a poll."}</EmptyTab>
          ) : total === 0 ? (
            <EmptyTab icon={InboxIcon} title="No messages yet" testId="no-messages">Send a message above, then poll. Messages stay in the queue after you receive them until you delete them.</EmptyTab>
          ) : null}
        </Block>
      </div>
      <MessageSheet m={open} onClose={() => setOpen(null)} onDelete={(m) => setAsking([m])} />
      <Dialog open={asking !== null} onOpenChange={(o) => { if (!o) { setAsking(null); setDelError(null); } }}>
        <DialogContent className="sm:max-w-[560px]" data-testid="delete-message-dialog">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Delete messages</DialogTitle>
            <DialogDescription className="text-[14px] leading-[22px] text-foreground">Delete {words(asking?.length ?? 0)} from <b className="font-mono font-semibold text-bright">{q.name}</b>?</DialogDescription>
          </DialogHeader>
          {asking && asking.length <= 3 && <dl className="grid grid-cols-[8rem_minmax(0,1fr)] gap-y-1 text-[13px]">{asking.map((m) => (<div key={m.MessageId} className="contents"><dt className="text-muted-foreground">Message ID</dt><dd className="font-mono text-bright">{m.MessageId}</dd><dt className="text-muted-foreground">Receive count</dt><dd className="text-bright">{receives(m)}</dd></div>))}</dl>}
          <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-foreground">SQS deletes with the receipt handle from the most recent receive, even if a visibility timeout is hiding the message from other consumers. A deleted message can't be recovered. Standard queues can, rarely, hand out a message again after it is deleted, so consumers should be idempotent.</p>
          <Refused operation="DeleteMessage" error={delError} testId="delete-message-dialog-error" />
          <DialogFooter>
            <Label className="sr-only">Actions</Label>
            <Button type="button" variant="outline" className="bg-card" onClick={() => setAsking(null)}>Cancel</Button>
            <Button type="button" variant="destructive" disabled={deleting} onClick={() => asking && void remove(asking)} data-testid="delete-message-confirm">{deleting ? "Working…" : "Delete"}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

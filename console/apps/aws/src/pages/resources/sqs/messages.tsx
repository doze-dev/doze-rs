// Send and receive messages (sqs__queue-messages, queue-messages-fifo, queue-message-sent, queue-messages-polled, message-detail,
// message-delete, message-deleted): send one with its result said in the form's place, poll without hiding anything from the queue's
// consumers (ReceiveMessage with a visibility timeout of 0, so each poll still counts as a receive), open a message, delete what was
// polled. The cards are in ./message-send.tsx and ./message-receive.tsx.
import { ArrowLeftIcon } from "lucide-react";
import { useState } from "react";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { callOp, useOp } from "../deep/api";
import { DeleteMessages, NoMessages, Polled, ReceiveCard } from "./message-receive";
import { type Sent, SendCard, SendResult } from "./message-send";
import { MessageSheet } from "./message-sheet";
import { countOf, deletedBanner, deletedNote, leftSentence, type Message, nearLimit, polledSentence, POLL, receives } from "./messages-model";
import { messages as words } from "./queue-model";
import { EMPTY, sendInput } from "./send";
import type { SubProps } from "./sub-pages";

type Deleted = { n: number; received: number | null };

export function SqsMessages({ d, q, back }: SubProps) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const [form, setForm] = useState(EMPTY);
  const [sent, setSent] = useState<Sent | null>(null);
  // A poll drops the Send form and shrinks the Receive card to its one-line bar (the boards of a poll show that and what it returned); "Send a message" on the bar brings the form back.
  const [folded, setFolded] = useState(false);
  const [polled, setPolled] = useState<{ ms: Message[]; waiting: number; inFlight: number } | null>(null);
  const [count, setCount] = useState("10");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<Message | null>(null);
  const [asking, setAsking] = useState<Message[] | null>(null);
  const [deleted, setDeleted] = useState<Deleted | null>(null);
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
    setFolded(true);
    poll.mutate({ ...POLL, MaxNumberOfMessages: n }, { onSuccess: (r) => { setPolled({ ms: r.Messages ?? [], waiting: q.available, inFlight: q.inFlight }); setPicked(new Set()); } });
  };
  const remove = async (ms: Message[]) => {
    setDeleting(true);
    setDelError(null);
    try {
      for (const m of ms) await callOp(t, "DeleteMessage", { ReceiptHandle: m.ReceiptHandle });
      const gone = new Set(ms.map((m) => m.MessageId));
      const close = ms.filter((m) => q.dlq && q.maxReceives !== null && receives(m) >= Math.max(2, q.maxReceives - 2));
      setPolled((p) => (p ? { ...p, ms: p.ms.filter((m) => !gone.has(m.MessageId)) } : p));
      setPicked(new Set());
      setOpen(null);
      setAsking(null);
      setDeleted({ n: ms.length, received: close[0] ? receives(close[0]) : null });
    } catch (e) {
      setDelError(e);
    } finally {
      setDeleting(false);
    }
  };
  const left = polled?.ms.length ?? 0;
  const banner = deleted ? deletedBanner(deleted.n, q.name, left) : null;
  const subtitle = polled ? (deleted ? leftSentence(left) : polledSentence(left, polled.waiting, polled.inFlight)) : "";
  const empty = polled && polled.ms.length === 0 && !deleted ? <NoMessages anyHere={total > 0} /> : !polled && total === 0 ? <NoMessages anyHere={false} /> : null;
  return (
    <>
      <PageHeader
        title="Send and receive messages"
        crumbs={[{ label: "Resources", to: "/resources" }, { label: d.group, to: "/resources" }, { label: d.service_title, to: `/resources/${d.service}` }, { label: d.name, to: `/resources/${d.service}/${d.kind}/${encodeURIComponent(d.name)}` }]}
        sub={<>Queue <span className="font-mono text-foreground">{q.name}</span>{polled ? "" : ", "}{polled ? null : q.fifo ? `a FIFO queue${q.dedup ? " with content-based deduplication on" : ""}` : `a standard queue with ${words(q.available)} waiting`}.{subtitle ? ` ${subtitle}` : ""}</>}
        actions={<Button type="button" variant="outline" className="bg-card" onClick={back} data-testid="back-to-queue"><ArrowLeftIcon /> Back to queue</Button>}
      />
      <div className="grid gap-6" data-testid="messages-page">
        {sent && <ResultBanner title="Message sent." returned={<>SendMessage returned MessageId <span className="font-mono">{sent.MessageId}</span>.</>} onDismiss={() => setSent(null)} testId="sent-banner" />}
        {banner && <ResultBanner title={banner.title} returned={banner.returned} onDismiss={() => setDeleted(null)} testId="deleted-banner" />}
        {sent ? <SendResult sent={sent} onAgain={() => setSent(null)} /> : folded ? null : <SendCard q={q} form={form} setForm={setForm} problem={typeof input === "string" ? input : null} error={send.error} pending={send.isPending} onSubmit={submit} />}
        <ReceiveCard q={q} count={count} setCount={setCount} countProblem={typeof n === "string" ? n : undefined} primary={sent !== null} pending={poll.isPending} error={poll.error} empty={empty} onPoll={run} onSend={folded && !sent ? () => setFolded(false) : undefined} />
        {polled && (polled.ms.length > 0 || deleted) && (
          <Polled q={q} ms={polled.ms} picked={picked} setPicked={setPicked} near={nearLimit(polled.ms, q.dlq, q.maxReceives, q.name)} note={deleted ? deletedNote(deleted.received, q.dlq) : null} onOpen={setOpen} onDelete={setAsking} error={delError} />
        )}
      </div>
      <MessageSheet m={open} onClose={() => setOpen(null)} />
      <DeleteMessages asking={asking} q={q} deleting={deleting} error={delError} onClose={() => { setAsking(null); setDelError(null); }} onConfirm={(ms) => void remove(ms)} />
    </>
  );
}

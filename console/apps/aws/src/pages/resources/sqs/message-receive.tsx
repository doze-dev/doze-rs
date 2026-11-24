// The Receive messages card, the table of what a poll returned and the delete confirm (sqs__queue-messages, queue-messages-polled,
// message-delete, message-deleted).
import { InboxIcon, Trash2Icon } from "lucide-react";
import { Callout } from "@doze/ui/components/app/callout";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Field, RuleHelp } from "../../../kit";
import { Refused } from "../deep/parts";
import { type Message, MOST, receives, sentOf, sizeOf } from "./messages-model";
import { messages as words, type Queue } from "./queue-model";

const card = "rounded-[var(--radius-card)] border bg-card p-6 lg:p-7";
const heading = "text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright";

export function ReceiveCard({ q, count, setCount, countProblem, primary, pending, error, empty, onPoll, onSend }: { q: Queue; count: string; setCount: (v: string) => void; countProblem: string | undefined; primary: boolean; pending: boolean; error: unknown; empty: React.ReactNode; onPoll: () => void; /** Set once a poll is on the page: the card is the one-line bar of the boards, and this brings the Send form back. */ onSend?: () => void }) {
  if (onSend) {
    return (
      <section className={`${card} grid gap-3`} aria-label="Receive messages" data-testid="receive-card" data-compact="">
        <h2 className={heading}>Receive messages</h2>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <Button type="button" variant={primary ? "default" : "outline"} className={primary ? "" : "bg-card"} onClick={onPoll} disabled={pending || countProblem !== undefined} data-testid="poll">{pending ? "Polling…" : "Poll for messages"}</Button>
          <label className="flex items-center gap-2 text-[13px] text-muted-foreground">Maximum message count<Input inputMode="numeric" value={count} onChange={(e) => setCount(e.target.value)} aria-invalid={countProblem ? true : undefined} className="h-8 w-16 bg-background font-mono text-[13px] text-foreground" data-testid="poll-count" /></label>
          {countProblem && <span className="text-[13px] text-bad" role="alert">{countProblem}</span>}
          <Button type="button" variant="ghost" size="sm" className="ml-auto text-muted-foreground" onClick={onSend} data-testid="send-show">Send a message</Button>
        </div>
        <Refused operation="ReceiveMessage" error={error} testId="poll-error" />
        {empty}
      </section>
    );
  }
  return (
    <section className={`${card} grid gap-4`} aria-label="Receive messages" data-testid="receive-card">
      <h2 className={heading}>Receive messages</h2>
      <Field label="Maximum message count" error={countProblem} help={<>1 to {MOST} (MaxNumberOfMessages).</>} testId="field-count">
        {(id) => <Input id={id} inputMode="numeric" value={count} onChange={(e) => setCount(e.target.value)} className="w-28 bg-background font-mono text-[13px]" data-testid="poll-count" />}
      </Field>
      <div><Button type="button" variant={primary ? "default" : "outline"} className={primary ? "" : "bg-card"} onClick={onPoll} disabled={pending || countProblem !== undefined} data-testid="poll">{pending ? "Polling…" : "Poll for messages"}</Button></div>
      <RuleHelp>
        {q.fifo ? "A FIFO queue hands out messages group by group; while one is in flight, later messages in its group wait. " : ""}
        Polling counts as receiving.{q.dlq && q.maxReceives !== null ? <> {q.name} moves a message to <span className="font-mono">{q.dlq}</span> once its receive count exceeds <b>{q.maxReceives}</b>, and viewing a message in the console is a receive.</> : null} Messages are not deleted when received; you delete them yourself. This console polls with a visibility timeout of 0, so the queue's consumers still get them.
      </RuleHelp>
      <Refused operation="ReceiveMessage" error={error} testId="poll-error" />
      {empty}
    </section>
  );
}

/** What a poll came back with when nothing was waiting (or nothing was sent yet). */
export function NoMessages({ anyHere }: { anyHere: boolean }) {
  return (
    <EmptyTab icon={InboxIcon} title="No messages yet" testId="no-messages">
      {anyHere ? "No message is waiting. A message in flight, or still delayed, is not seen by a poll." : "Send a message above, then poll. Messages stay in the queue after you receive them until you delete them."}
    </EmptyTab>
  );
}

export function Polled({ q, ms, picked, setPicked, near, note, onOpen, onDelete, error }: { q: Queue; ms: Message[]; picked: Set<string>; setPicked: (s: Set<string>) => void; near: { lead: string; text: string } | null; note: string | null; onOpen: (m: Message) => void; onDelete: (ms: Message[]) => void; error: unknown }) {
  const columns: DataColumn<Message>[] = [
    { id: "id", header: "Message ID", size: 330, accessorKey: "MessageId", meta: { mono: true } },
    { id: "body", header: "Body", accessorFn: (m) => m.Body, cell: (c) => <button type="button" className="text-[13.5px] font-medium text-link hover:underline" onClick={(e) => { e.stopPropagation(); onOpen(c.row.original); }} data-testid="message-open">More Details</button> },
    { id: "sent", header: "Sent date", size: 230, accessorFn: (m) => Number(m.Attributes?.SentTimestamp ?? 0), cell: (c) => sentOf(c.row.original) },
    { id: "size", header: "Size", size: 120, accessorFn: (m) => m.Body.length, cell: (c) => sizeOf(c.row.original) },
    { id: "count", header: "Receive count", size: 150, accessorFn: (m) => receives(m), meta: { numeric: true } },
  ];
  return (
    <section className="grid gap-3" aria-label="Messages" data-testid="polled">
      <h2 className={heading}>Messages ({ms.length})</h2>
      <Refused operation="DeleteMessage" error={error} testId="delete-message-error" />
      <SelectableTable aria-label="Messages" data={ms} columns={columns} rowId={(m) => m.MessageId} selected={picked} onSelectedChange={setPicked} noun="message" hint="None selected" selectedLabel={(n) => `${n} selected`} actionsEnd actionsAlways maxHeight={440} empty="No message is left from this poll."
        actions={(chosen) => <Button variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={chosen.length === 0} onClick={() => onDelete(chosen)} data-testid="delete-picked"><Trash2Icon /> Delete</Button>} />
      {near && <Callout tone="warn" lead={near.lead} className="mt-3" testId="near-limit">{near.text}</Callout>}
      {note && <p className="mt-2 border-l-2 pl-3.5 text-[13.5px] leading-[21px] text-muted-foreground" data-testid="deleted-note"><Emphasis text={note} names={q.dlq ? [q.dlq] : []} /></p>}
    </section>
  );
}

export function DeleteMessages({ asking, q, deleting, error, onClose, onConfirm }: { asking: Message[] | null; q: Queue; deleting: boolean; error: unknown; onClose: () => void; onConfirm: (ms: Message[]) => void }) {
  return (
    <Dialog open={asking !== null} onOpenChange={(o) => { if (!o) onClose(); }}>
      <DialogContent className="sm:max-w-[560px]" data-testid="delete-message-dialog">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">Delete messages</DialogTitle>
          <DialogDescription className="text-[14px] leading-[22px] text-foreground">Delete {words(asking?.length ?? 0)} from <b className="font-mono font-semibold text-bright">{q.name}</b>?</DialogDescription>
        </DialogHeader>
        {asking && asking.length <= 3 && <dl className="grid grid-cols-[8rem_minmax(0,1fr)] gap-y-1 text-[13px]">{asking.map((m) => (<div key={m.MessageId} className="contents"><dt className="text-muted-foreground">Message ID</dt><dd className="font-mono text-bright">{m.MessageId}</dd><dt className="text-muted-foreground">Receive count</dt><dd className="text-bright">{receives(m)}</dd></div>))}</dl>}
        <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-foreground">SQS deletes with the receipt handle from the most recent receive, even if a visibility timeout is hiding the message from other consumers. A deleted message can't be recovered. Standard queues can, rarely, hand out a message again after it is deleted, so consumers should be idempotent.</p>
        <Refused operation="DeleteMessage" error={error} testId="delete-message-dialog-error" />
        <DialogFooter>
          <Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button>
          <Button type="button" variant="destructive" disabled={deleting} onClick={() => asking && onConfirm(asking)} data-testid="delete-message-confirm">{deleting ? "Working…" : "Delete"}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

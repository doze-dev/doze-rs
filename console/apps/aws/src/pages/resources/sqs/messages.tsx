// A queue's messages: send one, peek at what waits (ReceiveMessage with a visibility timeout of 0, so the queue's consumers
// still get them; each peek counts as a receive), open one, delete what was peeked, purge the queue.
import { EyeIcon, Trash2Icon } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { Alert } from "@doze/ui/components/app/alert";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@doze/ui/components/ui/sheet";
import { bytes, dateTimeUtc } from "@doze/ui/lib/format";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { type Target, callOp, useOp } from "../deep/api";
import type { DeepProps } from "../deep/registry";
import { Card, Confirm, JsonBlock, Refused, pretty, useOpen } from "../deep/parts";
import { SendCard } from "./send";

export type Message = {
  MessageId: string;
  ReceiptHandle: string;
  Body: string;
  MD5OfBody?: string;
  Attributes?: Record<string, string>;
  MessageAttributes?: Record<string, { DataType: string; StringValue?: string; BinaryValue?: string }>;
};

/** The value of a field of the queue's details, as shown. */
export const fieldOf = (d: ResourceDetail, label: string) => d.fields.find((f) => f.label === label)?.value ?? null;

const PEEK = { MaxNumberOfMessages: 10, MessageSystemAttributeNames: ["All"], MessageAttributeNames: ["All"] };

function sent(m: Message): string {
  const at = Number(m.Attributes?.SentTimestamp);
  return Number.isFinite(at) && at > 0 ? dateTimeUtc(at) : "–";
}

function MessageSheet({ m, onClose, onDelete, deleting }: { m: Message | null; onClose: () => void; onDelete: (m: Message) => void; deleting: boolean }) {
  return (
    <Sheet open={m !== null} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-[560px]" data-testid="message-sheet">
        {m && (
          <>
            <SheetHeader className="border-b px-7 pt-7 pb-5">
              <SheetTitle className="text-[21px] font-semibold">Message</SheetTitle>
              <SheetDescription className="font-mono text-[12.5px] break-all">{m.MessageId}</SheetDescription>
            </SheetHeader>
            <div className="grid gap-6 px-7 py-6">
              <section className="grid gap-2">
                <div className="flex items-center justify-between">
                  <h3 className="text-[13.5px] text-muted-foreground">Body · {bytes(new TextEncoder().encode(m.Body).length)}</h3>
                  <CopyButton text={m.Body} what="Body copied" className="bg-card" />
                </div>
                <JsonBlock value={pretty(m.Body)} testId="message-body" />
              </section>
              {m.MessageAttributes && Object.keys(m.MessageAttributes).length > 0 && (
                <section className="grid gap-2">
                  <h3 className="text-[13.5px] text-muted-foreground">Message attributes</h3>
                  <dl className="grid grid-cols-[minmax(8rem,12rem)_minmax(0,1fr)] text-[13.5px]">
                    {Object.entries(m.MessageAttributes).map(([k, v]) => (
                      <div key={k} className="contents">
                        <dt className="border-b py-2 font-mono text-muted-foreground">{k}</dt>
                        <dd className="border-b py-2 font-mono break-all text-bright">
                          {v.StringValue ?? v.BinaryValue} <span className="text-muted-foreground">({v.DataType})</span>
                        </dd>
                      </div>
                    ))}
                  </dl>
                </section>
              )}
              <section className="grid gap-2">
                <h3 className="text-[13.5px] text-muted-foreground">Attributes</h3>
                <dl className="grid grid-cols-[minmax(8rem,16rem)_minmax(0,1fr)] text-[13.5px]">
                  {Object.entries(m.Attributes ?? {}).map(([k, v]) => (
                    <div key={k} className="contents">
                      <dt className="border-b py-2 text-muted-foreground">{k}</dt>
                      <dd className="border-b py-2 font-mono break-all text-bright">{k.endsWith("Timestamp") && Number(v) > 0 ? dateTimeUtc(Number(v)) : v}</dd>
                    </div>
                  ))}
                </dl>
              </section>
              <div className="flex justify-end border-t pt-5">
                <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={deleting} onClick={() => onDelete(m)} data-testid="message-delete">
                  <Trash2Icon /> {deleting ? "Deleting…" : "Delete message"}
                </Button>
              </div>
            </div>
          </>
        )}
      </SheetContent>
    </Sheet>
  );
}

/** Peek, the list of what was peeked, delete. */
function Peek({ t, d, send }: { t: Target; d: ResourceDetail; send: React.ReactNode }) {
  const [messages, setMessages] = useState<Message[] | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<Message | null>(null);
  const peek = useOp<{ Messages?: Message[] }>(t, "ReceiveMessage");
  const qc = useQueryClient();
  const [deleting, setDeleting] = useState(false);
  const [delError, setDelError] = useState<unknown>(null);
  const run = () => peek.mutate(PEEK, { onSuccess: (r) => (setMessages(r.Messages ?? []), setPicked(new Set())) });
  const remove = async (ms: Message[]) => {
    setDeleting(true);
    setDelError(null);
    try {
      for (const m of ms) await callOp(t, "DeleteMessage", { ReceiptHandle: m.ReceiptHandle });
      const gone = new Set(ms.map((m) => m.MessageId));
      setMessages((list) => (list ?? []).filter((m) => !gone.has(m.MessageId)));
      setPicked(new Set());
      setOpen(null);
      toast.success(ms.length === 1 ? "Message deleted" : `${ms.length} messages deleted`);
      void qc.invalidateQueries({ queryKey: ["resources", "one", t.service, t.kind, t.name] });
    } catch (e) {
      setDelError(e);
    } finally {
      setDeleting(false);
    }
  };
  const max = fieldOf(d, "Maximum receives");
  const dlq = fieldOf(d, "Dead-letter queue");
  const columns: DataColumn<Message>[] = [
    { id: "pick", header: "", size: 44, enableSorting: false, cell: (c) => <Checkbox aria-label={`Select ${c.row.original.MessageId}`} checked={picked.has(c.row.original.MessageId)} onClick={(e) => e.stopPropagation()} onCheckedChange={(on) => setPicked((p) => { const n = new Set(p); if (on) n.add(c.row.original.MessageId); else n.delete(c.row.original.MessageId); return n; })} /> },
    { id: "id", header: "Message ID", size: 330, accessorKey: "MessageId", meta: { mono: true } },
    { id: "body", header: "Body", accessorFn: (m) => m.Body, meta: { mono: true }, cell: (c) => <span className="text-link">{c.row.original.Body.replace(/\s+/g, " ").slice(0, 160)}</span> },
    { id: "sent", header: "Sent", size: 230, accessorFn: (m) => Number(m.Attributes?.SentTimestamp ?? 0), cell: (c) => sent(c.row.original) },
    { id: "size", header: "Size", size: 100, accessorFn: (m) => m.Body.length, meta: { numeric: true }, cell: (c) => bytes(new TextEncoder().encode(c.row.original.Body).length) },
    { id: "count", header: "Receives", size: 110, accessorFn: (m) => Number(m.Attributes?.ApproximateReceiveCount ?? 0), meta: { numeric: true } },
  ];
  const chosen = (messages ?? []).filter((m) => picked.has(m.MessageId));
  return (
    <>
      <div className="grid items-start gap-6 xl:grid-cols-2">
        {send}
        <Card
          title="Peek at messages"
          lede={<>Reads up to 10 waiting messages with <span className="font-mono">ReceiveMessage</span> and a visibility timeout of 0: they stay visible to the queue's consumers. Messages in flight with a consumer are not seen.</>}
          actions={
            <Button onClick={run} disabled={peek.isPending} data-testid="peek">
              <EyeIcon /> {peek.isPending ? "Peeking…" : messages ? "Peek again" : "Peek"}
            </Button>
          }
          testId="peek-card"
        >
          <div className="grid gap-4">
            <Alert tone="warn" title="A peek counts as a receive" testId="peek-note">
              Each message's receive count goes up by one{max && dlq ? <>, and this queue moves a message to <span className="font-mono">{dlq}</span> once its count passes {max}</> : null}. Peek on a queue an app consumes from with care.
            </Alert>
            <Refused operation="ReceiveMessage" error={peek.error} />
          </div>
        </Card>
      </div>
      {messages && (
        <section className="grid gap-4" aria-label="Messages peeked" data-testid="peeked">
          <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Messages peeked ({messages.length})</h2>
          <div className="flex min-h-9 flex-wrap items-center gap-3">
            <span className="text-[13px] text-muted-foreground">{chosen.length > 0 ? `${chosen.length} selected` : "Choose a message to open it; tick to delete."}</span>
            <span className="flex-1" />
            <Button variant="outline" size="sm" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={chosen.length === 0 || deleting} onClick={() => void remove(chosen)} data-testid="delete-picked">
              <Trash2Icon /> Delete{chosen.length > 1 ? ` ${chosen.length}` : ""}
            </Button>
          </div>
          <Refused operation="DeleteMessage" error={delError} testId="delete-message-error" />
          <DataTable aria-label="Messages peeked" data={messages} columns={columns} rowId={(m) => m.MessageId} onRowClick={setOpen} maxHeight={440} empty="No message is waiting. A message in flight, or still delayed, is not seen by a peek." />
        </section>
      )}
      <MessageSheet m={open} onClose={() => setOpen(null)} onDelete={(m) => void remove([m])} deleting={deleting} />
    </>
  );
}

export function QueueMessages({ d }: DeepProps) {
  const t = { service: d.service, kind: d.kind, name: d.name };
  const fifo = d.name.endsWith(".fifo");
  const purge = useOp(t, "PurgeQueue");
  const [purging, setPurging] = useOpen(() => purge.reset());
  const available = fieldOf(d, "Messages available");
  return (
    <div className="grid gap-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-[14.5px] text-muted-foreground" data-testid="queue-sentence">
          <span className="font-mono font-semibold text-bright">{d.name}</span> holds <b className="text-bright">{available ?? "?"}</b> available, <b className="text-bright">{fieldOf(d, "Messages in flight") ?? "?"}</b> in flight and <b className="text-bright">{fieldOf(d, "Messages delayed") ?? "?"}</b> delayed, as last read.
        </p>
        <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => setPurging(true)} data-testid="purge-open">
          <Trash2Icon /> Purge
        </Button>
      </div>
      <Peek t={t} d={d} send={<SendCard t={t} fifo={fifo} dedupByContent={fieldOf(d, "Content-based deduplication") === "Enabled"} />} />
      <Confirm
        open={purging}
        onOpenChange={setPurging}
        title="Purge queue"
        verb="Purge"
        operation="PurgeQueue"
        pending={purge.isPending}
        error={purge.error}
        run={() => purge.mutate({}, { onSuccess: () => (setPurging(false), toast.success(`${d.name} purged`)) })}
        testId="purge-dialog"
      >
        Delete every message in <b className="font-mono font-semibold text-bright">{d.name}</b>, in flight and delayed ones too? SQS allows one purge per queue every 60 seconds, and it cannot be undone.
      </Confirm>
    </div>
  );
}

// What a queue's page is made of beyond its tabs: the Details card (sqs__queue-detail) and the recent calls.
import { Link } from "@tanstack/react-router";
import { RadioIcon } from "lucide-react";
import { useState } from "react";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { clockUtc } from "@doze/ui/lib/format";
import type { CallRecord } from "../../../api/types/CallRecord";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { isConsole } from "../../live/search";
import { Result } from "../../live/outcome";
import { ResourceTo } from "../value";
import { kib, messages, type Queue, wait } from "./queue-model";

/** A titled block of a queue's page. */
export function Block({ title, lede, children, testId, actions }: { title: string; lede?: React.ReactNode; children: React.ReactNode; testId?: string; actions?: React.ReactNode }) {
  return (
    <section className="grid gap-3" aria-label={title} data-testid={testId}>
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
          {lede && <p className="mt-1 text-[13.5px] text-muted-foreground">{lede}</p>}
        </div>
        {actions}
      </div>
      {children}
    </section>
  );
}

/** A label and its value, as the Details card sets them out. */
export function Pair({ label, children, testId }: { label: string; children: React.ReactNode; testId?: string }) {
  return (
    <div className="contents">
      <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{label}</dt>
      <dd className="min-w-0 border-b py-3 text-[13.5px] break-words text-bright" data-testid={testId ?? `field-${label}`}>{children}</dd>
    </div>
  );
}

/** The queue's details: its ARN and URL, then its settings in two columns. */
export function DetailsCard({ q }: { q: Queue }) {
  const left: [string, React.ReactNode][] = [
    ["Name", <span key="n" className="font-mono">{q.name}</span>],
    ["Type", q.fifo ? "FIFO" : "Standard"],
    ["Encryption", q.sse === "none" ? "Disabled" : "Enabled"],
    ["Dead-letter queue", q.dlq ? <ResourceTo key="d" to={{ service: "sqs", kind: "queues", name: q.dlq }} className="font-mono">{q.dlq}</ResourceTo> : "–"],
    ["Approx. messages", `${q.available.toLocaleString("en-US")} available, ${q.inFlight.toLocaleString("en-US")} in flight, ${q.delayed.toLocaleString("en-US")} delayed`],
    ...(q.fifo ? ([["Content-based deduplication", q.dedup ? "Enabled" : "Disabled"]] as [string, string][]) : []),
  ];
  const right: [string, React.ReactNode][] = [
    ["Visibility timeout", wait(q.visibility)],
    ["Message retention period", wait(q.retention)],
    ["Delivery delay", wait(q.delay)],
    ["Maximum message size", kib(q.maxSize)],
    ...(q.fifo ? ([["Deduplication scope", q.scope ?? "queue"], ["FIFO throughput limit", q.throughput ?? "perQueue"]] as [string, string][]) : ([["Receive message wait time", wait(q.wait)]] as [string, string][])),
  ];
  const col = "grid grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] content-start [&>div:last-child>*]:border-b-0";
  return (
    <section className="rounded-[14px] border bg-card p-6 lg:p-7" aria-label="Details" data-testid="details">
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Details</h2>
      <dl className="mt-4 grid grid-cols-[6rem_minmax(0,1fr)] gap-y-3 border-b pb-5 text-[13.5px]">
        <dt className="text-muted-foreground">ARN</dt>
        <dd className="flex items-center gap-2 font-mono text-bright" data-testid="field-ARN">{q.arn}<CopyButton text={q.arn} what="ARN copied" iconOnly variant="ghost" label="Copy ARN" /></dd>
        <dt className="text-muted-foreground">URL</dt>
        <dd className="flex items-center gap-2 font-mono break-all text-bright" data-testid="field-URL">{q.url}<CopyButton text={q.url} what="URL copied" iconOnly variant="ghost" label="Copy URL" /></dd>
      </dl>
      <div className="mt-2 grid lg:grid-cols-2 lg:gap-x-12">
        <dl className={col}>{left.map(([l, v]) => <Pair key={l} label={l}>{v}</Pair>)}</dl>
        <dl className={col}>{right.map(([l, v]) => <Pair key={l} label={l}>{v}</Pair>)}</dl>
      </div>
    </section>
  );
}

const CALLS: DataColumn<CallRecord>[] = [
  { id: "time", header: "Time", size: 120, accessorKey: "at_ms", meta: { mono: true }, cell: (c) => clockUtc(c.row.original.at_ms) },
  { id: "caller", header: "Caller", size: 220, min: 140, accessorFn: (r) => r.caller ?? "", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.caller ?? "–"}{c.row.original.via?.by ? <span className="text-muted-foreground"> via {c.row.original.via.by}</span> : null}</span> },
  { id: "call", header: "Call", accessorFn: (r) => r.action ?? "", meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.action ?? "–"}</span> },
  { id: "result", header: "Result", size: 260, min: 140, accessorFn: (r) => r.status, cell: (c) => <Result call={c.row.original} /> },
];

/** The last calls that touched the queue, newest first; this console's own reads are hidden unless asked for, as on Live. */
export function RecentCalls({ d }: { d: ResourceDetail }) {
  const [mine, setMine] = useState(false);
  const theirs = d.calls.filter((c) => !isConsole(c));
  const hidden = d.calls.length - theirs.length;
  return (
    <Block title="Recent calls" testId="recent-calls" lede={<>The last calls that touched this queue, newest first. Times are UTC.{(hidden > 0 || mine) && <span data-testid="calls-console-hidden"> {mine ? "This console's own are shown." : `${hidden} made by this console ${hidden === 1 ? "is" : "are"} hidden.`} <button type="button" className="font-medium text-link hover:underline" onClick={() => setMine(!mine)} data-testid="calls-console-toggle">{mine ? "Hide them" : "Show"}</button></span>}</>} actions={<Button asChild variant="outline" size="sm" className="bg-card"><Link to="/live"><RadioIcon /> Open Live</Link></Button>}>
      <DataTable aria-label="Recent calls" data={mine ? d.calls : theirs} columns={CALLS} rowId={(r) => String(r.seq)} maxHeight={420} empty={hidden > 0 && !mine ? "Only this console has acted on this queue so far." : "No call in the Live record acted on this queue yet."} />
    </Block>
  );
}

export const holds = (n: number): string => messages(n);

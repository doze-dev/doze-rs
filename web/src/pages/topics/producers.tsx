// A partition's Producers (board topic-producers; classic pages/producers/*): the producers the partition remembers, each
// row opening in place to the batches it keeps; what an open transaction holds back, drawn offset by offset; the aborted
// transactions the partition remembers.
import { Link, useNavigate, useParams, useSearch } from "@tanstack/react-router";
import { ChevronDownIcon } from "lucide-react";
import { useState } from "react";
import { useProducers } from "@/api/topics";
import type { ProdCell } from "@/api/types/ProdCell";
import type { ProdRow } from "@/api/types/ProdRow";
import type { ProdTail } from "@/api/types/ProdTail";
import type { ProducersView } from "@/api/types/ProducersView";
import { type Stat, StatStrip } from "@/components/app/stat";
import { StatusDot } from "@/components/app/status-dot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { number } from "@/lib/format";
import { clock, dateTime, spanMs } from "@/lib/kafka";
import { cn } from "@/lib/utils";
import { PageError } from "../page-error";

const TAIL_CELL: Record<ProdCell, { bg: string; border: string }> = {
  committed: { bg: "var(--tint-good)", border: "var(--tint-good-line)" },
  marker: { bg: "var(--tint-cool)", border: "var(--tint-cool-line)" },
  held: { bg: "var(--tint-brand)", border: "var(--tint-brand-line)" },
};

/** The tail of a partition offset by offset, the last stable offset and the high watermark marked. */
export function TailRuler({ t, lastStable, end }: { t: ProdTail; lastStable: number; end: number }) {
  const n = t.cells.length;
  const held = t.cells.filter((c) => c === "held").length;
  const at = Math.min(n, Math.max(0, lastStable - t.from));
  const gaps = 3 * Math.max(0, n - 1);
  return (
    <div className="rounded-xl border bg-card p-5" data-testid="tail-ruler">
      <div className="relative h-12">
        {held > 0 && (
          <div className="absolute top-0 flex -translate-x-1/2 flex-col items-center text-[12px] whitespace-nowrap" style={{ left: `calc((100% - ${gaps}px) * ${(at / Math.max(1, n)).toFixed(5)} + ${3 * at - 1.5}px)` }}>
            <span className="text-muted-foreground">last stable offset</span>
            <b className="font-mono font-medium text-bright">{number(lastStable)}</b>
          </div>
        )}
        <div className="absolute top-0 right-0 flex flex-col items-end text-[12px] whitespace-nowrap">
          <span className="text-muted-foreground">high watermark</span>
          <b className="font-mono font-medium text-link">{number(end)}</b>
        </div>
      </div>
      <div className="flex h-14 gap-[3px]">
        {t.cells.map((c, i) => (
          <div key={i} className="flex min-w-0 flex-1 items-end justify-center overflow-hidden rounded-md border pb-1 font-mono text-[10.5px] text-muted-foreground" style={{ background: TAIL_CELL[c].bg, borderColor: TAIL_CELL[c].border }} title={`${number(t.from + i)}: ${c}`}>
            <span className="truncate">{number(t.from + i)}</span>
          </div>
        ))}
      </div>
      <div className="mt-3 flex flex-wrap gap-x-5 gap-y-2 text-[12.5px] text-muted-foreground">
        <span className="inline-flex items-center gap-1.5">
          <i className="inline-block size-3 rounded-sm border" style={{ background: TAIL_CELL.committed.bg, borderColor: TAIL_CELL.committed.border }} /> committed records
        </span>
        <span className="inline-flex items-center gap-1.5">
          <i className="inline-block size-3 rounded-sm border" style={{ background: TAIL_CELL.marker.bg, borderColor: TAIL_CELL.marker.border }} /> a commit marker, a control record that takes an offset
        </span>
        {held > 0 && (
          <span className="inline-flex items-center gap-1.5">
            <i className="inline-block size-3 rounded-sm border" style={{ background: TAIL_CELL.held.bg, borderColor: TAIL_CELL.held.border }} /> in the open transaction: {held} records a read_committed consumer cannot see
          </span>
        )}
      </div>
    </div>
  );
}

const GRID = "grid grid-cols-[6.5rem_4rem_7.5rem_5.5rem_minmax(0,1fr)_minmax(0,1fr)_6.5rem] items-center gap-4";

function Producer({ v, p, open, toggle }: { v: ProducersView; p: ProdRow; open: boolean; toggle: () => void }) {
  const last = p.batches[0];
  return (
    <div className="border-b last:border-b-0" data-testid="producer" data-producer={p.id}>
      <div className={cn(GRID, "min-h-14 px-4 py-2 text-[13px]")}>
        <span className="font-mono font-medium text-bright">{p.id}</span>
        <span className="font-mono">{p.epoch}</span>
        <span className="font-mono">{p.last_sequence === null ? "unknown" : number(p.last_sequence)}</span>
        <span className="font-mono" title={`last write ${dateTime(p.last_used_ms)}`}>
          {spanMs(Math.max(0, v.now_ms - p.last_used_ms))}
        </span>
        <span className="min-w-0">
          {p.txn ? (
            <>
              <Link to="/topics/$topic/transactions/$txn" params={{ topic: v.topic, txn: p.txn.id }} className="truncate font-medium text-link hover:underline">
                {p.txn.id}
              </Link>
              <span className="block text-[12.5px] text-muted-foreground">
                {p.txn.state}
                {p.txn.from !== null && `, from offset ${number(p.txn.from)}`}
              </span>
            </>
          ) : (
            <>
              <span className="text-muted-foreground">none</span>
              <span className="block text-[12.5px] text-muted-foreground">idempotent only</span>
            </>
          )}
        </span>
        <span className="min-w-0">
          {p.seen_as.length ? (
            <span className="font-mono">{p.seen_as.join(", ")}</span>
          ) : (
            <>
              <span className="text-muted-foreground">not known</span>
              <span className="block text-[12.5px] text-muted-foreground">a name appears when the wire recorder has seen the producer write</span>
            </>
          )}
        </span>
        <Button variant="ghost" size="sm" className="justify-self-end" aria-expanded={open} aria-controls={`batches-${p.id}`} onClick={toggle} data-testid="batches-toggle">
          {open ? "Hide" : "Batches"}
          <ChevronDownIcon className={cn("transition-transform duration-200", !open && "-rotate-90")} />
        </Button>
      </div>
      {open && (
        <div id={`batches-${p.id}`} className="border-t bg-well px-4 py-3" data-testid="batches">
          <div className="flex flex-wrap items-baseline justify-between gap-2 text-[13px]">
            <b className="font-semibold text-bright">
              Producer {p.id}, the last {p.batches.length} {p.batches.length === 1 ? "batch" : "batches"} {v.topic}-{v.partition} remembers
            </b>
            <span className="text-muted-foreground">
              {p.txn && `open from ${p.txn.from === null ? "" : number(p.txn.from)}; `}
              {last && `last write ${clock(last.at_ms)}`}
            </span>
          </div>
          <table className="mt-3 w-full table-fixed rounded-lg bg-card text-[13px]">
            <thead className="text-left text-[12px] tracking-[0.04em] text-muted-foreground uppercase">
              <tr>
                <th className="h-9 px-3 font-medium">Sequences</th>
                <th className="px-3 font-medium">Offsets</th>
                <th className="px-3 font-medium">At</th>
              </tr>
            </thead>
            <tbody>
              {p.batches.map((b) => {
                const records = Math.max(1, b.last_offset - b.first_offset + 1);
                return (
                  <tr key={`${b.first_sequence}-${b.first_offset}`} className="border-t">
                    <td className="px-3 py-2 font-mono">
                      {number(b.first_sequence)} to {number(b.last_sequence)}
                      <span className="block text-[12px] text-muted-foreground">
                        {records} {records === 1 ? "record" : "records"}
                        {b.open && ", open"}
                      </span>
                    </td>
                    <td className="px-3 font-mono">
                      {number(b.first_offset)} to {number(b.last_offset)}
                    </td>
                    <td className="px-3 font-mono">{clock(b.at_ms)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          <p className="mt-3 text-[12.5px] text-muted-foreground">
            A batch sent again with one of these sequences is answered with where it went, not written twice. A gap or an overlap in the sequences, or a repeat older than the {Math.max(5, p.batches.length)} remembered, is{" "}
            <b className="font-semibold text-refused">Refused</b> <code className="font-mono">OUT_OF_ORDER_SEQUENCE_NUMBER (45)</code>; a batch at an epoch below the producer's is <b className="font-semibold text-refused">Refused</b>{" "}
            <code className="font-mono">INVALID_PRODUCER_EPOCH (47)</code>.
          </p>
        </div>
      )}
    </div>
  );
}

export function ProducersPage() {
  const { topic } = useParams({ from: "/topics/$topic/producers" });
  const search = useSearch({ from: "/topics/$topic/producers" });
  const navigate = useNavigate({ from: "/topics/$topic/producers" });
  const partition = search.partition ?? 0;
  const q = useProducers(topic, partition);
  const [opened, setOpened] = useState<Set<number> | null>(null);
  const [was, setWas] = useState(partition);
  if (was !== partition) {
    setWas(partition);
    setOpened(null);
  }
  if (q.error && !q.data) return <PageError error={q.error} onRetry={() => void q.refetch()} />;
  if (!q.data) return <Skeleton className="h-96 w-full rounded-xl" />;
  const { view: v, held } = q.data;
  // A producer in a transaction opens by default; a click decides after that.
  const isOpen = (p: ProdRow) => (opened ? opened.has(p.id) : p.txn !== null);
  const toggle = (p: ProdRow) => {
    const now = new Set(v.producers.filter(isOpen).map((x) => x.id));
    if (now.has(p.id)) now.delete(p.id);
    else now.add(p.id);
    setOpened(now);
  };
  const stats: Stat[] = [
    { label: "Producers", value: number(v.producers.length), ch: 5 },
    { label: "Open transactions", value: number(v.open.length), ch: 4 },
    { label: "Last stable", value: number(v.last_stable), ch: 9 },
    { label: "High watermark", value: number(v.end), ch: 9 },
    { label: "Held back", value: number(held), unit: "records", ch: 7 },
  ];
  const x = v.open[0];
  const lastAbort = v.aborted.reduce<number | null>((a, b) => (b.at_ms !== null && (a === null || b.at_ms > a) ? b.at_ms : a), null);
  return (
    <div data-testid="producers">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <Badge variant="outline" className="font-mono text-[11.5px]">
          partition {v.partition} of {v.partitions}
        </Badge>
        {v.partitions > 1 && (
          <ToggleGroup type="single" variant="outline" size="sm" value={String(v.partition)} onValueChange={(p) => p && void navigate({ search: (s) => ({ ...s, partition: p === "0" ? undefined : Number(p) }), replace: true, resetScroll: false })} aria-label="Partition" className="bg-card" data-testid="producers-partition">
            {Array.from({ length: Math.min(v.partitions, 64) }, (_, p) => (
              <ToggleGroupItem key={p} value={String(p)} className="px-2.5 font-mono text-[12.5px]">
                {v.topic}-{p}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        )}
      </div>
      <StatStrip stats={stats} />
      <section className="mt-8">
        <h2 className="text-[17px] font-semibold text-bright">
          Producers on {v.topic}-{v.partition}
        </h2>
        <p className="mt-1 max-w-[52rem] text-[13.5px] text-muted-foreground">What DescribeProducers says: the id, its epoch, the last sequence the partition accepted, and when it last wrote. Open a row to see the last batches the partition remembers for that producer.</p>
        <div className="mt-4 overflow-x-auto rounded-xl border bg-card" role="table" aria-label="Producers">
          <div className="min-w-[56rem]">
            <div className={cn(GRID, "h-11 border-b bg-well px-4 text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase")} role="row">
              <span role="columnheader">Producer id</span>
              <span role="columnheader">Epoch</span>
              <span role="columnheader">Last sequence</span>
              <span role="columnheader">Idle</span>
              <span role="columnheader">Transaction</span>
              <span role="columnheader">Seen as</span>
              <span />
            </div>
            {v.producers.map((p) => (
              <Producer key={p.id} v={v} p={p} open={isOpen(p)} toggle={() => toggle(p)} />
            ))}
            {v.producers.length === 0 && <p className="flex min-h-24 items-center justify-center text-[13.5px] text-muted-foreground">No producer has written to this partition lately.</p>}
          </div>
        </div>
        <p className="mt-3 text-[12.5px] text-muted-foreground">The broker keeps producer ids, not client ids. A producer id silent for 1 day is forgotten on the partition.</p>
      </section>
      {held > 0 && (
        <section className="mt-10" data-testid="holds-back">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <h2 className="text-[17px] font-semibold text-bright">What the open transaction holds back</h2>
            {x && (
              <Button variant="outline" size="sm" asChild>
                <Link to="/topics/$topic/transactions/$txn" params={{ topic: v.topic, txn: x.id }}>
                  Open the transaction
                </Link>
              </Button>
            )}
          </div>
          <p className="mt-1 mb-4 text-[13.5px] text-muted-foreground">
            The tail of {v.topic}-{v.partition}. Records from {number(v.last_stable)} belong to {x ? <code className="font-mono text-foreground">{x.id}</code> : "an open transaction"}, still undecided.
          </p>
          <TailRuler t={v.tail} lastStable={v.last_stable} end={v.end} />
        </section>
      )}
      <section className="mt-10" data-testid="aborted">
        <h2 className="text-[17px] font-semibold text-bright">Aborted transactions remembered: {v.aborted.length}</h2>
        <p className="mt-1 max-w-[52rem] text-[13.5px] text-muted-foreground">
          {lastAbort !== null && `The last was aborted at ${dateTime(lastAbort)}. `}An abort is remembered until the log start passes it; a commit is not. read_committed consumers are told these ranges and skip them; read_uncommitted consumers still see the records.
        </p>
        {v.aborted.length > 0 ? (
          <div className="mt-4 overflow-x-auto rounded-xl border bg-card">
            <table className="w-full min-w-[44rem] table-fixed text-[13px]">
              <thead className="bg-well text-left text-[12px] tracking-[0.04em] text-muted-foreground uppercase">
                <tr>
                  <th className="h-10 px-4 font-medium">Aborted</th>
                  <th className="px-4 font-medium">Producer</th>
                  <th className="px-4 font-medium">Partition</th>
                  <th className="px-4 font-medium">Records</th>
                  <th className="w-[34%] px-4 font-medium">How</th>
                </tr>
              </thead>
              <tbody>
                {v.aborted.map((a) => (
                  <tr key={`${a.producer}-${a.partition}-${a.first}`} className="border-t align-top">
                    <td className="px-4 py-2.5 font-mono">{a.at_ms !== null ? dateTime(a.at_ms) : "not known"}</td>
                    <td className="px-4 py-2.5 font-mono">{a.producer}</td>
                    <td className="px-4 py-2.5 font-mono">
                      {v.topic}-{a.partition}
                    </td>
                    <td className="px-4 py-2.5 font-mono">
                      {number(a.first)} to {number(a.marker - 1)}
                      <span className="block text-[12px] text-muted-foreground">marker at {number(a.marker)}</span>
                    </td>
                    <td className="px-4 py-2.5">
                      <StatusDot tone="idle">Abort</StatusDot>
                      <span className="block text-[12.5px] text-muted-foreground">an abort marker ended it; the log does not say whether the producer or the timeout wrote it</span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="mt-3 text-[13px] text-muted-foreground">No aborted transaction is remembered on this partition.</p>
        )}
      </section>
    </div>
  );
}

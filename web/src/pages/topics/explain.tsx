// What explains a read under the Messages table (classic pages/topics/messages.rs `explainers` and committed.rs): one
// section, open while the page reads read_committed; the call the table made; and, for read_committed, where each reader
// stops and who is held back.
import { Link } from "@tanstack/react-router";
import { ChevronDownIcon } from "lucide-react";
import { useState } from "react";
import type { MessagesView } from "@/api/types/MessagesView";
import type { PartitionRow } from "@/api/types/PartitionRow";
import { StatusDot } from "@/components/app/status-dot";
import { Button } from "@/components/ui/button";
import { number } from "@/lib/format";
import { timeoutText } from "@/lib/kafka";
import { cn } from "@/lib/utils";

/** The bytes the table's Fetch asks for (`backend::requests::TABLE_BYTES`, 256 KiB). */
const TABLE_BYTES = 262_144;

function Card({ title, lede, children }: { title: string; lede?: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0 rounded-xl border bg-card p-5">
      <h3 className="text-[15px] font-semibold text-bright">{title}</h3>
      {lede && <p className="mt-1 text-[13px] text-muted-foreground">{lede}</p>}
      <div className="mt-3 text-[13.5px]">{children}</div>
    </div>
  );
}

function Call({ v }: { v: MessagesView }) {
  const q = v.query;
  const iso = q.reading === "committed" ? "1 (read_committed)" : "0 (read_uncommitted)";
  return (
    <Card title="The call">
      <pre className="overflow-x-auto rounded-lg bg-well p-3 font-mono text-[12.5px] leading-5 text-bright">
        {`Fetch v${v.head.calls.fetch}
  isolation_level: ${iso}
  topics[0]
    topic: ${v.head.name}
    partitions[0]
      partition: ${q.partitions[0] ?? 0}
      fetch_offset: ${q.offset ?? 0}
      partition_max_bytes: ${TABLE_BYTES}`}
      </pre>
      <p className="mt-3 text-[13px] text-muted-foreground">
        A key search reads the partition backwards from its end, a thousand offsets at a time, until enough records match or 8 MiB have been looked at; a time goes to the first record at or after it; an offset goes straight to the batch that holds it.
      </p>
    </Card>
  );
}

function Levels({ p, topic }: { p: PartitionRow; topic: string }) {
  const held = Math.max(0, p.end - p.last_stable);
  const row = (dt: string, dd: React.ReactNode, sub: string) => (
    <div className="grid grid-cols-[9rem_minmax(0,1fr)] gap-3 border-b py-2 last:border-b-0">
      <dt className="font-mono text-[12.5px] text-muted-foreground">{dt}</dt>
      <dd>
        {dd}
        <span className="block text-[12.5px] text-muted-foreground">{sub}</span>
      </dd>
    </div>
  );
  return (
    <Card title={`What each isolation level gets on ${topic}-${p.partition}`} lede="The same partition, the same Fetch, one field apart.">
      <dl>
        {row("read_uncommitted", <>offsets up to <span className="font-mono">{number(p.end)}</span></>, "every record, including those of the open transaction")}
        {row("read_committed", <>offsets up to <span className="font-mono">{number(p.last_stable)}</span></>, `stops before the first open record, ${held} behind`)}
        {row("After a commit", <>the LSO moves to {number(p.end)} at once</>, "the commit marker ends the hold")}
        {row("After an abort", <>the {held} records are skipped</>, "the aborted range is listed in the Fetch answer")}
      </dl>
    </Card>
  );
}

function Holds({ v }: { v: MessagesView }) {
  const t = v.head.txns[0];
  return (
    <div className="mt-6">
      <h3 className="text-[15px] font-semibold text-bright">Who this holds back</h3>
      <p className="mt-1 text-[13px] text-muted-foreground">Per partition, where a read_committed consumer stops. A group reading read_committed has its lag counted up to the last stable offset.</p>
      <div className="mt-3 overflow-x-auto rounded-xl border bg-card">
        <table className="w-full table-fixed text-[13px]" data-testid="holds">
          <thead className="bg-well text-[12px] tracking-[0.04em] text-muted-foreground uppercase">
            <tr>
              <th className="h-10 px-3 text-left font-medium">Partition</th>
              <th className="px-3 text-right font-medium">High watermark</th>
              <th className="px-3 text-right font-medium">Last stable offset</th>
              <th className="px-3 text-right font-medium">Held back</th>
              <th className="px-3 text-left font-medium">Why</th>
            </tr>
          </thead>
          <tbody>
            {v.holds.map((h) => {
              const held = Math.max(0, h.high_watermark - h.last_stable);
              return (
                <tr key={h.partition} className="border-t">
                  <td className="h-10 px-3 font-mono">
                    {v.head.name}-{h.partition}
                  </td>
                  <td className="px-3 text-right font-mono tabular-nums">{number(h.high_watermark)}</td>
                  <td className="px-3 text-right font-mono tabular-nums">{number(h.last_stable)}</td>
                  <td className="px-3 text-right">{held > 0 ? <StatusDot tone="warn">{held}</StatusDot> : <StatusDot tone="ok">0</StatusDot>}</td>
                  <td className="truncate px-3">{h.txn ? `${h.txn[0]} began at offset ${number(h.txn[1])}` : <span className="text-muted-foreground">no open transaction</span>}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <p className="mt-3 text-[13px] text-muted-foreground">
        {t ? `The transaction times out at ${timeoutText(t.timeout_ms)}: the coordinator then aborts it and fences producer ${t.producer_id}. ` : "A transaction that times out is aborted by the coordinator and its producer is fenced. "}
        {t ? (
          <Link to="/topics/$topic/transactions/$txn/held" params={{ topic: v.head.name, txn: t.id }} className="font-medium text-primary hover:underline">
            What it holds back
          </Link>
        ) : (
          <Link to="/topics/$topic/producers" params={{ topic: v.head.name }} className="font-medium text-primary hover:underline">
            What it holds back
          </Link>
        )}
      </p>
    </div>
  );
}

/** "How this read works": closed unless the page reads read_committed; opening it moves only what is below it. */
export function Explainers({ v }: { v: MessagesView }) {
  const committed = v.query.reading === "committed";
  const [open, setOpen] = useState(committed);
  const [was, setWas] = useState(committed);
  if (was !== committed) {
    setWas(committed);
    setOpen(committed);
  }
  const focus = [...v.head.parts].sort((a, b) => b.end - b.last_stable - (a.end - a.last_stable) || a.partition - b.partition)[0];
  return (
    <section className="mt-8" data-testid="explainers">
      <Button variant="ghost" className="-ml-3 text-[15px] font-semibold text-bright" aria-expanded={open} aria-controls="topic-explained" onClick={() => setOpen((o) => !o)}>
        <ChevronDownIcon className={cn("transition-transform duration-200", !open && "-rotate-90")} />
        How this read works
      </Button>
      {open && (
        <div id="topic-explained" className="mt-3">
          {committed && focus && (
            <div className="mb-6 grid gap-4 lg:grid-cols-2">
              <Levels p={focus} topic={v.head.name} />
              <Card title="Why filter by offset" lede="Ask for exactly the records you want to look at.">
                <p>
                  A Fetch names a partition and an offset and gets whole batches from the one that holds it, up to the byte limit. Asking from the last stable offset returns the first record of the open transaction onward.
                </p>
              </Card>
            </div>
          )}
          {committed && v.head.parts.some((p) => p.end > p.last_stable) && <Holds v={v} />}
          <div className={cn("grid gap-4 lg:grid-cols-2", committed && "mt-6")}>
            <Call v={v} />
            {!committed && (
              <Card title="Why filter by offset" lede="Ask for exactly the records you want to look at.">
                <p>
                  A Fetch names a partition and an offset and gets whole batches from the one that holds it, up to the byte limit. Asking from the last stable offset returns the first record of the open transaction onward. A key search has no such shortcut: it reads the partition in windows from its end.
                </p>
              </Card>
            )}
          </div>
        </div>
      )}
    </section>
  );
}

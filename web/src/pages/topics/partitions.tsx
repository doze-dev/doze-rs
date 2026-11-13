// A topic's partitions (board topic-partitions; classic pages/topics/partitions.rs): log start, log end, high watermark,
// last stable offset and size, with Delete records per row (R1b's dialog, `?dialog=delete-records&partition=`).
import { useNavigate, useParams } from "@tanstack/react-router";
import { InfoIcon } from "lucide-react";
import { useMemo } from "react";
import { useTopicFrame } from "@/api/topics";
import type { PartitionRow } from "@/api/types/PartitionRow";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { StatusDot } from "@/components/app/status-dot";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { number } from "@/lib/format";
import { bytes } from "@/lib/kafka";

export function PartitionsPage() {
  const { topic } = useParams({ from: "/topics/$topic/partitions" });
  const navigate = useNavigate({ from: "/topics/$topic/partitions" });
  const f = useTopicFrame(topic).data;
  const cols = useMemo<DataColumn<PartitionRow>[]>(
    () => [
      {
        id: "partition",
        accessorKey: "partition",
        header: "Partition",
        meta: { mono: true },
        cell: ({ row: { original: p } }) => (
          <span className="inline-flex items-center gap-2">
            {topic}-{p.partition}
            {p.offline !== null && <StatusDot tone="bad">Offline</StatusDot>}
          </span>
        ),
      },
      { accessorKey: "start", header: "Log start", size: 130, meta: { numeric: true, mono: true }, cell: (c) => number(c.getValue<number>()) },
      { accessorKey: "end", header: "Log end", size: 130, meta: { numeric: true, mono: true }, cell: (c) => number(c.getValue<number>()) },
      { id: "hw", accessorKey: "end", header: "High watermark", size: 150, meta: { numeric: true, mono: true }, cell: (c) => number(c.getValue<number>()) },
      {
        accessorKey: "last_stable",
        header: "Last stable offset",
        size: 230,
        meta: { numeric: true, mono: true },
        cell: ({ row: { original: p } }) => {
          const held = Math.max(0, p.end - p.last_stable);
          return (
            <span className="inline-flex items-center justify-end gap-2.5">
              {number(p.last_stable)}
              {held > 0 ? <StatusDot tone="warn">held {held}</StatusDot> : <StatusDot tone="ok">clear</StatusDot>}
            </span>
          );
        },
      },
      { id: "size", accessorFn: (p) => p.bytes ?? -1, header: "Size", size: 110, meta: { numeric: true, mono: true }, cell: ({ row: { original: p } }) => (p.bytes === null ? "unknown" : bytes(p.bytes)) },
      {
        id: "act",
        header: "",
        size: 150,
        enableSorting: false,
        meta: { numeric: true },
        cell: ({ row: { original: p } }) => (
          <Button variant="outline" size="sm" className="bg-card" onClick={() => void navigate({ search: (s) => ({ ...s, dialog: "delete-records", partition: p.partition }) })} data-testid={`delete-records-${p.partition}`}>
            Delete records
          </Button>
        ),
      },
    ],
    [topic, navigate],
  );
  if (!f) return <Skeleton className="h-72 w-full rounded-xl" />;
  const h = f.head;
  return (
    <div className="flex flex-col gap-6" data-testid="partitions">
      <div className="flex gap-3 rounded-xl border bg-card p-4 text-[13.5px]">
        <InfoIcon className="mt-0.5 size-4.5 shrink-0 text-cool" aria-hidden />
        <p>
          <b className="font-semibold text-bright">One broker.</b> This broker leads every partition and holds the only copy, so there are no leaders, replicas or in-sync lists to show. Said once, here.
        </p>
      </div>
      <div>
        <DataTable data={h.parts} columns={cols} rowId={(p) => String(p.partition)} aria-label="Partitions" maxHeight={720} rowHeight={48} />
        <p className="mt-4 max-w-[60rem] text-[13px] text-muted-foreground">
          Log start moves only when records are deleted by offset (DeleteRecords) or by retention. The log end is the next offset to be written; here the high watermark always equals it, because nothing is replicated.
        </p>
      </div>
      <section>
        <h2 className="text-[17px] font-semibold text-bright">Where the numbers come from</h2>
        <pre className="mt-3 overflow-x-auto rounded-xl border bg-well p-4 font-mono text-[12.5px] leading-5 text-bright" data-testid="list-offsets-call">
          {`ListOffsets v${h.calls.list_offsets}  topic ${h.name}, partitions ${h.parts.map((p) => p.partition).join(" ")}
  timestamp: -2  # earliest: the log start
  timestamp: -1  # latest: the log end
isolation_level 1  # read_committed: latest is the last stable offset`}
        </pre>
      </section>
    </div>
  );
}

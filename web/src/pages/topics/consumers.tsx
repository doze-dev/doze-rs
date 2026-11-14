// A topic's Consumers tab (classic pages/topics/consumers.rs): the groups that have committed offsets on the topic, each with
// where it stands on every partition of this topic and how far behind that is. It stays inside the topic.
import { Link, useParams } from "@tanstack/react-router";
import { UsersIcon } from "lucide-react";
import { useConsumers } from "@/api/topics";
import type { Reader } from "@/api/types/Reader";
import type { ReaderPartition } from "@/api/types/ReaderPartition";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { EmptyState } from "@/components/app/empty-state";
import { StatusDot, type Tone } from "@/components/app/status-dot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { number } from "@/lib/format";
import { dateTime, stamp } from "@/lib/kafka";
import { PageError } from "../page-error";

const phaseTone = (p: string): Tone => (p === "Stable" ? "ok" : p === "Empty" ? "idle" : "warn");

/** The lag cell: a bar with the figure, or the word for a partition with no figure (as the Groups pages say it). */
export function LagCell({ r }: { r: ReaderPartition }) {
  if (r.lag === "past_end") return <StatusDot tone="warn">{number(r.by)} past the end</StatusDot>;
  if (r.lag === "no_commit") return <StatusDot tone="idle">No commits yet</StatusDot>;
  if (r.lag === "gone") return <StatusDot tone="bad">Topic gone</StatusDot>;
  const n = r.lag === "behind" ? r.by : 0;
  return (
    <span className="flex items-center gap-3">
      <span className="h-1.5 w-28 overflow-hidden rounded-full bg-track" aria-hidden>
        <span className="block h-full rounded-full bg-primary" style={{ width: `${r.bar}%` }} />
      </span>
      <span className="font-mono tabular-nums">{number(n)}</span>
    </span>
  );
}

const cols = (topic: string): DataColumn<ReaderPartition>[] => [
  { accessorKey: "partition", header: "Partition", meta: { mono: true }, cell: (c) => `${topic}-${c.getValue<number>()}` },
  { id: "end", accessorFn: (r) => r.end ?? -1, header: "Log end", size: 140, meta: { numeric: true, mono: true }, cell: ({ row: { original: r } }) => (r.end === null ? "—" : number(r.end)) },
  { id: "committed", accessorFn: (r) => r.committed ?? -1, header: "Committed", size: 140, meta: { numeric: true, mono: true }, cell: ({ row: { original: r } }) => (r.committed === null ? <span className="text-muted-foreground">none</span> : number(r.committed)) },
  { id: "lag", accessorFn: (r) => r.by, header: "Lag", size: 260, cell: ({ row: { original: r } }) => <LagCell r={r} /> },
];

function Group({ g, topic, now }: { g: Reader; topic: string; now: number }) {
  return (
    <section className="rounded-xl border bg-card" data-testid="reader" data-group={g.id}>
      <header className="flex flex-wrap items-center justify-between gap-3 border-b px-4 py-3">
        <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
          <Link to="/groups/$group" params={{ group: g.id }} className="truncate font-mono text-[14px] font-medium text-link hover:underline">
            {g.id}
          </Link>
          <StatusDot tone={phaseTone(g.phase)}>{g.phase}</StatusDot>
          <Badge variant="outline" className="text-[11.5px]">
            {g.consumer_protocol ? "consumer protocol" : "classic"}
          </Badge>
          <span className="text-[13px] text-muted-foreground">
            {g.members === 1 ? "1 member" : `${number(g.members)} members`}
            {g.members > 0 && `, ${g.holding} hold${g.holding === 1 ? "s" : ""} ${topic}`}
          </span>
        </div>
        <div className="flex items-center gap-4 text-[13px] text-muted-foreground">
          {g.last_commit_ms !== null && <span title={dateTime(g.last_commit_ms)}>last commit {stamp(g.last_commit_ms, now)}</span>}
          {/* A figure with its label, in a slot of reserved width: a lag that grows moves nothing beside it. */}
          <span className="inline-flex items-baseline gap-1.5">
            <span className="text-[11.5px] font-medium tracking-[0.06em] uppercase">Behind</span>
            <span className="inline-block min-w-[6ch] text-right font-mono text-[14px] font-medium text-bright tabular-nums" data-testid="behind">
              {number(g.behind)}
            </span>
          </span>
        </div>
      </header>
      <div className="p-3">
        <DataTable data={g.rows} columns={cols(topic)} rowId={(r) => String(r.partition)} aria-label={`${g.id} on ${topic}`} className="border-0" />
      </div>
    </section>
  );
}

export function ConsumersPage() {
  const { topic } = useParams({ from: "/topics/$topic/consumers" });
  const q = useConsumers(topic);
  if (q.error && !q.data) return <PageError error={q.error} onRetry={() => void q.refetch()} />;
  if (!q.data) return <Skeleton className="h-72 w-full rounded-xl" />;
  const v = q.data;
  if (v.readers.length === 0) {
    return (
      <EmptyState
        icon={UsersIcon}
        title={`No consumer group has committed offsets for ${topic}`}
        action={
          <Button variant="outline" asChild>
            <Link to="/topics/$topic/messages" params={{ topic }}>
              Read the messages
            </Link>
          </Button>
        }
      >
        A group shows up here the first time it commits an offset on this topic. A consumer that reads without committing is not listed.
      </EmptyState>
    );
  }
  return (
    <div className="flex flex-col gap-4" data-testid="consumers">
      <p className="text-[13.5px] text-muted-foreground">
        The consumer groups that have committed offsets on <code className="font-mono text-foreground">{topic}</code>, each with where it stands on every partition.
        {v.readers.length < v.total && ` The ${v.readers.length} furthest behind are listed.`}
      </p>
      {v.readers.map((g) => (
        <Group key={g.id} g={g} topic={topic} now={v.now_ms} />
      ))}
    </div>
  );
}

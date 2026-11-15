// The Lag tab of a share group (KIP-932): a share group commits no offset, so its share-partitions take the table's place.
// Each row is one partition as the group reads it: where its share-partition starts (SPSO) and ends (SPEO), where the log
// ends, the records not yet done with, the records in flight by state, how often they have been delivered and who holds them.
import type { GroupLagView } from "@/api/types/GroupLagView";
import type { GroupShareView } from "@/api/types/GroupShareView";
import type { ShareRow } from "@/api/types/ShareRow";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { StatusDot } from "@/components/app/status-dot";
import { number } from "@/lib/format";
import { Callout, LagBar, Panel } from "./parts";
import { span } from "./words";

/** The four states of an in-flight record, in the order a record goes through them, with the tone each is drawn in. */
const STATES = [
  { key: "available", word: "available", tone: "bg-link" },
  { key: "acquired", word: "held", tone: "bg-warn" },
  { key: "acknowledged", word: "acknowledged", tone: "bg-good" },
  { key: "archived", word: "archived", tone: "bg-muted-foreground" },
] as const;

function inFlight(r: ShareRow): number {
  return r.available + r.acquired + r.acknowledged + r.archived;
}

/** The records in flight as one bar of four parts, and the counts under it. */
function FlightCell({ r }: { r: ShareRow }) {
  const all = inFlight(r);
  if (all === 0) return <span className="text-muted-foreground">nothing in flight</span>;
  return (
    <div className="flex min-w-0 flex-col gap-1" data-testid="share-flight">
      <div className="flex h-1.5 w-full overflow-hidden rounded-full bg-track" aria-hidden>
        {STATES.map((s) => (r[s.key] > 0 ? <div key={s.key} className={s.tone} style={{ width: `${(r[s.key] * 100) / all}%` }} /> : null))}
      </div>
      <span className="truncate text-[12px] text-muted-foreground tabular-nums">
        {STATES.filter((s) => r[s.key] > 0)
          .map((s) => `${number(r[s.key])} ${s.word}`)
          .join(" · ")}
      </span>
    </div>
  );
}

function deliveries(r: ShareRow, limit: number): string {
  if (r.deliveries.length === 0) return "—";
  return r.deliveries.map(([c, n]) => `${number(n)} × ${c}${c >= limit ? " (last)" : ""}`).join(", ");
}

function holders(r: ShareRow): string {
  if (r.holders.length === 0) return r.live ? "nobody" : "nobody (read again at the next fetch)";
  if (r.holders.length === 1) return `${r.holders[0][0]} (${number(r.holders[0][1])})`;
  return `${r.holders.length} members`;
}

function columns(v: GroupShareView): DataColumn<ShareRow>[] {
  return [
    { id: "partition", header: "Partition", size: 170, accessorFn: (r) => `${r.topic}-${r.partition}`, meta: { mono: true } },
    { id: "start", header: "Start (SPSO)", size: 120, accessorFn: (r) => (r.start === null ? "not yet" : number(r.start)), meta: { numeric: true, mono: true } },
    { id: "end", header: "End (SPEO)", size: 110, accessorFn: (r) => (r.end === null ? "—" : number(r.end)), meta: { numeric: true, mono: true } },
    { id: "log", header: "Log end", size: 100, accessorFn: (r) => (r.log_end === null ? "—" : number(r.log_end)), meta: { numeric: true, mono: true } },
    {
      id: "lag",
      header: "Not done with",
      size: 170,
      accessorFn: (r) => r.lag ?? -1,
      cell: ({ row: { original: r } }) =>
        r.lag === null ? <StatusDot tone="idle">starts at its first fetch</StatusDot> : <LagBar pct={r.log_end ? (r.lag * 100) / Math.max(1, r.log_end) : 0} n={r.lag} />,
    },
    { id: "flight", header: "In flight", accessorFn: inFlight, cell: ({ row: { original: r } }) => <FlightCell r={r} /> },
    { id: "deliveries", header: "Deliveries", size: 170, accessorFn: (r) => deliveries(r, v.delivery_limit), meta: { mono: true } },
    { id: "held", header: "Held by", size: 190, accessorFn: holders, meta: { mono: true } },
  ];
}

export function ShareLag({ v }: { v: GroupLagView }) {
  const s = v.share;
  if (!s) return null;
  const locks = s.rows.map((r) => r.next_lock_ms).filter((ms): ms is number => ms !== null);
  const soonest = locks.length > 0 ? Math.min(...locks) : null;
  const archived = s.rows.reduce((n, r) => n + r.archived, 0);
  return (
    <div className="flex flex-col gap-6" data-testid="share-partitions">
      {s.rows.length === 0 ? (
        <p className="rounded-xl border border-dashed bg-card/60 p-8 text-center text-[13.5px] text-muted-foreground">
          {v.head.id} reads no partition yet: a member that joins with a topic makes its share-partitions.
        </p>
      ) : (
        <DataTable data={s.rows} columns={columns(s)} rowId={(r) => `${r.topic}-${r.partition}`} aria-label={`Share-partitions of ${v.head.id}`} maxHeight={520} />
      )}
      <div className="grid gap-4 lg:grid-cols-2">
        <Callout data-testid="share-explain">
          <b className="font-semibold text-bright">A share group commits no offset.</b> Its members read the same partitions together, and each record goes to one of them at a time: acquired under a lock of {span(s.lock_ms)}, then accepted,
          released (to be delivered again) or rejected. The start moves past every record that is done with; the records between the start and the end are in flight.
          {soonest !== null && ` The soonest lock runs out in ${span(Math.max(0, soonest))}.`}
        </Callout>
        <Panel title="What the group is held to">
          <ul className="flex flex-col gap-1.5 text-[13.5px] text-muted-foreground" data-testid="share-limits">
            <li>
              A record released, or whose lock runs out, after <b className="text-bright">{s.delivery_limit}</b> deliveries is archived
              {archived > 0 ? ` (${number(archived)} archived now).` : "."}
            </li>
            <li>
              At most <b className="text-bright">{number(s.record_locks)}</b> records in flight per partition.
            </li>
            <li>
              A partition starts at <code className="font-mono">{s.reset}</code> when first read; records of aborted transactions are {s.read_committed ? "never delivered (read_committed)" : "delivered (read_uncommitted)"}.
            </li>
            <li>
              Change these in the group's configs: <code className="font-mono">kafka-configs --alter --entity-type groups --entity-name {v.head.id}</code>; move the starts with{" "}
              <code className="font-mono">kafka-share-groups --reset-offsets --group {v.head.id}</code> while the group is empty.
            </li>
          </ul>
        </Panel>
      </div>
    </div>
  );
}

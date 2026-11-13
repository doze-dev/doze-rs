// The Lag tab (boards group-lag, lag-no-commits, lag-ahead, lag-expiring): one table per topic the group committed on, each
// partition with its log end, its commit and its lag as a bar; the data chooses the state, except "when do these expire",
// which is asked for (`?expiring=1`, a segmented control in place of the classic page's button).
import { getRouteApi, Link } from "@tanstack/react-router";
import { useMemo } from "react";
import { useGroup } from "@/api/groups";
import type { GroupLagView } from "@/api/types/GroupLagView";
import type { LagRow } from "@/api/types/LagRow";
import type { LagTopic } from "@/api/types/LagTopic";
import { DataTable, type DataColumn } from "@/components/app/data-table";
import { StatusDot } from "@/components/app/status-dot";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { number } from "@/lib/format";
import { Callout, LagBar, Panel } from "./parts";
import { dateTime, span, stamp } from "./words";

const route = getRouteApi("/groups/$group/lag");

function LagCell({ r }: { r: LagRow }) {
  switch (r.lag.kind) {
    case "behind":
      return <LagBar pct={r.bar} n={r.lag.n} />;
    case "caught_up":
      return <LagBar pct={0} n={0} />;
    case "past_end":
      return <StatusDot tone="warn">{number(r.lag.n)} past the end</StatusDot>;
    case "no_commit":
      return <StatusDot tone="idle">No commits yet</StatusDot>;
    case "gone":
      return <StatusDot tone="bad">Topic gone</StatusDot>;
  }
}

function lagColumns(now: number): DataColumn<LagRow>[] {
  return [
    { id: "partition", header: "Partition", size: 190, accessorFn: (r) => `${r.topic}-${r.partition}`, meta: { mono: true } },
    { id: "end", header: "Log end", size: 110, accessorFn: (r) => (r.end === null ? "—" : number(r.end)), meta: { numeric: true, mono: true } },
    { id: "committed", header: "Committed", size: 116, accessorFn: (r) => (r.committed === null ? "none" : number(r.committed)), meta: { numeric: true, mono: true } },
    { id: "lag", header: "Lag", accessorFn: (r) => (r.lag.kind === "behind" ? r.lag.n : 0), cell: ({ row: { original: r } }) => <LagCell r={r} /> },
    { id: "last", header: "Last commit", size: 124, accessorFn: (r) => (r.last_commit_ms === null ? "never" : stamp(r.last_commit_ms, now)), meta: { mono: true } },
  ];
}

const EXPIRING: DataColumn<LagRow>[] = [
  { id: "partition", header: "Partition", accessorFn: (r) => `${r.topic}-${r.partition}`, meta: { mono: true } },
  { id: "end", header: "Log end", size: 120, accessorFn: (r) => (r.end === null ? "—" : number(r.end)), meta: { numeric: true, mono: true } },
  { id: "committed", header: "Committed", size: 120, accessorFn: (r) => number(r.committed ?? 0), meta: { numeric: true, mono: true } },
  { id: "forgotten", header: "Forgotten at", size: 240, accessorFn: (r) => (r.expires_ms === null ? "not while things stay as they are" : dateTime(r.expires_ms)), meta: { mono: true } },
  { id: "last", header: "Last commit", size: 140, accessorFn: (r) => (r.last_commit_ms === null ? "never" : dateTime(r.last_commit_ms)), meta: { mono: true } },
];

function topicBits(t: LagTopic, v: GroupLagView): string {
  const bits = [`${t.partitions} ${t.partitions === 1 ? "partition" : "partitions"}`];
  if (t.compacted) bits.push("compacted");
  if (t.retention_ms !== null && t.retention_ms >= 0) bits.push(`retention ${span(t.retention_ms)}`);
  if (t.holders.length === 0 && v.head.members.length === 0) bits.push("no member holds any");
  else if (t.holders.length === 1) bits.push(`held by ${t.holders[0]}`);
  else if (t.holders.length > 1) bits.push(`held by ${t.holders.length} members`);
  if (!t.exists) bits.push("the topic is gone");
  return bits.join(", ");
}

function behindWord(t: LagTopic): string {
  if (!t.rows.some((r) => r.committed !== null)) return "lag not known";
  return `${number(t.behind)} behind`;
}

function TopicBlock({ t, v, expiring }: { t: LagTopic; v: GroupLagView; expiring: boolean }) {
  const cols = useMemo(() => (expiring ? EXPIRING : lagColumns(v.head.now_ms)), [expiring, v.head.now_ms]);
  const rows = expiring ? t.rows.filter((r) => r.committed !== null) : t.rows;
  return (
    <section className="flex flex-col gap-2" data-testid="lag-topic" data-topic={t.name}>
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <div className="min-w-0 text-[13.5px]">
          <Link to="/topics/$topic" params={{ topic: t.name }} className="font-mono font-medium text-primary hover:underline">
            {t.name}
          </Link>
          <span className="ml-2 text-muted-foreground">{topicBits(t, v)}</span>
        </div>
        {!expiring && (
          <span className="font-mono text-[13px] text-bright tabular-nums" data-testid="topic-behind">
            {behindWord(t)}
          </span>
        )}
      </div>
      <DataTable data={rows} columns={cols} rowId={(r) => `${r.topic}-${r.partition}`} aria-label={`Lag of ${t.name}`} maxHeight={480} />
    </section>
  );
}

function RetentionWindow({ v }: { v: GroupLagView }) {
  const h = v.head;
  const first = Math.min(...v.topics.flatMap((t) => t.rows.map((r) => r.expires_ms ?? Infinity)));
  const hours = Math.floor(v.retention_ms / 3_600_000);
  const flag = <code className="font-mono">--offsets-retention {hours}h</code>;
  if (!Number.isFinite(first)) {
    return (
      <Panel title="Retention window" className="mb-6">
        <p className="text-[13.5px] text-muted-foreground">
          {span(v.retention_ms)} ({flag}). No date is set while things stay as they are: {h.members.length === 0 ? "the group may yet have members." : "a group with members keeps what it subscribes to."}
        </p>
      </Panel>
    );
  }
  const from = first - v.retention_ms - 1;
  const pct = Math.max(0, Math.min(100, ((h.now_ms - from) * 100) / Math.max(1, v.retention_ms)));
  return (
    <Panel title="Retention window" className="mb-6" data-testid="retention-window">
      <p className="mb-4 text-[13.5px] text-muted-foreground">
        {span(v.retention_ms)} ({flag}), counted from {h.empty_since_ms === from ? "when the group lost its last member" : "the commit"}, {dateTime(from)}.
      </p>
      <div className="flex items-center gap-3 font-mono text-[12.5px] text-muted-foreground">
        <span>{dateTime(from)}</span>
        <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-track" aria-hidden>
          <div className="h-full rounded-full bg-warn" style={{ width: `${pct}%` }} />
        </div>
        <span>{dateTime(first)}</span>
      </div>
      <div className="mt-2 flex justify-between text-[12.5px] text-muted-foreground tabular-nums">
        <span>{span(h.now_ms - from)} gone</span>
        <span className="text-bright">{span(first - h.now_ms)} left</span>
      </div>
    </Panel>
  );
}

function Explain({ v }: { v: GroupLagView }) {
  const h = v.head;
  const all = v.topics.flatMap((t) => t.rows);
  if (v.state === "no_commits") {
    const first = all.find((r) => r.lag.kind === "no_commit");
    const end = first?.end !== null && first?.end !== undefined ? number(first.end) : "its";
    return (
      <Callout className="mt-6" data-testid="lag-explain" data-state="no_commits">
        <b className="font-semibold text-bright">No commit is not lag {end}.</b> OffsetFetch answers offset -1 for a partition the group never committed, and the console shows that as "none". A consumer that finds -1 starts where its own{" "}
        <code className="font-mono">auto.offset.reset</code> says: <code className="font-mono">earliest</code> reads all {end} records, <code className="font-mono">latest</code> skips them. The broker cannot know which. The first commit from {h.id} turns this row into a number.
      </Callout>
    );
  }
  if (v.state === "ahead") {
    const r = all.find((x) => x.lag.kind === "past_end");
    const expires = Math.min(...all.map((x) => x.expires_ms ?? Infinity));
    const held = v.topics.find((t) => t.rows.some((x) => x.committed !== null));
    return (
      <div className="mt-6 flex flex-col gap-4" data-testid="lag-explain" data-state="ahead">
        {h.members.length === 0 && (
          <Callout>
            <b className="font-semibold text-bright">Nobody is in this group.</b> The last member left, or its session ran out. The group is Empty, so it can be reset or deleted: Kafka only accepts those while no member is there. It still holds committed offsets
            {held && ` on ${held.name}`}, which are not forgotten until the retention date{Number.isFinite(expires) ? `, in ${span(expires - h.now_ms)}` : ""}.
          </Callout>
        )}
        <div className="grid gap-4 lg:grid-cols-2">
          <Callout tone="warn">
            <b className="font-semibold text-bright">Kafka accepted this commit.</b> It stores offsets past the end without a check. The usual cause is a topic that was recreated while an app kept its old position, then committed it. Nothing is broken until a consumer starts.
          </Callout>
          <Callout>
            <b className="font-semibold text-bright">What a consumer would do.</b> It asks to fetch {number(r?.committed ?? 0)} and is told OFFSET_OUT_OF_RANGE. It then follows its <code className="font-mono">auto.offset.reset</code>: <code className="font-mono">latest</code> skips ahead to {number(r?.end ?? 0)},{" "}
            <code className="font-mono">earliest</code> re-reads {r ? `${r.topic}-${r.partition}` : ""} from its start. Resetting to latest here fixes it before that happens.
          </Callout>
        </div>
      </div>
    );
  }
  if (v.state === "expiring") {
    return (
      <div className="mt-6 grid gap-4 lg:grid-cols-2" data-testid="lag-explain" data-state="expiring">
        <Callout>
          <b className="font-semibold text-bright">What happens then.</b> At the first sweep after that time the offsets go. A group with no members and no offsets is forgotten, and the id is free: a consumer that joins as {h.id} afterwards is a new group and starts at its{" "}
          <code className="font-mono">auto.offset.reset</code>.
        </Callout>
        <Callout tone="warn">
          <b className="font-semibold text-bright">To keep them,</b> start a consumer in {h.id} before then: a group with members keeps what it subscribes to.
        </Callout>
      </div>
    );
  }
  const behind = all.find((r) => r.lag.kind === "behind");
  return (
    <p className="mt-4 text-[13px] text-muted-foreground" data-testid="lag-explain" data-state="normal">
      A full bar would be a consumer that has read none of the partition. {behind && `${behind.topic}-${behind.partition} is ${behind.bar} % of its log behind. `}The broker keeps no lag history: this is the lag now.
    </p>
  );
}

export function GroupLagPage() {
  const { group: id } = route.useParams();
  const search = route.useSearch();
  const navigate = route.useNavigate();
  const expiring = search.expiring === 1;
  const res = useGroup(id, expiring);
  const v = res.data;
  return (
    <div data-testid="group-lag">
      <div className="mb-5 flex flex-wrap items-center justify-between gap-3">
        <ToggleGroup
          type="single"
          variant="outline"
          value={expiring ? "expiring" : "lag"}
          onValueChange={(x) => x && void navigate({ search: (s) => ({ ...s, expiring: x === "expiring" ? 1 : undefined }), resetScroll: false })}
          aria-label="Show"
          data-testid="lag-view"
        >
          <ToggleGroupItem value="lag" className="px-3 data-[state=on]:bg-accent data-[state=on]:text-bright">
            Lag now
          </ToggleGroupItem>
          <ToggleGroupItem value="expiring" className="px-3 data-[state=on]:bg-accent data-[state=on]:text-bright">
            When do these expire?
          </ToggleGroupItem>
        </ToggleGroup>
        <span className="text-[13px] text-muted-foreground">Lag is the log end minus the committed offset: records the app has not finished with, not records it has not fetched.</span>
      </div>
      {!v ? (
        <Skeleton className="h-80 rounded-xl" />
      ) : (
        <>
          {expiring && <RetentionWindow v={v} />}
          {v.topics.length === 0 ? (
            <p className="rounded-xl border border-dashed bg-card/60 p-8 text-center text-[13.5px] text-muted-foreground">{id} has committed no offset and its members hold no partition yet.</p>
          ) : (
            <div className="flex flex-col gap-6">
              {v.topics
                .filter((t) => !expiring || t.rows.some((r) => r.committed !== null))
                .map((t) => (
                  <TopicBlock key={t.name} t={t} v={v} expiring={expiring} />
                ))}
            </div>
          )}
          <Explain v={v} />
        </>
      )}
    </div>
  );
}

// The tabs of a log group's page: its streams (logs__log-group-detail, log-streams-none, and the empty landing of log-group-created) and its
// metric filters (logs__metric-filters, metric-filters-none, metric-filter-delete); a group that has subscription filters also shows them.
import { Link } from "@tanstack/react-router";
import { FilterIcon, PlusIcon, ScrollTextIcon, SearchIcon, TrashIcon } from "lucide-react";
import { useState } from "react";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { PlainConfirm } from "../../../kit";
import { Value } from "../value";
import { useGroupWrite } from "./api";
import { type Group, type MetricFilter, type Stream, type SubscriptionFilter, arnWithStar, classWords, retentionWords } from "./model";

const teaching = "mt-5 max-w-[44rem] border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground";

/** The group's own identity: class, retention, filters, ARN, key. The Retention opens its setting. */
export function GroupCard({ g, retention }: { g: Group; retention: () => void }) {
  const row = (label: string, value: React.ReactNode) => (
    <div key={label} className="contents">
      <dt className="border-b py-3.5 pr-4 text-[13.5px] text-muted-foreground">{label}</dt>
      <dd className="min-w-0 border-b py-3.5 text-[13.5px] break-words text-bright" data-testid={`field-${label}`}>{value}</dd>
    </div>
  );
  const col = "grid grid-cols-[minmax(7rem,10rem)_minmax(0,1fr)] content-start [&>div:last-child>*]:border-b-0";
  return (
    <section className="mb-8 rounded-[14px] border bg-card p-6 lg:p-7" aria-label="Log group details" data-testid="details">
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Log group details</h2>
      <div className="mt-3 grid lg:grid-cols-2 lg:gap-x-12">
        <dl className={col}>
          {row("Log class", classWords(g.cls))}
          {row("Retention", <button type="button" onClick={retention} className="rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="retention-open">{retentionWords(g.retention)}</button>)}
          {row("Metric filters", g.filters)}
        </dl>
        <dl className={col}>
          {row("ARN", <span className="flex items-start gap-2 font-mono text-[13px]">{arnWithStar(g.arn)}<CopyButton text={arnWithStar(g.arn)} what="ARN copied" iconOnly variant="ghost" label="Copy ARN" /></span>)}
          {row("KMS key ID", g.kms ? <span className="font-mono text-[13px]">{g.kms}</span> : <span className="text-muted-foreground">None</span>)}
        </dl>
      </div>
    </section>
  );
}

export function StreamsTab({ g, streams, more, justMade, retention, group, back }: { g: Group; streams: Stream[]; more: boolean; justMade: boolean; retention: () => void; group: string; back: () => void }) {
  if (streams.length === 0) {
    return justMade ? (
      <EmptyTab icon={ScrollTextIcon} title="DescribeLogStreams returned no log streams" testId="streams-empty" action={<Button type="button" variant="outline" className="bg-card" onClick={back} data-testid="back-to-groups">Back to log groups</Button>}>
        A log stream is a sequence of log events that share the same source. Streams are created by whatever writes to the group, with CreateLogStream and PutLogEvents. Nothing has written to this group yet.
      </EmptyTab>
    ) : (
      <EmptyTab icon={ScrollTextIcon} title="No log streams" testId="streams-empty">
        Nothing has written to <span className="font-mono text-foreground">{group}</span> yet. A log stream appears when a producer first writes to it.
      </EmptyTab>
    );
  }
  const columns: DataColumn<Stream>[] = [
    { id: "name", header: "Log stream", accessorFn: (r) => r.name.toLowerCase(), meta: { mono: true }, cell: (c) => <Link to="/resources/$service/$kind/$name" params={{ service: "logs", kind: "streams", name: c.row.original.name }} search={{ group }} className="rounded-sm font-mono text-[13px] font-medium text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="stream-link">{c.row.original.name}</Link> },
    { id: "last", header: "Last event time", size: 260, accessorFn: (r) => r.last ?? 0, cell: (c) => <Value value={c.row.original.last === null ? null : String(c.row.original.last)} format="time" /> },
  ];
  return (
    <div data-testid="group-streams">
      <GroupCard g={g} retention={retention} />
      <DataTable aria-label="Log streams" data={streams} columns={columns} rowId={(r) => r.name} maxHeight={560} />
      <p className={teaching}>
        Each separate source of logs makes up a separate log stream. Choose a stream to read its events, or choose <b className="font-medium text-foreground">Search log group</b> to filter across all {streams.length}{more ? "+" : ""}. The columns are fields that DescribeLogStreams returns (logStreamName, lastEventTimestamp).{more ? " The newest 50 are shown." : ""} <span className="text-muted-foreground">On AWS the last event time can lag ingestion by up to an hour; doze-aws reports it as written.</span>
      </p>
    </div>
  );
}

export function FiltersTab({ group, filters, create, onDeleted }: { group: string; filters: MetricFilter[]; create: () => void; /** After a delete: the names of the filters that went (the page says so, in place of what it said before). */ onDeleted?: (names: string[]) => void }) {
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [asking, setAsking] = useState(false);
  const [prefix, setPrefix] = useState("");
  const del = useGroupWrite("DeleteMetricFilter");
  if (filters.length === 0) {
    return (
      <EmptyTab icon={FilterIcon} title="No metric filters" testId="filters-empty" action={<Button type="button" variant="outline" className="bg-card" onClick={create} data-testid="filter-create"><PlusIcon /> Create metric filter</Button>}>
        No metric filter turns events in <span className="font-mono text-foreground">{group}</span> into a metric.
      </EmptyTab>
    );
  }
  const columns: DataColumn<MetricFilter>[] = [
    { id: "name", header: "Filter name", size: 200, accessorFn: (r) => r.name, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.name}</span> },
    { id: "pattern", header: "Filter pattern", accessorFn: (r) => r.pattern, meta: { mono: true } },
    { id: "metric", header: "Metric name", size: 160, accessorFn: (r) => r.metric, meta: { mono: true } },
    { id: "ns", header: "Metric namespace", size: 170, accessorFn: (r) => r.namespace, meta: { mono: true } },
    { id: "value", header: "Metric value", size: 120, accessorFn: (r) => r.value, meta: { mono: true } },
    { id: "unit", header: "Unit", size: 100, accessorFn: (r) => r.unit || "None" },
  ];
  const picked = filters.filter((f) => chosen.has(f.name));
  const one = picked[0];
  const remove = () => {
    void (async () => {
      try {
        for (const f of picked) await del.mutateAsync({ group, input: { filterName: f.name } });
        setAsking(false);
        setChosen(new Set());
        onDeleted?.(picked.map((f) => f.name));
      } catch {
        /* said in the dialog */
      }
    })();
  };
  return (
    <div data-testid="group-filters">
      <SelectableTable<MetricFilter> aria-label="Metric filters" data={filters.filter((f) => f.name.startsWith(prefix.trim()))} empty={`No metric filter's name begins with “${prefix}”.`} columns={columns} rowId={(r) => r.name} selected={chosen} onSelectedChange={setChosen} noun="metric filter" hint=" " actionsAlways actionsEnd
        toolbar={<div className="relative w-[18rem] max-w-full"><SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" /><Input value={prefix} onChange={(e) => setPrefix(e.target.value)} placeholder="Filter name prefix" aria-label="Filter name prefix" className="bg-card pl-9" data-testid="filter-prefix" /></div>}
        actions={(sel) => <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={sel.length === 0} onClick={() => { del.reset(); setAsking(true); }} data-testid="filter-delete"><TrashIcon /> Delete</Button>} />
      <p className={teaching}>
        A metric filter turns the events that match its pattern into a CloudWatch metric. “The metric won't be visible until there are some data points for it.” One log group holds up to 100 metric filters. The columns are fields that DescribeMetricFilters returns. To add a filter, choose <b className="font-medium text-foreground">Create metric filter</b> in the log group's Actions.
      </p>
      <PlainConfirm open={asking} onOpenChange={setAsking} title="Delete metric filter" call="DeleteMetricFilter" pending={del.isPending} error={del.error} onConfirm={remove} testId="filter-delete-dialog"
        description={picked.length === 1 && one ? <>DeleteMetricFilter deletes <b className="font-mono font-semibold text-bright">{one.name}</b> from <b className="font-mono font-semibold text-bright">{group}</b>. Events no longer produce <b className="font-mono font-semibold text-bright">{one.metric}</b> in <b className="font-mono font-semibold text-bright">{one.namespace}</b>.</> : <>DeleteMetricFilter deletes {picked.length} metric filters from <b className="font-mono font-semibold text-bright">{group}</b>. Events no longer produce their metrics.</>} />
    </div>
  );
}

export function SubscriptionsTab({ group, subs, create }: { group: string; subs: SubscriptionFilter[]; create: () => void }) {
  const [asking, setAsking] = useState<SubscriptionFilter | null>(null);
  const del = useGroupWrite("DeleteSubscriptionFilter");
  const columns: DataColumn<SubscriptionFilter>[] = [
    { id: "name", header: "Filter name", size: 220, accessorFn: (r) => r.name, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.name}</span> },
    { id: "pattern", header: "Filter pattern", accessorFn: (r) => r.pattern, meta: { mono: true }, cell: (c) => c.row.original.pattern || <span className="text-muted-foreground">(every event)</span> },
    { id: "dest", header: "Destination", accessorFn: (r) => r.destination, meta: { mono: true } },
    { id: "del", header: "", size: 90, enableSorting: false, cell: (c) => <Button type="button" variant="ghost" size="sm" className="text-bad hover:text-bad" onClick={() => { del.reset(); setAsking(c.row.original); }} data-testid="sub-delete"><TrashIcon /> Delete</Button> },
  ];
  return (
    <div data-testid="group-subscriptions">
      <div className="mb-3 flex justify-end"><Button type="button" variant="outline" className="bg-card" onClick={create}><PlusIcon /> Subscription filter (Lambda)</Button></div>
      <DataTable aria-label="Subscription filters" data={subs} columns={columns} rowId={(r) => r.name} />
      <PlainConfirm open={asking !== null} onOpenChange={(o) => !o && setAsking(null)} title="Delete subscription filter" call="DeleteSubscriptionFilter" pending={del.isPending} error={del.error} testId="sub-delete-dialog"
        onConfirm={() => asking && del.mutate({ group, input: { filterName: asking.name } }, { onSuccess: () => setAsking(null) })}
        description={<>DeleteSubscriptionFilter stops sending <b className="font-mono font-semibold text-bright">{group}</b> events to {asking?.destination.split(":").pop()} by <b className="font-mono font-semibold text-bright">{asking?.name}</b>.</>} />
    </div>
  );
}

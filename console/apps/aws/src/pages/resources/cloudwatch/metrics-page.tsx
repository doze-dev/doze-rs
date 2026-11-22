// All metrics (cloudwatch__metrics-all, metrics-none, metrics-graph): browse the metrics that have data by namespace and dimension
// set, tick some to graph them, and add the graph to a dashboard. The graphed rows, the search and where in the browse view the page
// is are in the address, so a graph can be shared.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { LinkIcon, PlusIcon, SearchIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { Chart } from "@doze/ui/components/app/chart";
import { copyText } from "@doze/ui/components/app/copy";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { useAbout } from "../../../api/queries";
import { type MetricQuery, useMetricData, useServiceCrumbs, useUnit } from "../../../kit";
import { AddToDashboard } from "./add-to-dashboard";
import { useMetrics } from "./api";
import { type GM, decode, encode, sourceOf, summaryLine, toQueries } from "./graph-model";
import { GraphedTable } from "./metrics-graphed";
import { Browse } from "./metrics-browse";
import { selSearch } from "./wizard-model";
import { RangeBar, spanWords, useRange } from "./range";

type Params = { sel?: string; q?: string; name?: string; key?: string; mode?: string };

/** The metrics page's address, and a way to change part of it without leaving it. */
export function useParams2() {
  const search = useSearch({ strict: false }) as Params;
  const navigate = useNavigate();
  const set = (patch: Partial<Params>) => void navigate({ to: ".", search: ((prev: Record<string, unknown>) => { const next = { ...prev, ...patch } as Record<string, unknown>; for (const k of Object.keys(next)) if (next[k] === undefined || next[k] === "") delete next[k]; return next; }) as never, replace: true });
  return [search, set] as const;
}

export function MetricsPage({ service, kind }: { service: string; kind: string }) {
  const [p, set] = useParams2();
  const crumbs = useServiceCrumbs(service);
  const navigate = useNavigate();
  const about = useAbout().data;
  const all = useMetrics();
  const rows = useMemo(() => decode(p.sel), [p.sel]);
  const [range, patchRange, span] = useRange("3h");
  const [typed, setTyped] = useState(p.q ?? "");
  const [adding, setAdding] = useState(false);
  const queries: MetricQuery[] = useMemo(() => toQueries(rows), [rows]);
  const data = useMetricData(queries, span, queries.length > 0);
  const unit = useUnit(queries, span, queries.length > 0);
  const refresh = () => { void all.refetch(); patchRange({ now: Date.now() }); };
  const tab = p.mode === "graphed" || (rows.length > 0 && (p.mode === "options" || p.mode === "source")) ? p.mode : "browse";
  const alarmFrom = (r: GM) => void navigate({ to: "/resources/$service/$second", params: { service, second: "alarms" }, search: { do: "create", ...selSearch({ ns: r.ns ?? "", m: r.m ?? "", d: r.d ?? [], stat: r.stat, period: r.period }) } });
  const region = about?.region ?? "us-east-1";
  return (
    <>
      <PageHeader
        title="All metrics"
        crumbs={[...crumbs, { label: "All metrics", to: `/resources/${service}/${kind}` }]}
        sub={rows.length > 0 ? summaryLine(rows, spanWords(range)) : "Metrics are grouped by namespace, then by the set of dimensions they were published with."}
        refresh={<RefreshControl updatedAt={Math.max(all.dataUpdatedAt, data.dataUpdatedAt)} fetching={all.isFetching || data.isFetching} onRefresh={refresh} interval={false} />}
        actions={rows.length > 0 ? (
          <>
            <Button type="button" variant="outline" className="bg-card" onClick={() => void copyText(window.location.href, "Link to this graph copied")} data-testid="share"><LinkIcon /> Actions › Share</Button>
            <Button type="button" variant="outline" className="bg-card" onClick={() => setAdding(true)} data-testid="add-to-dashboard"><PlusIcon /> Actions › Add to dashboard</Button>
          </>
        ) : undefined}
      />
      <div className="mb-5 flex flex-wrap items-center gap-3">
        {rows.length === 0 && (
          <div className="relative w-full max-w-[35rem]">
            <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
            <form onSubmit={(e) => { e.preventDefault(); set({ q: typed.trim() || undefined }); }} role="search">
              <Input value={typed} onChange={(e) => { setTyped(e.target.value); set({ q: e.target.value.trim() || undefined }); }} placeholder="Search for a metric, dimension or resource ID" aria-label="Search for a metric, dimension or resource ID" className="bg-card pl-9" data-testid="metric-search" />
            </form>
          </div>
        )}
        <RangeBar state={range} onChange={patchRange} compact={rows.length === 0} />
      </div>
      <div className="mb-8 rounded-[14px] border bg-card p-6" data-testid="metrics-chart">
        <Chart series={data.series.map((s) => ({ ...s, area: rows.length === 1 }))} from={span.from} to={span.to} height={240} unit={unit} loading={queries.length > 0 && data.isPending} aria-label="The graphed metrics over the range" emptyAxes empty={queries.length === 0 ? "No metrics selected." : "No data points in this range. doze-aws keeps samples for 24 hours."} testId="metrics-graph" />
      </div>
      <Tabs value={tab} onValueChange={(v) => set({ mode: v })} className="gap-0">
        <TabsList variant="line" className="h-11 w-full justify-start gap-2 overflow-x-auto rounded-none border-b p-0" aria-label="Metrics views">
          <TabsTrigger value="browse" className="h-11 flex-none px-3 text-[14px]" data-testid="tab-browse">Browse</TabsTrigger>
          <TabsTrigger value="graphed" className="h-11 flex-none px-3 text-[14px]" data-testid="tab-graphed">Graphed metrics{rows.length > 0 && <span className="ml-1.5 rounded-full bg-muted px-1.5 text-[11.5px] text-muted-foreground tabular-nums">{rows.length}</span>}</TabsTrigger>
          {rows.length > 0 && <TabsTrigger value="options" className="h-11 flex-none px-3 text-[14px]" data-testid="tab-options">Options</TabsTrigger>}
          {rows.length > 0 && <TabsTrigger value="source" className="h-11 flex-none px-3 text-[14px]" data-testid="tab-source">Source</TabsTrigger>}
        </TabsList>
        <div className="pt-6" role="tabpanel" data-testid="tab-panel">
          {tab === "browse" && <Browse view={p} onView={(v) => set(v)} rows={rows} onRows={(next) => set({ sel: next.length ? encode(next) : undefined })} retry={refresh} />}
          {tab === "graphed" && (rows.length === 0 ? <EmptyState icon={SearchIcon} title="No metrics selected">Browse to a namespace and tick the metrics to graph them here.</EmptyState> : <GraphedTable rows={rows} onChange={(next) => set({ sel: next.length ? encode(next) : undefined })} onAlarm={alarmFrom} />)}
          {tab === "options" && (
            <div className="grid max-w-[46rem] gap-3 text-[14px]" data-testid="options-tab">
              <p>The graph is drawn as lines over the range chosen above, one for each shown row, with the period of each row. The area under a single line is filled.</p>
              <p className="text-muted-foreground">Left is the only Y-axis. The range is not kept in the address: a shared link opens on the last 3 hours.</p>
            </div>
          )}
          {tab === "source" && <pre className="max-h-[32rem] overflow-auto rounded-[10px] border bg-well px-4 py-3 font-mono text-[12.5px] leading-5 whitespace-pre-wrap" data-testid="source-tab">{sourceOf(rows, region)}</pre>}
        </div>
      </Tabs>
      {adding && <AddToDashboard rows={rows} region={region} onClose={() => setAdding(false)} />}
    </>
  );
}

// The Browse tab (cloudwatch__metrics-all, metrics-none): the metrics that have data by namespace and dimension set, and the metrics
// of one set to tick. Used by the metrics page (its place kept in the address) and by the widget page (kept in the page).
import { SearchIcon } from "lucide-react";
import { useMemo } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { HINT } from "../index-page";
import { type MetricRef, useMetrics } from "./api";
import { type GM, nextId } from "./graph-model";
import { type NsRow, groupMetrics, metricsIn, refId } from "./metrics-model";
import { dimensionText } from "./wizard-model";

export type View = { q?: string; name?: string; key?: string };

const NS_COLUMNS = (open: (r: NsRow) => void): DataColumn<NsRow>[] => [
  { id: "ns", header: "Namespace", size: 360, min: 160, accessorKey: "namespace", meta: { mono: true } },
  { id: "dims", header: "Dimensions", min: 200, enableSorting: false, accessorFn: (r) => r.names.join(", "), meta: { mono: true }, cell: (c) => (c.row.original.names.length === 0 ? <span className="text-muted-foreground">None</span> : <button type="button" className="rounded-sm font-mono text-[13px] text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" onClick={() => open(c.row.original)} data-testid="dims-link">{c.row.original.names.join(", ")}</button>) },
  { id: "count", header: "Metrics", size: 110, fit: 86, accessorKey: "count", meta: { numeric: true } },
];

/** `searched`: the list is a search's answer, so a section with none of it says that in a line (the one empty state is for a search that finds nothing at all). */
function Namespaces({ title, lede, rows, open, testId, searched = false }: { title: string; lede: string; rows: NsRow[]; open: (r: NsRow) => void; testId: string; searched?: boolean }) {
  return (
    <section className="grid gap-3" data-testid={testId}>
      <div><h2 className="text-[19px] leading-7 font-semibold tracking-[-0.01em] text-bright">{title}</h2><p className="mt-1 text-[13.5px] text-muted-foreground">{lede}</p></div>
      {rows.length === 0 && searched ? (
        <p className="rounded-[var(--radius-panel)] border border-dashed px-5 py-4 text-[13.5px] text-muted-foreground" data-testid={`${testId}-none`}>No metric in these namespaces matches the search.</p>
      ) : rows.length === 0 ? (
        <EmptyState icon={SearchIcon} title="No metrics found">ListMetrics returned nothing for this search. Metrics appear here once something publishes them.</EmptyState>
      ) : (
        <DataTable aria-label={title} data={rows} columns={NS_COLUMNS(open)} rowId={(r) => `${r.namespace}/${r.key}`} maxHeight={420} />
      )}
    </section>
  );
}

const keyOfRow = (r: GM) => refId({ Namespace: r.ns ?? "", MetricName: r.m ?? "", Dimensions: r.d ?? [] });

export function Browse({ view, onView, rows, onRows, retry }: { view: View; onView: (patch: View) => void; rows: GM[]; onRows: (rows: GM[]) => void; retry?: () => void }) {
  const all = useMetrics();
  const loaded = useMemo(() => all.data ?? [], [all.data]);
  const groups = useMemo(() => groupMetrics(loaded, view.q ?? ""), [loaded, view.q]);
  const drill = view.name !== undefined && view.key !== undefined ? metricsIn(loaded, view.name, view.key, view.q ?? "") : null;
  const chosen = new Set(rows.filter((r) => r.expr === undefined).map(keyOfRow));
  const toggle = (next: Set<string>) => {
    const out = rows.filter((r) => r.expr !== undefined || next.has(keyOfRow(r)));
    for (const m of (drill ?? []).filter((x) => next.has(refId(x)) && !chosen.has(refId(x)))) out.push({ id: nextId(out), ns: m.Namespace, m: m.MetricName, d: m.Dimensions ?? [], stat: "Sum", period: 300, label: m.MetricName, shown: true });
    onRows(out);
  };
  if (all.isError && !all.data) return <ErrorBlock what="The metrics could not be listed" error={all.error} hint={HINT} onRetry={() => (retry ?? (() => void all.refetch()))()} retrying={all.isFetching} testId="list-error" />;
  if (all.isPending) return <Skeleton className="h-64 rounded-[var(--radius-card)]" data-testid="list-loading" />;
  if (drill)
    return (
      <section className="grid gap-4" data-testid="metrics-in">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div><h2 className="text-[19px] leading-7 font-semibold text-bright"><span className="font-mono">{view.name}</span> <span className="font-normal text-muted-foreground">· {view.key || "no dimensions"}</span></h2><p className="mt-1 text-[13.5px] text-muted-foreground">Tick the metrics to graph.</p></div>
          <Button type="button" variant="outline" className="bg-card" onClick={() => onView({ name: undefined, key: undefined })} data-testid="metrics-back">All metrics</Button>
        </div>
        <SelectableTable<MetricRef>
          aria-label="Metrics"
          data={drill}
          rowId={refId}
          selected={chosen}
          onSelectedChange={toggle}
          noun="metric"
          maxHeight={520}
          hint=" "
          columns={[
            { id: "name", header: "Metric name", size: 280, min: 140, accessorFn: (r) => r.MetricName, meta: { mono: true } },
            { id: "dims", header: "Dimensions", min: 200, enableSorting: false, accessorFn: (r) => dimensionText(r.Dimensions ?? []), meta: { mono: true } },
          ]}
        />
      </section>
    );
  const open = (r: NsRow) => onView({ name: r.namespace, key: r.key });
  // A search that finds nothing is the board's one empty state, under Custom namespaces.
  if (groups.custom.length === 0 && groups.aws.length === 0) return <Namespaces testId="custom-namespaces" title="Custom namespaces" lede="Published with PutMetricData by your own code. Pick a dimension set to list its metrics." rows={[]} open={open} />;
  return (
    <div className="grid gap-8" data-testid="browse-tab">
      <Namespaces testId="custom-namespaces" title="Custom namespaces" lede="Published with PutMetricData by your own code. Pick a dimension set to list its metrics." rows={groups.custom} open={open} searched={(view.q ?? "") !== ""} />
      <Namespaces testId="aws-namespaces" title="AWS namespaces" lede="Published by the AWS services you use." rows={groups.aws} open={open} searched={(view.q ?? "") !== ""} />
      <div className="grid gap-4">
        <p className="max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="retention-note"><b className="font-semibold text-foreground">Samples are kept for 24 hours.</b> doze-aws keeps every sample for 24 hours unless it was started with <span className="font-mono">--cloudwatch-retention-hours</span>; the 1d, 3d and 1w ranges show no more than that.</p>
        <p className="max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="no-metrics-note"><b className="font-semibold text-foreground">Only AWS/Lambda is published by a service in doze-aws.</b> Queues, topics, tables, streams and the other services publish no metrics. Publish your own with <span className="font-mono">PutMetricData</span>.</p>
      </div>
    </div>
  );
}

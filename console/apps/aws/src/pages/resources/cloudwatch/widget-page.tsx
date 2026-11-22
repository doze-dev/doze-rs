// Add metric graph (cloudwatch__widget-line, and the same page for Stacked area, Number, Gauge, Bar and Pie): pick the metrics, see the
// graph, and Create widget adds it to the dashboard being worked on (its draft); Save dashboard on the dashboard's page stores it.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Chart } from "@doze/ui/components/app/chart";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { SearchIcon } from "lucide-react";
import { useAbout } from "../../../api/queries";
import { useMetricData, useServiceCrumbs } from "../../../kit";
import { useDashboard } from "./api";
import { getDraft, setDraft } from "./dashboard-draft";
import { parseBody, startOf, stringify, withWidget } from "./dashboard-model";
import { type GM, metricWidget, toQueries, type WidgetView } from "./graph-model";
import { Browse, type View } from "./metrics-browse";
import { GraphedTable } from "./metrics-graphed";
import { RangeBar, useRange } from "./range";

const KINDS: Record<string, { label: string; view: WidgetView; stacked?: boolean }> = {
  line: { label: "Line", view: "timeSeries" },
  stacked: { label: "Stacked area", view: "timeSeries", stacked: true },
  number: { label: "Number", view: "singleValue" },
  gauge: { label: "Gauge", view: "gauge" },
  bar: { label: "Bar", view: "bar" },
  pie: { label: "Pie", view: "pie" },
};

export function WidgetPage({ name, mode }: { name: string; mode: string | undefined }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("cloudwatch");
  const about = useAbout().data;
  const dashboard = useDashboard(name);
  const [kind, setKind] = useState(mode && KINDS[mode] ? mode : "line");
  const [rows, setRows] = useState<GM[]>([]);
  const [view, setView] = useState<View>({});
  const [tab, setTab] = useState("browse");
  const [persist, setPersist] = useState(false);
  const [title, setTitle] = useState("");
  const [range, patchRange, span] = useRange("3h");
  const queries = toQueries(rows);
  const data = useMetricData(queries, span, queries.length > 0);
  const region = about?.region ?? "us-east-1";
  const back = () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "cloudwatch", kind: "dashboards", name }, search: {} });
  const k = KINDS[kind]!;
  const create = () => {
    const body = parseBody(getDraft(name) ?? dashboard.data?.DashboardBody ?? "");
    if (!body) return;
    const widget = metricWidget(rows, { view: k.view, stacked: k.stacked, region, title: title.trim() || undefined });
    if (persist) (widget.properties as Record<string, unknown>).start = startOf(span.to - span.from);
    setDraft(name, stringify(withWidget(body, widget)));
    void navigate({ to: "/resources/$service/$kind/$name", params: { service: "cloudwatch", kind: "dashboards", name }, search: { done: "widget-added" } });
  };
  return (
    <>
      <PageHeader title="Add metric graph" crumbs={[...crumbs, { label: "Dashboards", to: "/resources/cloudwatch/dashboards" }, { label: name, to: `/resources/cloudwatch/dashboards/${name}` }, { label: "Add metric graph", to: `/resources/cloudwatch/dashboards/${name}` }]} sub={<>New {k.label.toLowerCase()} widget on <b className="font-mono font-semibold text-bright">{name}</b>. Choose Create widget to add it to the dashboard, then Save dashboard.</>} />
      {dashboard.isError && <Alert tone="bad" title="The dashboard could not be read" testId="widget-dashboard-error" className="mb-6">The widget is added to the body of {name}, which has to be read first.</Alert>}
      <div className="mb-5 flex flex-wrap items-center gap-3">
        <RangeBar state={range} onChange={patchRange} compact />
        <Select value={kind} onValueChange={setKind}><SelectTrigger className="w-44 bg-card" aria-label="Widget type" data-testid="widget-kind"><SelectValue /></SelectTrigger><SelectContent>{Object.entries(KINDS).map(([id, x]) => <SelectItem key={id} value={id}>{x.label}</SelectItem>)}</SelectContent></Select>
        <Label className="font-normal"><Checkbox checked={persist} onCheckedChange={(v) => setPersist(v === true)} data-testid="persist-range" /> Persist time range</Label>
      </div>
      <div className="mb-8 rounded-[14px] border bg-card p-6" data-testid="widget-preview">
        <Chart series={data.series.map((s) => ({ ...s, area: k.stacked === true || rows.length === 1 }))} from={span.from} to={span.to} height={240} loading={queries.length > 0 && data.isPending} aria-label="The widget over the range" empty={queries.length === 0 ? "No metrics selected." : "No data points in this range."} testId="widget-graph" />
      </div>
      <Tabs value={tab} onValueChange={setTab} className="gap-0">
        <TabsList variant="line" className="h-11 w-full justify-start gap-2 rounded-none border-b p-0" aria-label="Widget views">
          <TabsTrigger value="browse" className="h-11 flex-none px-3 text-[14px]" data-testid="tab-browse">Browse</TabsTrigger>
          <TabsTrigger value="graphed" className="h-11 flex-none px-3 text-[14px]" data-testid="tab-graphed">Graphed metrics{rows.length > 0 && <span className="ml-1.5 rounded-full bg-muted px-1.5 text-[11.5px] text-muted-foreground tabular-nums">{rows.length}</span>}</TabsTrigger>
          <TabsTrigger value="options" className="h-11 flex-none px-3 text-[14px]" data-testid="tab-options">Options</TabsTrigger>
        </TabsList>
        <div className="pt-6" role="tabpanel">
          {tab === "browse" && (
            <div className="grid gap-5">
              <div className="relative w-full max-w-[35rem]"><SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" /><Input value={view.q ?? ""} onChange={(e) => setView({ ...view, q: e.target.value || undefined })} placeholder="Search for a metric, dimension or resource ID" aria-label="Search for a metric, dimension or resource ID" className="bg-card pl-9" data-testid="metric-search" /></div>
              <Browse view={view} onView={(p) => setView({ ...view, ...p })} rows={rows} onRows={setRows} />
            </div>
          )}
          {tab === "graphed" && (rows.length === 0 ? <EmptyState icon={SearchIcon} title="No metrics selected">Browse to a namespace and tick the metrics to graph them here.</EmptyState> : <GraphedTable rows={rows} onChange={setRows} />)}
          {tab === "options" && <div className="grid max-w-[34rem] gap-2"><Label htmlFor="widget-title">Widget title</Label><Input id="widget-title" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Orders placed" data-testid="widget-title" /><p className="text-[13px] text-muted-foreground">Optional. The widget is drawn without a heading line if it has none.</p></div>}
        </div>
      </Tabs>
      <p className="mt-6 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="persist-words"><b className="font-semibold text-foreground">Persist time range</b> keeps this widget on the time range you pick, even if the time range of the rest of the dashboard changes later.</p>
      <p className="mt-3 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="dashboard-local"><b className="font-semibold text-foreground">Stored, not applied locally.</b> doze-aws keeps the dashboard body exactly as written and never interprets it; this console draws it.</p>
      <div className="mt-8 flex flex-wrap items-center justify-end gap-3 border-t pt-5" data-slot="form-footer">
        <Button type="button" variant="outline" className="bg-card" onClick={back} data-testid="widget-cancel">Cancel</Button>
        <Button type="button" disabled={rows.length === 0 || !dashboard.data} onClick={create} data-testid="widget-create">Create widget</Button>
      </div>
    </>
  );
}

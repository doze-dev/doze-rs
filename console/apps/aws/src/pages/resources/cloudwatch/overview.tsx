// CloudWatch's own page (cloudwatch__overview): the alarms by AWS service and the ones in ALARM or changed most recently, three cards
// that go to Alarms, Metrics and Dashboards, and the dashboard named CloudWatch-Default when there is one.
import { Link } from "@tanstack/react-router";
import { BellIcon, BarChart3Icon, LayoutGridIcon } from "lucide-react";
import { useMemo } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { useAbout } from "../../../api/queries";
import { useServiceCrumbs } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { useAlarms, useDashboard, useDashboards, useMetrics } from "./api";
import { parseBody } from "./dashboard-model";
import { type Alarm, awsService, byService, namespaceOf, recentFirst, stateCounts, toMs } from "./model";
import { useRange } from "./range";
import { Widgets } from "./widgets";
import { stateTone } from "./words";

type Row = ReturnType<typeof byService>[number];
const num = "tabular-nums";

const SERVICE_COLUMNS: DataColumn<Row>[] = [
  { id: "service", header: "Service", min: 160, accessorKey: "service" },
  { id: "alarm", header: "ALARM", size: 100, fit: 96, accessorKey: "ALARM", meta: { numeric: true } },
  { id: "insufficient", header: "INSUFFICIENT_DATA", size: 190, fit: 190, accessorKey: "INSUFFICIENT_DATA", meta: { numeric: true } },
  { id: "ok", header: "OK", size: 80, fit: 64, accessorKey: "OK", meta: { numeric: true } },
];

const RECENT_COLUMNS: DataColumn<Alarm>[] = [
  { id: "alarm", header: "Alarm", min: 160, accessorFn: (r) => r.AlarmName, meta: { mono: true }, cell: (c) => <ResourceTo to={{ service: "cloudwatch", kind: "alarms", name: c.row.original.AlarmName }} className="font-mono text-[13px] font-medium" testId="recent-link">{c.row.original.AlarmName}</ResourceTo> },
  { id: "state", header: "State", size: 190, fit: 190, accessorFn: (r) => r.StateValue, cell: (c) => <StatusDot tone={stateTone(c.row.original.StateValue)}>{c.row.original.StateValue}</StatusDot> },
  { id: "updated", header: "State updated", size: 240, fit: 130, accessorFn: (r) => toMs(r.StateUpdatedTimestamp) ?? 0, cell: (c) => { const t = toMs(c.row.original.StateUpdatedTimestamp); return t === null ? "–" : dateTimeUtc(t); } },
];

function Card({ to, icon: Icon, title, children, testId }: { to: string; icon: typeof BellIcon; title: string; children: React.ReactNode; testId: string }) {
  return (
    <Link to={to as never} className="flex items-center gap-4 rounded-[14px] border bg-card px-6 py-5 outline-none hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50" data-testid={testId}>
      <Icon aria-hidden className="size-5 shrink-0 text-good" />
      <span className="grid gap-0.5"><span className="text-[16px] font-semibold text-bright">{title}</span><span className="text-[13.5px] text-muted-foreground">{children}</span></span>
    </Link>
  );
}

function DefaultDashboard() {
  const body = useDashboard("CloudWatch-Default");
  const alarms = useAlarms();
  const [, , span] = useRange("3h");
  const parsed = body.data ? parseBody(body.data.DashboardBody) : null;
  if (!parsed || parsed.widgets.length === 0) return null;
  return (
    <section className="mt-12 grid gap-4" data-testid="default-dashboard">
      <div><h2 className="text-[19px] leading-7 font-semibold text-bright">CloudWatch-Default</h2><p className="mt-1 text-[13.5px] text-muted-foreground">The dashboard of this name is shown here.</p></div>
      <Widgets widgets={parsed.widgets} range={span} alarms={alarms.data} />
    </section>
  );
}

export function Overview() {
  const about = useAbout().data;
  const crumbs = useServiceCrumbs("cloudwatch");
  const alarms = useAlarms();
  const metrics = useMetrics();
  const dashboards = useDashboards();
  const names = useMemo(() => alarms.data ?? [], [alarms.data]);
  const counts = stateCounts(names);
  const services = useMemo(() => byService(names), [names]);
  const recent = useMemo(() => recentFirst(names).slice(0, 3), [names]);
  const own = useMemo(() => [...new Set((metrics.data ?? []).map((m) => m.Namespace).filter((n) => !n.startsWith("AWS/")))].sort(), [metrics.data]);
  const spaces = useMemo(() => new Set((metrics.data ?? []).map((m) => m.Namespace)), [metrics.data]);
  const custom = names.filter((a) => a.StateValue === "ALARM" && !awsService(namespaceOf(a)));
  const refresh = () => { void alarms.refetch(); void metrics.refetch(); void dashboards.refetch(); };
  const lead = `Metrics, alarms and dashboards for account ${about?.account ?? ""} in ${about?.region ?? ""}.${own.length ? ` Your code publishes the ${own.length === 1 ? own[0] : `${own.slice(0, -1).join(", ")} and ${own[own.length - 1]}`} ${own.length === 1 ? "namespace" : "namespaces"}; the others come from the services it uses.` : " The services you use publish their own namespaces; your code can publish more with PutMetricData."}`;
  const dashCount = dashboards.data?.length;
  return (
    <>
      <PageHeader title="CloudWatch" crumbs={crumbs} refresh={<RefreshControl updatedAt={Math.max(alarms.dataUpdatedAt, metrics.dataUpdatedAt)} fetching={alarms.isFetching || metrics.isFetching} onRefresh={refresh} interval={false} />} sub={<span data-testid="overview-lead"><Emphasis text={lead} names={own} /></span>} />
      {alarms.isError && !alarms.data ? (
        <ErrorBlock what="The alarms could not be read" error={alarms.error} hint={HINT} onRetry={refresh} retrying={alarms.isFetching} testId="overview-error" />
      ) : !alarms.data ? (
        <div className="grid gap-4" data-testid="overview-loading"><Skeleton className="h-8 w-60" /><Skeleton className="h-52 rounded-[14px]" /></div>
      ) : (
        <>
          <section className="grid gap-3" data-testid="alarms-by-service">
            <div><h2 className="text-[19px] leading-7 font-semibold tracking-[-0.01em] text-bright">Alarms by AWS service</h2><p className="mt-1 max-w-[46rem] text-[13.5px] text-muted-foreground">The AWS services you use that have alarms, and the alarms that are in ALARM or changed state most recently.{custom[0] && recent.some((r) => r.AlarmName === custom[0]!.AlarmName) ? <> <span className="font-mono text-foreground">{custom[0].AlarmName}</span> watches a custom metric, so it appears only on the right.</> : null}</p></div>
            <div className="grid items-start gap-6 lg:grid-cols-2">
              {services.length === 0 ? <p className="rounded-[14px] border border-dashed px-5 py-8 text-[13.5px] text-muted-foreground" data-testid="no-service-alarms">No alarm watches an AWS service's metric.</p> : <DataTable aria-label="Alarms by AWS service" data={services} columns={SERVICE_COLUMNS} rowId={(r) => r.service} maxHeight={320} />}
              {recent.length === 0 ? <p className="rounded-[14px] border border-dashed px-5 py-8 text-[13.5px] text-muted-foreground" data-testid="no-alarms">There are no alarms yet.</p> : <DataTable aria-label="Recent alarms" data={recent} columns={RECENT_COLUMNS} rowId={(r) => r.AlarmName} maxHeight={320} />}
            </div>
          </section>
          <section className="mt-12 grid gap-3" data-testid="where-to-go">
            <div><h2 className="text-[19px] leading-7 font-semibold tracking-[-0.01em] text-bright">Where to go</h2><p className="mt-1 text-[13.5px] text-muted-foreground">The console's navigation pane: Alarms, Metrics and Dashboards.</p></div>
            <div className="grid gap-4 md:grid-cols-3">
              <Card to="/resources/cloudwatch/alarms" icon={BellIcon} title="Alarms" testId="card-alarms"><span className={num}>{counts.all}</span> {counts.all === 1 ? "alarm" : "alarms"}, <span className={num}>{counts.ALARM}</span> in ALARM</Card>
              <Card to="/resources/cloudwatch/metrics" icon={BarChart3Icon} title="Metrics" testId="card-metrics">{metrics.data ? <>All metrics: <span className={num}>{spaces.size}</span> {spaces.size === 1 ? "namespace" : "namespaces"}</> : "All metrics"}</Card>
              <Card to="/resources/cloudwatch/dashboards" icon={LayoutGridIcon} title="Dashboards" testId="card-dashboards">{dashCount === undefined ? "Dashboards" : <><span className={num}>{dashCount}</span> custom {dashCount === 1 ? "dashboard" : "dashboards"}</>}</Card>
            </div>
          </section>
          {dashboards.data?.some((d) => d.DashboardName === "CloudWatch-Default") && <DefaultDashboard />}
        </>
      )}
    </>
  );
}

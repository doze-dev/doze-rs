import { useMemo } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { number } from "@doze/ui/lib/format";
import { useServices } from "../../api/queries";
import type { ServiceRow } from "../../api/types/ServiceRow";
import type { ServiceSeen } from "../../api/types/ServiceSeen";
import { ago } from "../../people";
import { CardSkeleton, Section } from "../card";

type Row = ServiceRow & { seen: ServiceSeen | null };

const percent = (r: ServiceRow) => (r.total > 0 ? Math.round((r.built / r.total) * 100) : 0);

function columns(now: number): DataColumn<Row>[] {
  return [
    { id: "title", header: "Service", accessorKey: "title", cell: (c) => <span className="font-medium text-bright">{c.getValue<string>()}</span> },
    { id: "name", header: "Name", size: 150, accessorKey: "name", meta: { mono: true } },
    {
      id: "state",
      header: "This run",
      size: 130,
      accessorFn: (r) => (r.served ? "Served" : "Not served"),
      cell: (c) => <StatusDot tone={c.row.original.served ? "ok" : "idle"}>{c.getValue<string>()}</StatusDot>,
    },
    {
      id: "called",
      header: "Apps last called",
      size: 170,
      sortDescFirst: true,
      accessorFn: (r) => r.seen?.last_ms ?? 0,
      cell: (c) => {
        const s = c.row.original.seen;
        return s ? <span title={`${number(s.calls)} calls in the Live record`}>{ago(s.last_ms, now)}</span> : <span className="text-muted-foreground">Not in Live</span>;
      },
    },
    { id: "built", header: "Operations built", size: 170, accessorKey: "built", sortDescFirst: true, meta: { numeric: true }, cell: (c) => `${number(c.row.original.built)} of ${number(c.row.original.total)}` },
    { id: "share", header: "Share", size: 90, accessorFn: percent, sortDescFirst: true, meta: { numeric: true }, cell: (c) => `${c.getValue<number>()}%` },
    { id: "apis", header: "APIs", size: 220, enableSorting: false, accessorFn: (r) => r.apis.map((a) => a.sdk_id).join(", ") },
  ];
}

/** The services of the build: served or not, when apps last called each (from Live), and how much of each is built. doze-aws
 * does not report whether a service's actor is asleep, so this says when it was called, not whether it is awake. */
export function BuildServices({ called, now }: { called: ServiceSeen[]; now: number }) {
  const services = useServices();
  const list = services.data?.services;
  const rows = useMemo<Row[] | undefined>(() => list?.map((s) => ({ ...s, seen: called.find((c) => c.service === s.name) ?? null })), [list, called]);
  const cols = useMemo(() => columns(now), [now]);
  const counts = list ? { served: list.filter((r) => r.served).length, all: list.length, built: list.reduce((n, r) => n + r.built, 0), total: list.reduce((n, r) => n + r.total, 0) } : null;
  return (
    <Section
      title="Services"
      testId="services"
      lede={
        counts ? (
          <span data-testid="services-counts">
            {number(counts.served)} of {number(counts.all)} are served in this run; {number(counts.built)} of {number(counts.total)} operations are built. A service sleeps until its first call and costs almost nothing until then.
          </span>
        ) : (
          "Every service of this build, and how many of AWS's operations each answers."
        )
      }
    >
      {services.isError && !rows ? (
        <ErrorBlock what="The services could not be loaded" error={services.error} hint="Check that doze-aws is still running, then try again." onRetry={() => void services.refetch()} retrying={services.isFetching} testId="services-error" />
      ) : rows ? (
        <DataTable aria-label="Services of this build" data={rows} columns={cols} rowId={(r) => r.name} maxHeight={720} empty="This build has no services." />
      ) : (
        <div className="rounded-xl border bg-card px-6 py-2">
          <CardSkeleton rows={6} />
        </div>
      )}
    </Section>
  );
}

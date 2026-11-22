// CloudWatch's dashboards (cloudwatch__dashboards-list, dashboards-none, dashboard-create, dashboard-delete): the list, rows chosen
// for Delete, Create dashboard as a dialog (`?dialog=create`), and the empty state a delete of the last one lands on.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { LayoutDashboardIcon, PlusIcon, SearchIcon, TrashIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { Alert, ErrorBlock } from "@doze/ui/components/app/alert";
import type { DataColumn } from "@doze/ui/components/app/data-table";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { PlainConfirm, useServiceCrumbs } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { type DashboardEntry, useDashboards, useDeleteDashboards } from "./api";
import { CreateDashboard } from "./dashboard-create";
import { toMs } from "./model";

const COLUMNS: DataColumn<DashboardEntry>[] = [
  { id: "name", header: "Name", min: 200, accessorFn: (r) => r.DashboardName.toLowerCase(), meta: { mono: true }, cell: (c) => <ResourceTo to={{ service: "cloudwatch", kind: "dashboards", name: c.row.original.DashboardName }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.DashboardName}</ResourceTo> },
  { id: "modified", header: "Last modified", size: 220, fit: 130, accessorFn: (r) => toMs(r.LastModified) ?? 0, cell: (c) => { const t = toMs(c.row.original.LastModified); return t === null ? "–" : dateTimeUtc(t); } },
];

export const deleteWords = (names: readonly string[]): string => (names.length === 1 ? `Delete ${names[0]}. The dashboard and its widgets are removed.` : `Delete ${names.join(", ")}. The dashboards and their widgets are removed.`);

export function DashboardsList({ service, kind }: { service: string; kind: string }) {
  const navigate = useNavigate();
  const search = useSearch({ strict: false }) as { dialog?: string; gone?: string };
  const crumbs = useServiceCrumbs(service);
  const q = useDashboards();
  const del = useDeleteDashboards();
  const [typed, setTyped] = useState("");
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [asking, setAsking] = useState(false);
  const [done, setDone] = useState<string[] | null>(null);
  const all = useMemo(() => [...(q.data ?? [])].sort((a, b) => a.DashboardName.localeCompare(b.DashboardName)), [q.data]);
  const rows = all.filter((r) => r.DashboardName.toLowerCase().includes(typed.trim().toLowerCase()));
  const picked = all.filter((r) => chosen.has(r.DashboardName)).map((r) => r.DashboardName);
  const go = (s: { dialog?: string; gone?: string }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: s, replace: true });
  const none = q.data !== undefined && all.length === 0;
  const create = () => go({ dialog: "create" });
  const deleted = done ?? (search.gone ? [search.gone] : null);
  return (
    <>
      <PageHeader
        title="Dashboards"
        crumbs={[...crumbs, { label: "Dashboards", to: `/resources/${service}/${kind}` }]}
        sub="Custom dashboards put graphs, numbers, alarm states and notes on one page."
        refresh={<RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={() => void q.refetch()} interval={false} />}
        actions={<Button onClick={create} data-testid="create-open"><PlusIcon /> Create dashboard</Button>}
      />
      {deleted && <ResultBanner className="mb-6" title={deleted.length === 1 ? `Dashboard ${deleted[0]} deleted.` : `${deleted.length} dashboards deleted.`} returned="DeleteDashboards removed the dashboards and their widgets. The metrics and alarms they showed stay." onDismiss={() => { setDone(null); go({}); }} testId="deleted-banner" />}
      {q.isError && !q.data ? (
        <ErrorBlock what="The dashboards could not be listed" error={q.error} hint={HINT} onRetry={() => void q.refetch()} retrying={q.isFetching} testId="list-error" />
      ) : !q.data ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-48 rounded-xl" /></div>
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={LayoutDashboardIcon} title="No dashboards" action={<Button onClick={create} data-testid="empty-create"><PlusIcon /> Create dashboard</Button>}>A dashboard is a saved arrangement of graphs and widgets.</EmptyState>
        </div>
      ) : (
        <>
          <SelectableTable<DashboardEntry>
            aria-label="Dashboards"
            data={rows}
            columns={COLUMNS}
            rowId={(r) => r.DashboardName}
            selected={chosen}
            onSelectedChange={setChosen}
            noun="dashboard"
            maxHeight={620}
            actionsEnd
            hint=" "
            selectedLabel={(n) => `${n} ${n === 1 ? "dashboard" : "dashboards"} selected`}
            toolbar={
              <div className="relative w-[18rem] max-w-full">
                <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search dashboards" aria-label="Search dashboards" className="bg-card pl-9" data-testid="list-search" />
              </div>
            }
            actions={() => <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={picked.length === 0} onClick={() => setAsking(true)} data-testid="action-delete"><TrashIcon /> Delete</Button>}
            empty={`No dashboard's name contains “${typed}”.`}
          />
          <div className="mt-6 grid gap-3">
            <p className="max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="dashboards-scope"><b className="font-semibold text-foreground">A dashboard belongs to the Region it was made in.</b> AWS keeps dashboards for the account, in no Region; doze-aws keeps them with the Region's other CloudWatch data. A dashboard named <span className="font-mono">CloudWatch-Default</span> is drawn on the CloudWatch overview page.</p>
            <p className="max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="dashboards-local"><b className="font-semibold text-foreground">Stored, not applied locally.</b> doze-aws keeps the dashboard body exactly as written and never interprets it; this console draws it.</p>
          </div>
        </>
      )}
      {search.dialog === "create" && <CreateDashboard names={all.map((r) => r.DashboardName)} onClose={() => go({})} />}
      <PlainConfirm
        open={asking}
        onOpenChange={(o) => { setAsking(o); if (!o) del.reset(); }}
        title="Delete dashboards?"
        description={<Emphasis text={deleteWords(picked)} names={picked} />}
        verb="Delete dashboards"
        call="DeleteDashboards"
        pending={del.isPending}
        disabled={picked.length > 100}
        error={del.error}
        testId="delete-dialog"
        onConfirm={() => del.mutate(picked, { onSuccess: () => { setAsking(false); setChosen(new Set()); setDone(picked); } })}
      >
        <p className="text-[13.5px] text-muted-foreground">One call can delete up to 100 dashboards.</p>
        {picked.length > 100 && <Alert tone="warn" title="Too many for one call" testId="delete-too-many">Choose 100 dashboards or fewer.</Alert>}
      </PlainConfirm>
    </>
  );
}

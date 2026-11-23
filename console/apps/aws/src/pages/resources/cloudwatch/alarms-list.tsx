// CloudWatch's alarms (cloudwatch__alarms-list, alarms-none, alarm-delete): the list with its state chips and counts, rows chosen
// with their checkboxes for Actions > Delete, and the empty state a delete of the last alarm lands on. `?do=create` is the wizard.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { BellIcon, PlusIcon, SearchIcon, TrashIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import type { DataColumn } from "@doze/ui/components/app/data-table";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { FilterChips } from "@doze/ui/components/app/filter-chips";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { PlainConfirm, useServiceCrumbs } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { useAlarms, useDeleteAlarms } from "./api";
import { CreateAlarm } from "./wizard";
import { type Alarm, conditionWords, stateCounts, toMs } from "./model";
import { deleteWords, stateTone, watching } from "./words";

export const ALARM_COLUMNS: DataColumn<Alarm>[] = [
  { id: "name", header: "Name", min: 180, accessorFn: (r) => r.AlarmName.toLowerCase(), meta: { mono: true }, cell: (c) => (
    <span className="grid gap-0.5">
      <ResourceTo to={{ service: "cloudwatch", kind: "alarms", name: c.row.original.AlarmName }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.AlarmName}</ResourceTo>
      <span className="font-mono text-[12px] text-muted-foreground">{c.row.original.Metrics || c.row.original.MetricName ? "Metric alarm" : "Composite alarm"}</span>
    </span>
  ) },
  { id: "state", header: "State", size: 190, fit: 190, accessorFn: (r) => r.StateValue, cell: (c) => <StatusDot tone={stateTone(c.row.original.StateValue)}>{c.row.original.StateValue}</StatusDot> },
  { id: "updated", header: "State updated", size: 240, fit: 190, accessorFn: (r) => toMs(r.StateUpdatedTimestamp) ?? 0, cell: (c) => { const t = toMs(c.row.original.StateUpdatedTimestamp); return t === null ? "–" : dateTimeUtc(t); } },
  { id: "condition", header: "Condition", min: 220, enableSorting: false, accessorFn: (r) => conditionWords(r), cell: (c) => <span className="block py-1 text-[13.5px] leading-5 whitespace-normal" data-testid="condition">{c.row.original.AlarmRule ?? conditionWords(c.row.original)}</span> },
  { id: "actions", header: "Actions enabled", size: 150, fit: 150, enableSorting: false, accessorFn: (r) => (r.ActionsEnabled ? 1 : 0), cell: (c) => (c.row.original.ActionsEnabled ? "Yes" : "No") },
];

export function AlarmsList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { do?: string; name?: string; gone?: string };
  if (search.do === "create") return <CreateAlarm editing={search.name} />;
  return <Alarms service={service} kind={kind} gone={search.gone} />;
}

function Alarms({ service, kind, gone }: { service: string; kind: string; gone?: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs(service);
  const q = useAlarms();
  const del = useDeleteAlarms();
  const [typed, setTyped] = useState("");
  const [state, setState] = useState("");
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [asking, setAsking] = useState(false);
  const [done, setDone] = useState<string[] | null>(null);
  const all = useMemo(() => [...(q.data ?? [])].sort((a, b) => (toMs(b.StateUpdatedTimestamp) ?? 0) - (toMs(a.StateUpdatedTimestamp) ?? 0)), [q.data]);
  const counts = stateCounts(all);
  const rows = all.filter((a) => (!state || a.StateValue === state) && a.AlarmName.toLowerCase().includes(typed.trim().toLowerCase()));
  const picked = all.filter((a) => chosen.has(a.AlarmName));
  const create = () => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: { do: "create" } });
  const refresh = () => void q.refetch();
  const none = q.data !== undefined && all.length === 0;
  return (
    <>
      <PageHeader
        title="Alarms"
        crumbs={crumbs}
        sub="Alarms watch one metric and run actions when their state changes."
        refresh={<RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={refresh} interval={false} />}
        actions={<Button onClick={create} data-testid="create-open"><PlusIcon /> Create alarm</Button>}
      />
      {!none && (done ?? (gone ? [gone] : null)) && <ResultBanner className="mb-6" title={(done ?? [gone])?.length === 1 ? `Alarm ${(done ?? [gone])[0]} deleted.` : `${done?.length} alarms deleted.`} returned="DeleteAlarms removed it with its history. The metric it watched stays." onDismiss={() => { setDone(null); if (gone) void navigate({ to: ".", search: {}, replace: true }); }} testId="deleted-banner" />}
      {q.isError && !q.data ? (
        <ErrorBlock what="The alarms could not be listed" error={q.error} hint={HINT} onRetry={refresh} retrying={q.isFetching} testId="list-error" />
      ) : !q.data ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={BellIcon} title="No alarms" action={<Button onClick={create} data-testid="empty-create"><PlusIcon /> Create alarm</Button>}>
            An alarm watches one metric and changes state when it crosses a threshold.
          </EmptyState>
        </div>
      ) : (
        <SelectableTable<Alarm>
          aria-label="Alarms"
          data={rows}
          columns={ALARM_COLUMNS}
          rowId={(r) => r.AlarmName}
          selected={chosen}
          onSelectedChange={setChosen}
          noun="alarm"
          maxHeight={680}
          rowHeight={64}
          actionsEnd
          hint=" "
          selectedLabel={(n) => `${n} ${n === 1 ? "alarm" : "alarms"} selected`}
          toolbar={
            <div className="flex flex-wrap items-center gap-3">
              <div className="relative w-[18rem] max-w-full">
                <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search alarms" aria-label="Search alarms" className="bg-card pl-9" data-testid="list-search" />
              </div>
              <FilterChips label="Filter alarms by state" value={state} onChange={setState} chips={[{ value: "", label: "All", count: counts.all }, { value: "ALARM", label: "ALARM", count: counts.ALARM }, { value: "OK", label: "OK", count: counts.OK }, { value: "INSUFFICIENT_DATA", label: "INSUFFICIENT_DATA", count: counts.INSUFFICIENT_DATA }]} />
            </div>
          }
          actions={() => (
            <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={picked.length === 0} onClick={() => setAsking(true)} data-testid="action-delete"><TrashIcon /> Actions › Delete</Button>
          )}
          empty={typed ? `No alarm's name contains “${typed}”.` : "No alarm is in this state."}
        />
      )}
      <PlainConfirm
        open={asking}
        onOpenChange={(o) => { setAsking(o); if (!o) del.reset(); }}
        title={picked.length === 1 ? `Delete ${picked[0]?.AlarmName}?` : `Delete ${picked.length} alarms?`}
        description={<Emphasis text={deleteWords(picked)} names={[...picked.map((a) => a.AlarmName), ...picked.map(watching)]} />}
        verb="Delete"
        call="DeleteAlarms"
        pending={del.isPending}
        error={del.error}
        testId="delete-dialog"
        onConfirm={() => {
          const names = picked.map((a) => a.AlarmName);
          del.mutate(names, { onSuccess: () => { setAsking(false); setChosen(new Set()); setDone(names); } });
        }}
      />
    </>
  );
}

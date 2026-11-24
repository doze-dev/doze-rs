// CloudWatch Logs' log groups: the list (logs__log-groups, log-groups-none, and the list a deletion lands on, log-group-deleted). A row is
// chosen with its checkbox for Delete; its Retention opens the retention setting, its Metric filters count opens the group's filters.
// `?create=yes` is Create log group, as a page.
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { PlusIcon, ScrollTextIcon, SearchIcon, TrashIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import type { DataColumn } from "@doze/ui/components/app/data-table";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useDelete, useResourceList } from "../../../api/resources";
import { PlainConfirm } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo, Value } from "../value";
import { LogsCreate } from "./create";
import { type GroupRow, classWords, count, groupRows, listSentence, retentionWords } from "./model";
import { RetentionDialog } from "./retention-dialog";
import { FromYourApp } from "../../../kit/from-your-app";

const NAMES = (rows: readonly GroupRow[]) => rows.map((r) => r.name);

export function LogsList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { create?: string; gone?: string };
  if (search.create === "yes") return <LogsCreate />;
  return <Groups service={service} kind={kind} gone={search.gone} />;
}

function Groups({ service, kind, gone }: { service: string; kind: string; gone?: string }) {
  const navigate = useNavigate();
  const list = useResourceList(service, kind);
  const del = useDelete(service, kind);
  const [typed, setTyped] = useState("");
  const [shown, setShown] = useState("");
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [asking, setAsking] = useState(false);
  const [retention, setRetention] = useState<GroupRow | null>(null);
  const [failed, setFailed] = useState<unknown>(null);
  const { refetch } = list;
  const refresh = useCallback(() => void refetch(), [refetch]);
  const first = list.data?.pages[0];
  const all = useMemo(() => groupRows(list.data?.pages.flatMap((p) => p.rows) ?? []), [list.data]);
  const rows = useMemo(() => all.filter((r) => r.name.toLowerCase().includes(shown.trim().toLowerCase())), [all, shown]);
  const picked = all.filter((r) => chosen.has(r.name));
  const go = (next: { create?: "yes"; gone?: string }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: next });
  const toGroup = (name: string, tab?: string) => ({ to: "/resources/$service/$kind/$name" as const, params: { service, kind, name }, search: tab ? { tab } : {} });

  const columns: DataColumn<GroupRow>[] = [
    { id: "name", header: "Log group", min: 200, accessorFn: (r) => r.name.toLowerCase(), meta: { mono: true }, cell: (c) => <ResourceTo to={{ service, kind, name: c.row.original.name }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
    { id: "class", header: "Log class", size: 130, accessorFn: (r) => classWords(r.cls), cell: (c) => <span className="inline-flex rounded-full border bg-secondary px-2.5 py-0.5 text-[12.5px] text-secondary-foreground">{classWords(c.row.original.cls)}</span> },
    { id: "retention", header: "Retention", size: 150, accessorFn: (r) => r.retention ?? Number.POSITIVE_INFINITY, cell: (c) => <button type="button" onClick={() => setRetention(c.row.original)} title={`Change the retention of ${c.row.original.name}`} className="rounded-sm text-left text-[13.5px] text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="retention-open">{retentionWords(c.row.original.retention)}</button> },
    { id: "filters", header: "Metric filters", size: 140, accessorFn: (r) => r.filters, meta: { numeric: true }, cell: (c) => c.row.original.filters > 0 ? <Link {...toGroup(c.row.original.name, "metric-filters")} className="rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="filters-open">{c.row.original.filters}</Link> : <span>0</span> },
    { id: "created", header: "Creation time", size: 200, accessorFn: (r) => r.created ?? "", cell: (c) => <Value value={c.row.original.created} format="time" /> },
  ];

  const none = all.length === 0;
  const sentence = listSentence(all);
  const remove = () => {
    setFailed(null);
    const names = picked.map((p) => p.name);
    void (async () => {
      try {
        for (const n of names) await del.mutateAsync(n);
        setAsking(false);
        setChosen(new Set());
        go({ gone: names.join(", ") });
      } catch (e) {
        setFailed(e);
      }
    })();
  };
  return (
    <>
      <PageHeader
        title="Log groups"
        crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.service_title, to: `/resources/${service}` }] : [])]}
        sub="A log group is a group of log streams that share the same retention, monitoring, and access control settings."
        refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={refresh} interval={false} />}
      />
      {gone && <ResultBanner className="mb-6" title="Deleted." returned={<>DeleteLogGroup returned 200. <b className="font-mono font-semibold text-bright">{gone}</b> and the log events in {gone.includes(",") ? "them" : "it"} are gone.</>} onDismiss={() => go({})} testId="deleted-banner" />}
      {list.isError && !first ? (
        <ErrorBlock what="The log groups could not be listed" error={list.error} hint={HINT} onRetry={refresh} retrying={list.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={ScrollTextIcon} title="No log groups" action={<Button onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Create log group</Button>}>
            Log groups appear when a function runs or a producer writes. You can also create one.
          </EmptyState>
          <FromYourApp service="logs" />
        </div>
      ) : (
        <>
          {sentence && <p className="mb-6 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="list-sentence"><Emphasis text={sentence} names={NAMES(all)} bold={[/^\d[\d,]* log groups?\./]} /></p>}
          <SelectableTable<GroupRow>
            aria-label="Log groups"
            data={rows}
            columns={columns}
            rowId={(r) => r.name}
            selected={chosen}
            onSelectedChange={setChosen}
            noun="log group"
            maxHeight={680}
            actionsAlways
            actionsEnd
            hint=" "
            toolbar={
              <form className="flex min-w-0 flex-1 flex-wrap items-center gap-3" role="search" onSubmit={(e) => { e.preventDefault(); setShown(typed); }}>
                <div className="relative w-[22rem] max-w-full">
                  <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                  <Input value={typed} onChange={(e) => { setTyped(e.target.value); setShown(e.target.value); }} placeholder="Filter log groups by name" aria-label="Filter log groups by name" className="bg-card pl-9" data-testid="list-search" />
                </div>
                <Button type="submit" variant="outline" className="bg-card">Search</Button>
              </form>
            }
            actions={(sel) => (
              <>
                <span className="mr-1 text-[13.5px] text-muted-foreground">Actions</span>
                <Button type="button" onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Create log group</Button>
                <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={sel.length === 0} onClick={() => { setFailed(null); setAsking(true); }} data-testid="action-delete"><TrashIcon /> Delete log group(s)</Button>
              </>
            )}
            empty={`No log group's name contains “${shown}”.`}
          />
          <Pagination className="mt-3" shown={rows.length} total={rows.length} more={list.hasNextPage} onMore={() => void list.fetchNextPage()} loading={list.isFetchingNextPage} noun={rows.length === 1 ? "log group" : "log groups"} />
          <p className="mt-8 max-w-[44rem] text-[13px] leading-5 text-muted-foreground" data-testid="list-teaching">
            The columns are fields that DescribeLogGroups returns (logGroupName, logGroupClass, retentionInDays, metricFilterCount, creationTime). Never Expire means no retention policy is set: on AWS the events are then kept indefinitely, and doze-aws drops those older than its <span className="font-mono">--logs-retention-days</span> (7 by default).
          </p>
        </>
      )}
      <PlainConfirm
        open={asking}
        onOpenChange={(o) => { setAsking(o); if (!o) setFailed(null); }}
        title="Delete log group(s)"
        description={<>DeleteLogGroup deletes {picked.length === 1 ? <b className="font-mono font-semibold text-bright">{picked[0]?.name}</b> : `${count(picked.length, "log group")}`} and permanently deletes all the archived log events associated with {picked.length === 1 ? "it" : "them"}.</>}
        call="DeleteLogGroup"
        pending={del.isPending}
        error={failed}
        onConfirm={remove}
        testId="delete-dialog"
      />
      <RetentionDialog group={retention} onClose={() => setRetention(null)} />
    </>
  );
}

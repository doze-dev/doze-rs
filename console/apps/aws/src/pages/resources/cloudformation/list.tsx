// The stacks (cloudformation__stacks-list, stacks-none; and the list a deletion's "Back to stacks" lands on): a sentence on what
// the list holds, the navigation pane, a filter by status (Deleted reads the records CloudFormation keeps of deleted stacks), and the
// rows, a nested stack marked.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { ChevronDownIcon, FilterIcon, LayersIcon, PlusIcon } from "lucide-react";
import { useMemo } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import { HINT } from "../index-page";
import type { OwnProps } from "../deep/registry";
import { at } from "../deep/parts";
import { useCfnRead } from "./api";
import { listSummary, type StackRow, stackRow } from "./model";
import { Nav } from "./nav";
import { Lede, StackTo, Status } from "./parts";

const COLUMNS: DataColumn<StackRow>[] = [
  {
    id: "name",
    header: "Stack name",
    min: 220,
    accessorFn: (r) => r.name.toLowerCase(),
    cell: (c) => (
      <span className="grid justify-items-start gap-1 py-1">
        {c.row.original.nested && <Badge variant="secondary" className="font-normal tracking-wide text-muted-foreground" data-testid="nested-marker">NESTED</Badge>}
        <StackTo name={c.row.original.name} />
      </span>
    ),
  },
  { id: "status", header: "Status", size: 280, accessorFn: (r) => r.status, cell: (c) => <Status status={c.row.original.status} /> },
  { id: "created", header: "Creation time", size: 230, accessorFn: (r) => r.created ?? "", cell: (c) => at(c.row.original.created) },
];

const FILTERS = [["", "All"], ["COMPLETE", "Complete"], ["IN_PROGRESS", "In progress"], ["FAILED", "Failed"], ["ROLLBACK", "Rolled back"], ["DELETED", "Deleted"]] as const;

export function StacksList({ service, kind }: { service: string; kind: string }) {
  // The filter is kept in the address as `?q=` (the list search the router already carries for a kind): `?q=ROLLBACK`.
  const search = useSearch({ strict: false }) as { q?: string };
  const status = FILTERS.some(([v]) => v === search.q) ? (search.q ?? "") : "";
  const navigate = useNavigate();
  const region = useAbout().data?.region ?? "this Region";
  const deleted = status === "DELETED";
  const list = useResourceList(service, kind, false, deleted || !status ? {} : { status });
  const all = useResourceList(service, kind);
  const gone = useCfnRead<{ StackSummaries?: { StackName: string; StackStatus: string; CreationTime?: string | number }[] }>("ListStacks", {}, { enabled: deleted });
  const first = list.data?.pages[0];
  const rows = useMemo<StackRow[]>(() => (deleted ? (gone.data?.StackSummaries ?? []).map((s) => ({ name: s.StackName, status: s.StackStatus, created: s.CreationTime === undefined ? null : String(s.CreationTime), nested: false })) : (list.data?.pages.flatMap((p) => p.rows) ?? []).map(stackRow)), [deleted, gone.data, list.data]);
  const everything = useMemo(() => (all.data?.pages.flatMap((p) => p.rows) ?? []).map(stackRow), [all.data]);
  const counts = first?.params.find((p) => p.key === "status")?.options ?? [];
  const create = () => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: { do: "create" } });
  const setStatus = (value: string) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: value ? { q: value } : {}, replace: true });
  const refresh = () => {
    void list.refetch();
    void all.refetch();
  };
  const loading = !first || (deleted && gone.isPending);
  const none = everything.length === 0 && !all.isPending && !status;
  const said = listSummary(everything, region);
  return (
    <>
      <PageHeader
        title="Stacks"
        crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.group, to: "/resources" }, { label: first.service_title, to: `/resources/${service}` }] : [])]}
        sub={none ? "Groups of resources you create, update and delete together from a template." : "Groups of resources you create, update and delete together from a template. Choose a stack name to open it."}
        refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={refresh} interval={false} />}
        actions={<Button onClick={create} data-testid="create-open"><PlusIcon /> Create stack</Button>}
      />
      {list.isError && !first ? (
        <ErrorBlock what="The stacks could not be listed" error={list.error} hint={HINT} onRetry={refresh} retrying={list.isFetching} testId="list-error" />
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={LayersIcon} title="No stacks" action={<Button variant="outline" className="bg-card" onClick={create}><PlusIcon /> Create stack</Button>}>
            You have no stacks in this region. Create a stack from a template to provision resources together.
          </EmptyState>
        </div>
      ) : (
        <>
          {!all.isPending && <Lede said={said} className="mb-5" testId="list-sentence" />}
          <Nav active="stacks" />
          <div className="mb-4 flex flex-wrap items-center gap-3">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button type="button" variant="outline" className="rounded-full bg-card" data-testid="filter-status"><FilterIcon /> Filter status{status && <span className="font-medium text-bright">: {FILTERS.find(([v]) => v === status)?.[1]}</span>} <ChevronDownIcon /></Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="min-w-56">
                <DropdownMenuRadioGroup value={status} onValueChange={setStatus}>
                  {FILTERS.map(([value, label]) => {
                    const n = counts.find((o) => o.value === value)?.count;
                    return <DropdownMenuRadioItem key={value || "all"} value={value} data-testid={`filter-${value || "all"}`}>{label}{n !== undefined && n !== null && <span className="ml-auto pl-4 text-muted-foreground tabular-nums">{n}</span>}</DropdownMenuRadioItem>;
                  })}
                </DropdownMenuRadioGroup>
              </DropdownMenuContent>
            </DropdownMenu>
            {deleted && <p className="text-[13px] text-muted-foreground" data-testid="deleted-note">CloudFormation keeps the record of a deleted stack by its stack ID; by name it is gone.</p>}
          </div>
          {loading ? (
            <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-64 rounded-xl" /></div>
          ) : (
            <>
              <DataTable aria-label="Stacks" data={rows} columns={COLUMNS} rowId={(r) => r.name} maxHeight={680} rowHeight={56} empty={deleted ? "No stack has been deleted." : "No stack has that status."} />
              <Pagination className="mt-3" shown={rows.length} total={rows.length} more={!deleted && list.hasNextPage} onMore={() => void list.fetchNextPage()} loading={list.isFetchingNextPage} noun={rows.length === 1 ? "stack" : "stacks"} />
            </>
          )}
        </>
      )}
    </>
  );
}

export const StacksListPage = (p: OwnProps) => <StacksList service={p.service} kind={p.kind} />;

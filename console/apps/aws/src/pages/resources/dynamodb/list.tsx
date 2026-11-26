// The tables' list (dynamodb__tables-list, tables-none, table-created, table-deleted): every table with its keys, indexes and capacity,
// chosen to Delete (a dialog over the list), PartiQL editor and Create table as pages of their own. What the page was left with (a
// table made, a table deleted) is said at the top, from the address (`?dialog=created&name=`).
import { CodeIcon, PlusIcon, TableIcon, Trash2Icon } from "lucide-react";
import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import type { DataColumn } from "@doze/ui/components/app/data-table";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import { HINT } from "../index-page";
import type { OwnProps } from "../deep/registry";
import { ResourceTo } from "../value";
import { listRows, listSentence, modeWords, type TableRow, TYPE_WORDS, unitsWords } from "./model";
import { crumbs, useListNav } from "./parts";
import { TableDelete } from "./table-delete";
import { FromYourApp } from "../../../kit/from-your-app";

const Key = ({ k }: { k: TableRow["hash"] | null }) =>
  k ? (
    <span className="grid">
      <span className="font-mono text-[13px]">{k.name}</span>
      <span className="text-[12.5px] text-muted-foreground">{TYPE_WORDS[k.type]}</span>
    </span>
  ) : (
    <span className="text-muted-foreground">–</span>
  );

const COLUMNS: DataColumn<TableRow>[] = [
  { id: "name", header: "Name", accessorFn: (r) => r.name, size: 240, min: 150, meta: { mono: true }, cell: (c) => <ResourceTo to={{ service: "dynamodb", kind: "tables", name: c.row.original.name }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
  { id: "status", header: "Status", accessorFn: (r) => r.status, size: 130, fit: 90, cell: (c) => <StatusDot tone={c.row.original.status === "ACTIVE" ? "ok" : c.row.original.status === "CREATING" || c.row.original.status === "UPDATING" ? "warn" : "idle"}>{c.row.original.status.charAt(0) + c.row.original.status.slice(1).toLowerCase()}</StatusDot> },
  { id: "pk", header: "Partition key", accessorFn: (r) => r.hash.name, size: 220, min: 130, cell: (c) => <Key k={c.row.original.hash} /> },
  { id: "sk", header: "Sort key", accessorFn: (r) => r.range?.name ?? "", size: 220, min: 130, cell: (c) => <Key k={c.row.original.range} /> },
  { id: "indexes", header: "Indexes", accessorFn: (r) => r.indexes, size: 110, fit: 80, meta: { numeric: true } },
  { id: "mode", header: "Capacity mode", accessorFn: (r) => modeWords(r), size: 220, min: 150, cell: (c) => (
      <span className="grid">
        <span>{modeWords(c.row.original)}</span>
        {unitsWords(c.row.original) && <span className="text-[12.5px] text-muted-foreground">{unitsWords(c.row.original)}</span>}
      </span>
    ) },
];

export function TablesList({ search }: OwnProps) {
  const list = useResourceList("dynamodb", "tables");
  const about = useAbout().data;
  const go = useListNav();
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [deleting, setDeleting] = useState<string | null>(null);
  const first = list.data?.pages[0];
  const rows = useMemo(() => listRows(list.data?.pages.flatMap((p) => p.rows) ?? []), [list.data]);
  const region = about?.region ?? "this Region";
  const one = selected.size === 1 ? [...selected][0] : undefined;
  const made = search.dialog === "created" && search.name ? rows.find((r) => r.name === search.name) : undefined;
  const gone = search.dialog === "deleted" && search.name ? search.name : undefined;
  // The count of the items that went with it, written `n318` (the router reads a bare number as a number and drops it from a text member).
  const wentWith = search.sel?.startsWith("n") ? Number(search.sel.slice(1)) : null;
  const create = (
    <Button onClick={() => go({ do: "create" })} data-testid="create-open">
      <PlusIcon /> Create table
    </Button>
  );
  const none = first && rows.length === 0;
  return (
    <div data-testid="tables-list">
      <PageHeader
        title="Tables"
        crumbs={crumbs().slice(0, 3)}
        sub={`Tables in ${region}.${none ? "" : " Choose a table name to open it. The columns are fields of DescribeTable."}`}
        refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={() => void list.refetch()} interval={false} />}
        actions={
          <>
            {!none && (
              <>
                <Button variant="outline" className="bg-card" onClick={() => go({ do: "partiql" })} data-testid="partiql-open">
                  <CodeIcon /> PartiQL editor
                </Button>
                <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={!one} onClick={() => one && setDeleting(one)} data-testid="table-delete">
                  <Trash2Icon /> Delete
                </Button>
              </>
            )}
            {create}
          </>
        }
      />
      {made && <ResultBanner className="mb-6" title={`${made.name} was created and is ${made.status.charAt(0) + made.status.slice(1).toLowerCase()}.`} returned="On AWS a new table is Creating for a while; here it is ready at once." onDismiss={() => go({})} testId="created-banner" />}
      {gone && <ResultBanner className="mb-6" title={`${gone}${wentWith !== null ? ` and its ${wentWith} ${wentWith === 1 ? "item" : "items"}` : ""} ${wentWith !== null ? "are" : "is"} gone.`} returned="On AWS the table stays in the list as Deleting for a while; here it is removed at once." onDismiss={() => go({})} testId="deleted-banner" />}
      {list.isError && !first ? (
        <ErrorBlock what="The tables could not be listed" error={list.error} hint={HINT} onRetry={() => void list.refetch()} retrying={list.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading">
          <Skeleton className="h-9 w-80" />
          <Skeleton className="h-64 rounded-xl" />
        </div>
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={TableIcon} title="No tables" action={<Button variant="outline" className="bg-card" onClick={() => go({ do: "create" })}>Create table</Button>}>
            You have no tables in this region. Create a table to start storing items.
          </EmptyState>
          <FromYourApp service="dynamodb" />
        </div>
      ) : (
        <>
          {/* Just after a table was made the page says that and nothing else about the tables (the board's). */}
          {!made && (
            <p className="mb-5 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="list-sentence">
              <Emphasis text={listSentence(rows, region, one ?? null)} names={rows.map((r) => r.name)} bold={[/\d+ tables?/, /\d[\d,]* items?/]} />
            </p>
          )}
          <SelectableTable
            aria-label="Tables"
            data={rows}
            columns={COLUMNS}
            rowId={(r) => r.name}
            selected={selected}
            single
            hint="Choose a table to delete it."
            onSelectedChange={setSelected}
            noun="table"
            maxHeight={620}
          />
          <Pagination shown={rows.length} total={rows.length} more={list.hasNextPage} onMore={() => void list.fetchNextPage()} loading={list.isFetchingNextPage} noun={rows.length === 1 ? "table" : "tables"} className="mt-3" />
        </>
      )}
      {deleting && <TableDelete name={deleting} open onOpenChange={(o) => !o && setDeleting(null)} onDone={(items) => (setDeleting(null), setSelected(new Set()), go({ dialog: "deleted", name: deleting, ...(items !== null ? { sel: `n${items}` } : {}) }))} />}
    </div>
  );
}

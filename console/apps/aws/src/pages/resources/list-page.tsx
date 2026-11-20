import { Link } from "@tanstack/react-router";
import { PlusIcon, SearchIcon, SearchXIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { number } from "@doze/ui/lib/format";
import { cn } from "@doze/ui/lib/utils";
import { useCatalogue, useResourceList } from "../../api/resources";
import type { ResourceList } from "../../api/types/ResourceList";
import type { ResourceRow } from "../../api/types/ResourceRow";
import { CreateDialog } from "./create-dialog";
import { HINT } from "./index-page";
import { iconOf, sortKey } from "./model";
import { ResourceTo, Value } from "./value";

function columnsOf(page: ResourceList): DataColumn<ResourceRow>[] {
  const name: DataColumn<ResourceRow> = {
    id: "name",
    header: "Name",
    accessorFn: (r) => r.label.toLowerCase(),
    meta: { mono: true },
    cell: (c) => (
      <ResourceTo to={{ service: page.service, kind: page.kind, name: c.row.original.name }} className="font-mono text-[13px] font-medium" testId="row-link">
        <span title={c.row.original.id}>{c.row.original.label}</span>
      </ResourceTo>
    ),
  };
  return [
    name,
    ...page.columns.map((col, i): DataColumn<ResourceRow> => {
      const numeric = col.format === "number" || col.format === "bytes";
      return {
        id: `c${i}`,
        header: col.title,
        size: col.format === "time" ? 220 : numeric ? Math.max(130, col.title.length * 9 + 40) : 180,
        accessorFn: (r) => sortKey(r.cells[i] ?? null, col.format),
        meta: { numeric, mono: col.format === "mono" },
        sortDescFirst: numeric,
        cell: (c) => <Value value={c.row.original.cells[i] ?? null} format={col.format} />,
      };
    }),
  ];
}

/** The other kinds of the same service (SNS's topics and subscriptions), as tabs. */
function KindTabs({ service, kind }: { service: string; kind: string }) {
  const kinds = useCatalogue().data?.services.find((s) => s.name === service)?.kinds ?? [];
  if (kinds.length < 2) return null;
  return (
    <nav aria-label="Kinds" className="mb-6 flex gap-8 border-b">
      {kinds.map((k) => (
        <Link key={k.id} to="/resources/$service/$second" params={{ service, second: k.id }} search={{}} aria-current={k.id === kind ? "page" : undefined} className={cn("-mb-px border-b-2 px-0.5 pt-3 pb-3 text-[14px] whitespace-nowrap transition-colors focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none", k.id === kind ? "border-primary font-medium text-bright" : "border-transparent text-muted-foreground hover:text-foreground")}>
          {k.title}
        </Link>
      ))}
    </nav>
  );
}

/** A kind of resource, listed by the service's own list operation (one page at a time; the next on request), with a search over
 * what is shown, sortable columns, and Create when the descriptor has a form. */
export function ResourceListPage({ service, kind }: { service: string; kind: string }) {
  const list = useResourceList(service, kind);
  const [query, setQuery] = useState("");
  const [creating, setCreating] = useState(false);
  const { refetch } = list;
  const refresh = useCallback(() => void refetch(), [refetch]);
  const first = list.data?.pages[0];
  const rows = useMemo(() => list.data?.pages.flatMap((p) => p.rows) ?? [], [list.data]);
  const columns = useMemo(() => (first ? columnsOf(first) : []), [first]);
  const q = query.trim().toLowerCase();
  const shown = q ? rows.filter((r) => r.label.toLowerCase().includes(q) || r.name.toLowerCase().includes(q)) : rows;
  const Icon = iconOf(service);
  const title = first?.title ?? kind;
  const crumbs = [{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.service_title, to: `/resources/${service}` }] : [])];
  return (
    <>
      <PageHeader
        title={title}
        crumbs={crumbs}
        sub={first ? <span className="text-foreground">{first.about}</span> : undefined}
        refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={refresh} interval={false} />}
        actions={
          first?.create && (
            <Button onClick={() => setCreating(true)} data-testid="create-open">
              <PlusIcon /> Create {first.one}
            </Button>
          )
        }
      />
      <KindTabs service={service} kind={kind} />
      {list.isError && !first ? (
        <ErrorBlock what={`The ${title.toLowerCase()} could not be listed`} error={list.error} hint={HINT} onRetry={refresh} retrying={list.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading">
          <Skeleton className="h-9 w-80" />
          <Skeleton className="h-64 rounded-xl" />
        </div>
      ) : rows.length === 0 ? (
        <div data-testid="list-empty">
          <EmptyState
            icon={Icon}
            title={`No ${first.title.toLowerCase()} yet`}
            action={
              first.create && (
                <Button onClick={() => setCreating(true)}>
                  <PlusIcon /> Create {first.one}
                </Button>
              )
            }
          >
            {first.operation} answered with none. {first.create ? `Create one here, or with your app or the AWS CLI.` : "Create one with your app, a deploy tool or the AWS CLI."}
          </EmptyState>
        </div>
      ) : (
        <>
          <div className="mb-4 flex flex-wrap items-center gap-3">
            <div className="relative w-full max-w-[22rem]">
              <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={`Search ${first.title.toLowerCase()} by name`} aria-label={`Search ${first.title.toLowerCase()} by name`} className="bg-card pl-9" data-testid="list-search" />
            </div>
            <span className="text-[13px] text-muted-foreground tabular-nums" data-testid="list-count">
              {q ? `${number(shown.length)} of ${number(rows.length)}` : `${number(rows.length)} ${rows.length === 1 ? first.one : first.title.toLowerCase()}`}
              {list.hasNextPage ? ", more to load" : ""}
            </span>
          </div>
          <DataTable
            aria-label={first.title}
            data={shown}
            columns={columns}
            rowId={(r) => r.name}
            maxHeight={680}
            empty={
              <span className="inline-flex items-center gap-2 text-muted-foreground">
                <SearchXIcon aria-hidden className="size-4" /> No {first.title.toLowerCase()} match “{query}”.
              </span>
            }
          />
          {list.hasNextPage && (
            <div className="mt-4 flex justify-end">
              <Button variant="outline" className="bg-card" onClick={() => void list.fetchNextPage()} disabled={list.isFetchingNextPage} data-testid="list-more">
                {list.isFetchingNextPage ? "Loading…" : "Load the next page"}
              </Button>
            </div>
          )}
        </>
      )}
      {first?.create && <CreateDialog open={creating} onOpenChange={setCreating} service={service} kind={kind} one={first.one} operation={first.create.operations.join(", ")} inputs={first.create.inputs} />}
    </>
  );
}


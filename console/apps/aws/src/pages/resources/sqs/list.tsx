// SQS's queues: the list (sqs__queues-list, queues-empty, queues-none, and the list a deletion lands on, queue-deleted), searched
// by name prefix as ListQueues does (QueueNamePrefix: a prefix, case-sensitive).
import { useNavigate, useSearch } from "@tanstack/react-router";
import { PlusIcon, RowsIcon, SearchIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import type { ResourceList } from "../../../api/types/ResourceList";
import { HINT } from "../index-page";
import { ResourceTo, Value } from "../value";
import { SqsCreate } from "./create";
import { deletedWords, listRows, type QueueRow } from "./list-model";
import { remainWords } from "./queue-model";

const COLUMNS: DataColumn<QueueRow>[] = [
  { id: "name", header: "Name", accessorFn: (r) => r.name.toLowerCase(), meta: { mono: true }, cell: (c) => <ResourceTo to={{ service: "sqs", kind: "queues", name: c.row.original.name }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
  { id: "type", header: "Type", accessorKey: "type", size: 90, enableSorting: false },
  { id: "created", header: "Created", accessorFn: (r) => r.createdMs ?? 0, size: 190, min: 170, cell: (c) => <Value value={c.row.original.created} format="time" /> },
  { id: "available", header: "Messages available", accessorKey: "available", size: 190, meta: { numeric: true } },
  { id: "inFlight", header: "Messages in flight", accessorKey: "inFlight", size: 190, meta: { numeric: true } },
  { id: "encryption", header: "Encryption", accessorFn: (r) => (r.encrypted ? 1 : 0), size: 120, enableSorting: false, cell: (c) => (c.row.original.encrypted ? "Enabled" : <span className="text-muted-foreground">–</span>) },
  { id: "dedup", header: "Content-based deduplication", accessorFn: (r) => (r.dedup ? 1 : 0), size: 270, enableSorting: false, cell: (c) => (c.row.original.dedup ? "Enabled" : <span className="text-muted-foreground">–</span>) },
];

/** The queues' list page; with `?create=yes` the Create queue page in its place. */
export function SqsList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { q?: string; create?: string; gone?: string; from?: string };
  if (search.create === "yes") return <SqsCreate />;
  return <QueuesList service={service} kind={kind} q={search.q ?? ""} gone={search.gone} from={search.from} />;
}

function QueuesList({ service, kind, q, gone, from }: { service: string; kind: string; q: string; gone?: string; from?: string }) {
  const navigate = useNavigate();
  const about = useAbout().data;
  const list = useResourceList(service, kind, false, q ? { prefix: q } : {});
  const everything = useResourceList(service, kind);
  const [typed, setTyped] = useState(q);
  const { refetch } = list;
  const refresh = useCallback(() => void refetch(), [refetch]);
  const first = list.data?.pages[0];
  const rows = useMemo(() => listRows(list.data?.pages.flatMap((p: ResourceList) => p.rows) ?? []), [list.data]);
  const total = everything.data?.pages.flatMap((p: ResourceList) => p.rows).length;
  const region = about?.region ?? "this Region";
  const go = (next: { q?: string; create?: "yes" }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: next });
  const create = <Button onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Create queue</Button>;
  const sentence = gone ? remainWords(first?.sentence ?? null) : first?.sentence ?? null;
  const orphan = from && gone && !rows.some((r: QueueRow) => r.dlq === from) ? `${from} no longer has a source queue.` : null;
  const none = rows.length === 0;
  return (
    <>
      <PageHeader
        title="Queues"
        crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.group, to: "/resources" }, { label: first.service_title, to: `/resources/${service}` }] : [])]}
        sub={`Standard and FIFO queues in ${region}.${none && !q ? "" : " Choose a name to open it."}`}
        refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={refresh} interval={false} />}
        actions={create}
      />
      {gone && <ResultBanner className="mb-6" title={`Queue ${gone} deleted.`} returned={deletedWords()} onDismiss={() => go({})} testId="deleted-banner" />}
      {list.isError && !first ? (
        <ErrorBlock what="The queues could not be listed" error={list.error} hint={HINT} onRetry={refresh} retrying={list.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : none && !q ? (
        <div data-testid="list-empty">
          <EmptyState icon={RowsIcon} title="No queues" action={<Button onClick={() => go({ create: "yes" })}><PlusIcon /> Create queue</Button>}>
            There are no queues in {region}. Create a queue to start sending and receiving messages.
          </EmptyState>
        </div>
      ) : (
        <>
          {(sentence || orphan) && <p className="mb-5 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="list-sentence">{[sentence, orphan].filter(Boolean).join(" ")}</p>}
          <form className="mb-4 flex flex-wrap items-center gap-3" role="search" onSubmit={(e) => { e.preventDefault(); go(typed.trim() ? { q: typed.trim() } : {}); }}>
            <div className="relative w-full max-w-[22rem]">
              <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search queues by name prefix" aria-label="Search queues by name prefix" className="bg-card pl-9 font-mono text-[13px]" data-testid="list-search" />
            </div>
            <Button type="submit" variant="outline" className="bg-card" data-testid="list-search-go">Search</Button>
            {q && <Button type="button" variant="ghost" onClick={() => { setTyped(""); go({}); }} data-testid="search-clear">Clear search</Button>}
          </form>
          {none ? (
            <div data-testid="list-no-match">
              <EmptyState icon={SearchIcon} title="ListQueues returned no queues" action={total ? <Button variant="outline" className="bg-card" onClick={() => { setTyped(""); go({}); }} data-testid="search-show-all">Show all {total} {total === 1 ? "queue" : "queues"}</Button> : undefined}>
                QueueNamePrefix <span className="font-mono text-foreground">“{q}”</span> returns only the queues whose name begins with that text, and the match is case-sensitive.
              </EmptyState>
            </div>
          ) : (
            <>
              <DataTable aria-label="Queues" data={rows} columns={COLUMNS} rowId={(r) => r.name} maxHeight={680} />
              <Pagination className="mt-3" shown={rows.length} total={rows.length} more={list.hasNextPage} onMore={() => void list.fetchNextPage()} loading={list.isFetchingNextPage} noun={rows.length === 1 ? "queue" : "queues"} />
            </>
          )}
        </>
      )}
    </>
  );
}


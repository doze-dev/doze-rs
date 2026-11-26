// Kinesis's data streams: the list (kinesis__streams-list, streams-none, and the list a deletion lands on, stream-deleted). A row is chosen with its
// checkbox for Delete. `?create=yes` is Create data stream, as a page.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { DatabaseIcon, PlusIcon, SearchIcon, TrashIcon } from "lucide-react";
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
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import { HINT } from "../index-page";
import { ResourceTo, Value } from "../value";
import { streamArn, useConsumerNames } from "./api";
import { KinesisCreate } from "./create";
import { DeleteStream, lastDeleted } from "./delete";
import { type StreamRow, count, list, listSentence, modeWords, statusWords, streamRows } from "./model";

const tone = (s: string | null) => (s === "ACTIVE" ? "ok" : s === "CREATING" || s === "UPDATING" ? "info" : "idle");

export function KinesisList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { create?: string; gone?: string };
  if (search.create === "yes") return <KinesisCreate />;
  return <Streams service={service} kind={kind} gone={search.gone} />;
}

function Streams({ service, kind, gone }: { service: string; kind: string; gone?: string }) {
  const navigate = useNavigate();
  const about = useAbout().data;
  const region = about?.region ?? "us-east-1";
  const lister = useResourceList(service, kind);
  const [typed, setTyped] = useState("");
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [asking, setAsking] = useState(false);
  const { refetch } = lister;
  const refresh = useCallback(() => void refetch(), [refetch]);
  const first = lister.data?.pages[0];
  const all = useMemo(() => streamRows(lister.data?.pages.flatMap((p) => p.rows) ?? []), [lister.data]);
  const rows = useMemo(() => all.filter((r) => r.name.toLowerCase().includes(typed.trim().toLowerCase())), [all, typed]);
  const fed = all.filter((r) => r.consumers > 0).slice(0, 2);
  const names = useConsumerNames(Object.fromEntries(fed.map((r) => [r.name, streamArn(region, about?.account ?? "000000000000", r.name)])));
  const picked = all.filter((r) => chosen.has(r.name));
  const go = (next: { create?: "yes"; gone?: string }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: next });
  const columns: DataColumn<StreamRow>[] = [
    { id: "name", header: "Name", min: 200, accessorFn: (r) => r.name.toLowerCase(), meta: { mono: true }, cell: (c) => <ResourceTo to={{ service, kind, name: c.row.original.name }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
    { id: "status", header: "Status", size: 150, accessorFn: (r) => r.status ?? "", cell: (c) => <StatusDot tone={tone(c.row.original.status)}>{statusWords(c.row.original.status)}</StatusDot> },
    { id: "mode", header: "Capacity mode", size: 190, accessorFn: (r) => r.mode ?? "", cell: (c) => modeWords(c.row.original.mode) },
    { id: "created", header: "Created", size: 200, accessorFn: (r) => r.created ?? "", cell: (c) => <Value value={c.row.original.created} format="time" /> },
  ];
  const none = all.length === 0;
  const left = gone ? lastDeleted(gone) : null;
  const sentence = gone && !none ? `${count(all.length, "data stream")} ${all.length === 1 ? "remains" : "remain"}. Anything still writing to ${gone} now gets ResourceNotFoundException.` : listSentence(all, names);
  return (
    <>
      {gone && <ResultBanner className="mb-8" title="Stream deleted." returned={<>Deleted <b className="font-mono font-semibold text-bright">{gone}</b>. Its shards, its records and its tags are gone{left && left.consumers.length > 0 ? <>, and the {left.consumers.length === 1 ? "consumer" : "consumers"} <b className="font-mono font-semibold text-bright">{list(left.consumers)}</b> {left.consumers.length === 1 ? "was" : "were"} deregistered with it</> : null}.</>} onDismiss={() => go({})} testId="deleted-banner" />}
      <PageHeader
        title="Data streams"
        crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.service_title, to: `/resources/${service}` }] : [])]}
        sub={`Kinesis data streams in ${region}.${none ? "" : " Choose a name to open the stream."}`}
        refresh={<RefreshControl updatedAt={lister.dataUpdatedAt} fetching={lister.isFetching} onRefresh={refresh} interval={false} />}
        actions={<Button onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Create data stream</Button>}
      />
      {lister.isError && !first ? (
        <ErrorBlock what="The data streams could not be listed" error={lister.error} hint={HINT} onRetry={refresh} retrying={lister.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={DatabaseIcon} title="No data streams" action={<Button onClick={() => go({ create: "yes" })}><PlusIcon /> Create data stream</Button>}>
            Create a data stream to start ingesting records.
          </EmptyState>
        </div>
      ) : (
        <>
          {sentence && <p className="mb-6 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="list-sentence"><Emphasis text={sentence} names={[...all.map((r) => r.name), ...Object.values(names).flat()]} /></p>}
          <SelectableTable<StreamRow>
            aria-label="Data streams"
            data={rows}
            columns={columns}
            rowId={(r) => r.name}
            selected={chosen}
            onSelectedChange={setChosen}
            noun="data stream"
            maxHeight={680}
            actionsAlways
            hint=" "
            toolbar={
              <form className="flex flex-wrap items-center gap-3" role="search" onSubmit={(e) => e.preventDefault()}>
                <div className="relative w-[22rem] max-w-full">
                  <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                  <Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search data streams by name" aria-label="Search data streams by name" className="bg-card pl-9" data-testid="list-search" />
                </div>
                <Button type="submit" variant="outline" className="bg-card">Search</Button>
              </form>
            }
            actions={(sel) => (
              <>
                <span className="text-[13.5px] text-muted-foreground">Actions</span>
                <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={sel.length !== 1} title={sel.length > 1 ? "Delete acts on one stream at a time." : undefined} onClick={() => setAsking(true)} data-testid="action-delete"><TrashIcon /> Delete</Button>
              </>
            )}
            empty={`No data stream's name contains “${typed}”.`}
          />
          <Pagination className="mt-3" shown={rows.length} total={rows.length} more={lister.hasNextPage} onMore={() => void lister.fetchNextPage()} loading={lister.isFetchingNextPage} noun={rows.length === 1 ? "data stream" : "data streams"} />
        </>
      )}
      {picked[0] && <DeleteStream open={asking} onOpenChange={setAsking} name={picked[0].name} onDone={() => { setAsking(false); setChosen(new Set()); go({ gone: picked[0]?.name }); }} />}
    </>
  );
}

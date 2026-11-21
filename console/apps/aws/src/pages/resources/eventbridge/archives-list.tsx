// EventBridge's archives (eventbridge__archives-list, archives-none, and the delete from the list): rows chosen for Replay and Delete.
// `?create=yes` is Create archive (`?name=<bus>` its source); `?gone=<name>` a deleted one.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { ArchiveIcon, PlusIcon, SearchIcon, TrashIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import type { DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { bytes, number } from "@doze/ui/lib/format";
import { PlainConfirm, RuleHelp, useDeleteClearing, useServiceCrumbs } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { useArchives } from "./api";
import { ArchiveCreate } from "./archive-create";
import { Said, State } from "./parts";
import { SectionStrip } from "./strip";
import type { ArchiveItem } from "./types";
import { archivesSentence, busOfArn, retentionWords } from "./words";

export function ArchivesList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { create?: string; name?: string; gone?: string };
  if (search.create === "yes") return <ArchiveCreate bus={search.name} />;
  return <Archives service={service} kind={kind} gone={search.gone} />;
}

function Archives({ service, kind, gone }: { service: string; kind: string; gone?: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs(service);
  const q = useArchives();
  const [typed, setTyped] = useState("");
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [asking, setAsking] = useState(false);
  const all = q.data?.Archives ?? [];
  const rows = useMemo(() => all.filter((a) => a.ArchiveName.startsWith(typed.trim())), [all, typed]);
  const picked = all.filter((a) => chosen.has(a.ArchiveName));
  const one = picked[0];
  const del = useDeleteClearing({ service, kind, name: one?.ArchiveName ?? "" });
  const go = (search: { create?: "yes"; name?: string; gone?: string }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search });
  const columns: DataColumn<ArchiveItem>[] = [
    { id: "name", header: "Name", min: 180, accessorFn: (a) => a.ArchiveName, cell: (c) => <ResourceTo to={{ service, kind, name: c.row.original.ArchiveName }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.ArchiveName}</ResourceTo> },
    { id: "source", header: "Source", size: 170, accessorFn: (a) => busOfArn(a.EventSourceArn), cell: (c) => <ResourceTo to={{ service, kind: "buses", name: busOfArn(c.row.original.EventSourceArn) }} className="font-mono text-[13px]">{busOfArn(c.row.original.EventSourceArn)}</ResourceTo> },
    { id: "state", header: "State", size: 130, accessorFn: (a) => a.State ?? "", cell: (c) => <State state={c.row.original.State} /> },
    { id: "retention", header: "Retention period", size: 170, accessorFn: (a) => a.RetentionDays ?? 0, cell: (c) => retentionWords(c.row.original.RetentionDays) },
    { id: "events", header: "Events", size: 120, accessorFn: (a) => a.EventCount ?? 0, meta: { numeric: true }, cell: (c) => number(c.row.original.EventCount ?? 0) },
    { id: "size", header: "Size", size: 110, accessorFn: (a) => a.SizeBytes ?? 0, meta: { numeric: true }, cell: (c) => bytes(c.row.original.SizeBytes ?? 0) },
  ];
  return (
    <>
      {gone && <ResultBanner className="mb-8" title="Archive deleted." returned={<>Deleted <b className="font-mono font-semibold text-bright">{gone}</b> with the events in it and its pattern. They can no longer be replayed.</>} onDismiss={() => go({})} testId="deleted-banner" />}
      <PageHeader
        title="Archives"
        crumbs={[...crumbs]}
        belowCrumbs={<SectionStrip current="archives" />}
        sub={<><b className="font-semibold text-foreground">Events kept so you can send them again.</b> Each archive takes events from one bus, optionally filtered by a pattern.</>}
        refresh={<RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={() => void q.refetch()} interval={false} />}
        actions={<Button onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Create archive</Button>}
      />
      {q.isError ? (
        <ErrorBlock what="The archives could not be listed" error={q.error} hint={HINT} onRetry={() => void q.refetch()} retrying={q.isFetching} testId="list-error" />
      ) : !q.data ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : all.length === 0 ? (
        <div data-testid="list-empty">
          <EmptyState icon={ArchiveIcon} title="No archives" action={<Button onClick={() => go({ create: "yes" })}><PlusIcon /> Create archive</Button>}>An archive stores the events that match a pattern so you can replay them later.</EmptyState>
        </div>
      ) : (
        <>
          <Said text={archivesSentence(all)} names={[...new Set(all.flatMap((a) => [a.ArchiveName, busOfArn(a.EventSourceArn)]))]} bold={[/\b\d[\d,]*(?= (?:archives?|events?|days?)\b)/]} />
          <SelectableTable<ArchiveItem>
            aria-label="Archives"
            data={rows}
            columns={columns}
            rowId={(a) => a.ArchiveName}
            selected={chosen}
            onSelectedChange={setChosen}
            noun="archive"
            maxHeight={680}
            actionsAlways
            hint=" "
            toolbar={
              <form className="flex flex-wrap items-center gap-3" role="search" onSubmit={(e) => e.preventDefault()}>
                <div className="relative w-[22rem] max-w-full"><SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" /><Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search by name prefix" aria-label="Search by name prefix" className="bg-card pl-9" data-testid="list-search" /></div>
                <Button type="submit" variant="outline" className="bg-card">Search</Button>
              </form>
            }
            actions={(sel) => (
              <>
                <Button type="button" variant="outline" className="bg-card" disabled={sel.length !== 1} title={sel.length > 1 ? "A replay starts from one archive." : undefined} onClick={() => void navigate({ to: "/resources/$service/$second", params: { service, second: "replays" }, search: { create: "yes", name: one?.ArchiveName } })} data-testid="action-replay">Replay</Button>
                <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={sel.length !== 1} title={sel.length > 1 ? "Delete acts on one archive at a time." : undefined} onClick={() => setAsking(true)} data-testid="action-delete"><TrashIcon /> Delete</Button>
              </>
            )}
            empty={`No archive's name begins with “${typed}”.`}
          />
          <Pagination className="mt-3" shown={rows.length} total={rows.length} noun={rows.length === 1 ? "archive" : "archives"} />
          <p className="mt-4 max-w-3xl text-[13px] text-muted-foreground">Columns follow the fields ListArchives returns: ArchiveName, EventSourceArn, State, RetentionDays, EventCount and SizeBytes.</p>
          <RuleHelp className="mt-4 max-w-3xl">On AWS, event counts and sizes are reconciled about every 24 hours and events can take a few minutes to reach an archive, so wait about 10 minutes before replaying recent ones. Here the counts follow each event as it is archived, and <b>retention is applied</b>: events older than it are removed when the next event arrives, at most once an hour.</RuleHelp>
          <RuleHelp className="mt-4 max-w-3xl">Deleting an archive deletes its events and its pattern. Deleted the last archive? The list then shows its empty state.</RuleHelp>
        </>
      )}
      {one && (
        <PlainConfirm open={asking} onOpenChange={(o) => { setAsking(o); if (!o) del.reset(); }} title="Delete archive?" description={<>Delete <b className="font-mono font-semibold text-bright">{one.ArchiveName}</b>, which holds {number(one.EventCount ?? 0)} {one.EventCount === 1 ? "event" : "events"} from <b className="font-mono font-semibold text-bright">{busOfArn(one.EventSourceArn)}</b>? Its events and its pattern are deleted with it, and they can no longer be replayed.</>} call="DeleteArchive" pending={del.isPending} error={del.error} onConfirm={() => del.mutate({ clear: false }, { onSuccess: () => { setAsking(false); setChosen(new Set()); go({ gone: one.ArchiveName }); } })} testId="archive-delete" />
      )}
    </>
  );
}

// EventBridge's event buses (eventbridge__buses-list, send-events): the default bus as a card with its Actions, the custom buses as a table with
// the ARN under each name. `?create=yes` is Create event bus, `?do=send&name=<bus>` is Send events.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useQueries } from "@tanstack/react-query";
import { ChevronDownIcon, PlusIcon, SearchIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { RuleHelp, callKindOp, useServiceCrumbs } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { RULES, useArchives, useBuses } from "./api";
import { BusCreate } from "./bus-create";
import { Heading, Said, when } from "./parts";
import { SendEvents } from "./send-events";
import { SectionStrip } from "./strip";
import type { BusItem, Rules } from "./types";
import { busesSentence } from "./words";

export function BusesList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { create?: string; do?: string; name?: string; gone?: string };
  if (search.create === "yes") return <BusCreate />;
  if (search.do === "send") return <SendEvents bus={search.name ?? "default"} />;
  return <Buses service={service} kind={kind} gone={search.gone} />;
}

function Buses({ service, kind, gone }: { service: string; kind: string; gone?: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs(service);
  const buses = useBuses();
  const archives = useArchives();
  const [typed, setTyped] = useState("");
  const all = useMemo(() => buses.data?.EventBuses ?? [], [buses.data]);
  const names = all.map((b) => b.Name);
  const reads = useQueries({ queries: names.map((n) => ({ queryKey: ["resources", "kindop", "eventbridge", "rules", "ListRules", n, "page"], queryFn: () => callKindOp<Rules>(RULES, "ListRules", { EventBusName: n, Limit: 100 }), retry: false })) });
  const rulesBy = Object.fromEntries(names.flatMap((n, i) => (reads[i]?.data ? [[n, reads[i]?.data?.Rules ?? []] as const] : [])));
  const def = all.find((b) => b.Name === "default");
  const custom = useMemo(() => all.filter((b) => b.Name !== "default" && b.Name.startsWith(typed.trim())).sort((a, b) => a.Name.localeCompare(b.Name)), [all, typed]);
  const go = (search: { create?: "yes"; do?: string; name?: string }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search });
  const refresh = () => {
    void buses.refetch();
    void archives.refetch();
    reads.forEach((r) => void r.refetch());
  };
  const columns: DataColumn<BusItem>[] = [
    { id: "name", header: "Name", min: 240, accessorFn: (b) => b.Name, cell: (c) => <div className="grid min-w-0 leading-5"><ResourceTo to={{ service, kind, name: c.row.original.Name }} className="truncate font-mono text-[13.5px] font-medium" testId="row-link">{c.row.original.Name}</ResourceTo><span className="truncate font-mono text-[11.5px] text-muted-foreground">{c.row.original.Arn}</span></div> },
    { id: "description", header: "Description", size: 280, accessorFn: (b) => b.Description ?? "", cell: (c) => c.row.original.Description ?? <span className="text-muted-foreground">–</span> },
    { id: "created", header: "Created", size: 200, accessorFn: (b) => b.CreationTime ?? 0, cell: (c) => when(c.row.original.CreationTime) },
    { id: "modified", header: "Last modified", size: 200, accessorFn: (b) => b.LastModifiedTime ?? 0, cell: (c) => when(c.row.original.LastModifiedTime) },
  ];
  return (
    <>
      {gone && <ResultBanner className="mb-8" title="Event bus deleted." returned={<>Deleted <b className="font-mono font-semibold text-bright">{gone}</b> with its rules and their targets. An app that keeps sending to it by name now gets ResourceNotFoundException from PutEvents.</>} onDismiss={() => go({})} testId="deleted-banner" />}
      <PageHeader
        title="Event buses"
        crumbs={[...crumbs]}
        belowCrumbs={<SectionStrip current="buses" />}
        sub={<><b className="font-semibold text-foreground">Where events arrive.</b> Every account has a default bus for events from AWS services. Create custom buses for your own apps, then write rules on them.</>}
        refresh={<RefreshControl updatedAt={buses.dataUpdatedAt} fetching={buses.isFetching} onRefresh={refresh} interval={false} />}
        actions={<Button onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Create event bus</Button>}
      />
      {buses.isError ? (
        <ErrorBlock what="The event buses could not be listed" error={buses.error} hint={HINT} onRetry={refresh} retrying={buses.isFetching} testId="list-error" />
      ) : !buses.data ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : (
        <>
          <Said text={busesSentence(names, rulesBy, archives.data?.Archives ?? [])} names={names} />
          <section className="mb-10 rounded-[var(--radius-card)] border bg-card p-6 lg:p-7" aria-labelledby="default-bus" data-testid="default-bus">
            <div className="flex items-start justify-between gap-4">
              <Heading title="Default event bus" id="default-bus">Receives events from AWS services in this account. It can't be deleted.</Heading>
              <DropdownMenu>
                <DropdownMenuTrigger asChild><Button variant="outline" className="bg-card" data-testid="default-actions">Actions <ChevronDownIcon /></Button></DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="min-w-48">
                  <DropdownMenuItem onSelect={() => go({ do: "send", name: "default" })} data-testid="action-send">Send events</DropdownMenuItem>
                  <DropdownMenuItem onSelect={() => void navigate({ to: "/resources/$service/$second", params: { service, second: "rules" }, search: { name: "default" } })} data-testid="action-rules">View rules</DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
            <dl className="mt-2 grid grid-cols-[minmax(7rem,12rem)_minmax(0,1fr)] border-t text-[13.5px] [&>div:last-child>*]:border-b-0">
              {[["Name", <ResourceTo key="n" to={{ service, kind, name: "default" }} className="font-mono">default</ResourceTo>], ["ARN", <span key="a" className="flex items-center gap-2 font-mono">{def?.Arn ?? "–"}{def && <CopyButton text={def.Arn} what="ARN copied" iconOnly variant="ghost" label="Copy ARN" />}</span>]].map(([l, v]) => (
                <div key={String(l)} className="contents"><dt className="border-b py-3.5 text-muted-foreground">{l}</dt><dd className="min-w-0 border-b py-3.5 break-all text-bright">{v}</dd></div>
              ))}
            </dl>
          </section>
          <Heading title="Custom event buses">Buses you created.</Heading>
          <form className="mb-4 flex flex-wrap items-center gap-3" role="search" onSubmit={(e) => e.preventDefault()}>
            <div className="relative w-[22rem] max-w-full">
              <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search by name prefix" aria-label="Search by name prefix" className="bg-card pl-9" data-testid="list-search" />
            </div>
            <Button type="submit" variant="outline" className="bg-card">Search</Button>
          </form>
          <DataTable<BusItem> aria-label="Custom event buses" data={custom} columns={columns} rowId={(b) => b.Name} rowHeight={58} empty={typed ? `No custom event bus begins with “${typed}”.` : "You have no custom event bus yet. Create one for your own app's events."} />
          <p className="mt-3 text-[13px] text-muted-foreground">Columns follow the fields ListEventBuses returns: Name, Description, CreationTime and LastModifiedTime.</p>
          <Pagination className="mt-3" shown={custom.length} total={custom.length} noun={custom.length === 1 ? "custom event bus" : "custom event buses"} />
          <RuleHelp className="mt-6 max-w-3xl">An account can have <b>100</b> event buses in a Region by default on AWS; the quota can be raised. This account has {all.length}, counting default.</RuleHelp>
        </>
      )}
    </>
  );
}


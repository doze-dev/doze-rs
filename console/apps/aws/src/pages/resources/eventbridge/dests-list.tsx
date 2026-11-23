// EventBridge's API destinations (eventbridge__api-destinations-list, api-destinations-none): the endpoint, the method, the connection and the rate limit.
// `?create=yes` is Create API destination, `?do=connection` Create connection.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { GlobeIcon, PlusIcon, SearchIcon, TrashIcon } from "lucide-react";
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
import { LocalStatus, PlainConfirm, RuleHelp, useDeleteClearing, useServiceCrumbs } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { useDests } from "./api";
import { ConnectionCreate } from "./connection-create";
import { DestCreate } from "./dest-create";
import { Breakable, Said, State } from "./parts";
import { SectionStrip } from "./strip";
import type { ApiDestItem } from "./types";
import { connectionName, destsSentence } from "./words";

export function DestsList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { create?: string; do?: string; name?: string; gone?: string };
  if (search.do === "connection") return <ConnectionCreate />;
  if (search.create === "yes") return <DestCreate connection={search.name} />;
  return <Dests service={service} kind={kind} gone={search.gone} />;
}

function Dests({ service, kind, gone }: { service: string; kind: string; gone?: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs(service);
  const q = useDests();
  const [typed, setTyped] = useState("");
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [asking, setAsking] = useState(false);
  const all = useMemo(() => q.data?.ApiDestinations ?? [], [q.data]);
  const rows = useMemo(() => all.filter((d) => d.Name.startsWith(typed.trim())), [all, typed]);
  const one = all.find((d) => chosen.has(d.Name));
  const del = useDeleteClearing({ service, kind, name: one?.Name ?? "" });
  const go = (search: { create?: "yes"; gone?: string }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search });
  const columns: DataColumn<ApiDestItem>[] = [
    { id: "name", header: "Name", min: 150, accessorFn: (d) => d.Name, cell: (c) => <ResourceTo to={{ service, kind, name: c.row.original.Name }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.Name}</ResourceTo> },
    { id: "state", header: "State", size: 120, accessorFn: (d) => d.ApiDestinationState ?? "", cell: (c) => <State state={c.row.original.ApiDestinationState} /> },
    { id: "endpoint", header: "API destination endpoint", size: 400, min: 260, accessorFn: (d) => d.InvocationEndpoint, meta: { mono: true }, cell: (c) => <span className="block text-[12.5px] leading-4 [overflow-wrap:anywhere] whitespace-normal" data-testid="endpoint"><Breakable text={c.row.original.InvocationEndpoint} /></span> },
    { id: "method", header: "HTTP method", size: 130, accessorFn: (d) => d.HttpMethod, cell: (c) => c.row.original.HttpMethod },
    { id: "connection", header: "Connection", size: 150, accessorFn: (d) => connectionName(d.ConnectionArn), meta: { mono: true }, cell: (c) => connectionName(c.row.original.ConnectionArn) },
    { id: "rate", header: "Rate limit per second", size: 175, accessorFn: (d) => d.InvocationRateLimitPerSecond ?? 300, meta: { numeric: true }, cell: (c) => String(c.row.original.InvocationRateLimitPerSecond ?? 300) },
  ];
  return (
    <>
      {gone && <ResultBanner className="mb-8" title="API destination deleted." returned={<>Deleted <b className="font-mono font-semibold text-bright">{gone}</b>. A rule that has it as a target keeps the target and can no longer deliver to it.</>} onDismiss={() => go({})} testId="deleted-banner" />}
      <PageHeader
        title="API destinations"
        crumbs={[...crumbs]}
        belowCrumbs={<SectionStrip current="api-destinations" />}
        sub={<><b className="font-semibold text-foreground">HTTPS endpoints a rule can send events to.</b> Each one uses a connection for its credentials.</>}
        refresh={<RefreshControl updatedAt={q.dataUpdatedAt} fetching={q.isFetching} onRefresh={() => void q.refetch()} interval={false} />}
        actions={<Button onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Create API destination</Button>}
      />
      {q.isError ? (
        <ErrorBlock what="The API destinations could not be listed" error={q.error} hint={HINT} onRetry={() => void q.refetch()} retrying={q.isFetching} testId="list-error" />
      ) : !q.data ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : all.length === 0 ? (
        <div data-testid="list-empty">
          <EmptyState icon={GlobeIcon} title="No API destinations" action={<Button onClick={() => go({ create: "yes" })}><PlusIcon /> Create API destination</Button>}>An API destination is an HTTP endpoint a rule can call, with the connection that holds its credentials.</EmptyState>
        </div>
      ) : (
        <>
          <Said text={destsSentence(all)} names={[...all.map((d) => d.Name), ...all.map((d) => connectionName(d.ConnectionArn))]} bold={[/\b\d+(?= API destinations?\b)/]} />
          <SelectableTable<ApiDestItem>
            aria-label="API destinations"
            data={rows}
            columns={columns}
            rowId={(d) => d.Name}
            selected={chosen}
            onSelectedChange={setChosen}
            noun="API destination"
            single
            rowHeight={52}
            maxHeight={680}
            actionsAlways
            hint=" "
            toolbar={
              <form className="flex flex-wrap items-center gap-3" role="search" onSubmit={(e) => e.preventDefault()}>
                <div className="relative w-[22rem] max-w-full"><SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" /><Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search by name prefix" aria-label="Search by name prefix" className="bg-card pl-9" data-testid="list-search" /></div>
                <Button type="submit" variant="outline" className="bg-card">Search</Button>
              </form>
            }
            actions={(sel) => <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={sel.length !== 1} onClick={() => setAsking(true)} data-testid="action-delete"><TrashIcon /> Delete</Button>}
            empty={`No API destination's name begins with “${typed}”.`}
          />
          <Pagination className="mt-3" shown={rows.length} total={rows.length} noun={rows.length === 1 ? "API destination" : "API destinations"} />
          <p className="mt-4 max-w-3xl text-[13px] text-muted-foreground">Columns follow the fields ListApiDestinations returns: Name, ApiDestinationState, InvocationEndpoint, HttpMethod, ConnectionArn and InvocationRateLimitPerSecond.</p>
          <RuleHelp className="mt-4 max-w-3xl">EventBridge gives the endpoint <b>5 seconds</b> to answer. A request that fails by a transport error, a 429 or a 5xx is retried under the target's retry policy, and what runs out goes to the target's dead-letter queue; any other status is the endpoint refusing the event and is not retried.</RuleHelp>
          <RuleHelp className="mt-4 max-w-3xl">An account can have <b>3,000</b> API destinations by default on AWS. The rate limit defaults to <b>300</b> invocations per second.</RuleHelp>
          <LocalStatus className="mt-3 max-w-3xl" state="stored" reason={<>The rate limit is kept and returned, not enforced.</>} />
        </>
      )}
      {one && (
        <PlainConfirm open={asking} onOpenChange={(o) => { setAsking(o); if (!o) del.reset(); }} title="Delete API destination?" description={<>Delete <b className="font-mono font-semibold text-bright">{one.Name}</b>? A rule that has it as a target keeps the target and can no longer deliver to it. Its connection <b className="font-mono font-semibold text-bright">{connectionName(one.ConnectionArn)}</b> stays.</>} call="DeleteApiDestination" pending={del.isPending} error={del.error} onConfirm={() => del.mutate({ clear: false }, { onSuccess: () => { setAsking(false); setChosen(new Set()); go({ gone: one.Name }); } })} testId="dest-delete" />
      )}
    </>
  );
}

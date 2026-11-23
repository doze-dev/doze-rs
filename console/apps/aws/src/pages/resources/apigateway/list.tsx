// API Gateway's APIs (apigateway__apis-list, apis-none): REST and HTTP APIs as one list, with the sentence over it, the account's
// navigation, a search, Actions (Delete) over the rows chosen, and the footer count. `?do=create` is the type chooser.
import { useQueries } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { ChevronDownIcon, NetworkIcon, PlusIcon, SearchIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { callKindOp, useServiceCrumbs } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { HTTP, REST, useHttpApis, useRestApis } from "./api";
import { ApiDelete } from "./api-delete";
import { CreateType } from "./create-type";
import { lazy, Suspense } from "react";
import { AccountNav, Said, useWhere } from "./parts";
import type { Deployments, HttpStages, Stages } from "./types";
import type { AnyApi } from "./types";
import { type Answering, anyApis, apiKind, apisSentence, when } from "./words";

/** The stages each API answers at, for the sentence: a REST API's with the time of the deployment, an HTTP API's as they are. */
function useAnswering(apis: readonly AnyApi[]): Record<string, Answering[]> {
  const reads = useQueries({
    queries: apis.map((a) => ({
      queryKey: ["resources", "kindop", "apigateway", "answering", a.id],
      retry: false,
      queryFn: async (): Promise<Answering[]> => {
        if (a.protocol === "REST") {
          const [s, d] = await Promise.all([callKindOp<Stages>(REST, "GetStages", { restApiId: a.id }), callKindOp<Deployments>(REST, "GetDeployments", { restApiId: a.id, limit: 500 })]);
          return (s.item ?? []).map((x) => ({ stage: x.stageName, auto: false, deployed: when(d.item?.find((y) => y.id === x.deploymentId)?.createdDate ?? x.createdDate) }));
        }
        const s = await callKindOp<HttpStages>(HTTP, "GetStages", { ApiId: a.id });
        return (s.items ?? []).map((x) => ({ stage: x.stageName, auto: Boolean(x.autoDeploy), deployed: null }));
      },
    })),
  });
  return Object.fromEntries(apis.flatMap((a, i) => (reads[i]?.data ? [[a.id, reads[i].data] as const] : [])));
}

const FORMS: Record<string, React.LazyExoticComponent<() => React.ReactNode>> = {
  "create-rest": lazy(() => import("./rest-create").then((m) => ({ default: m.RestCreate }))),
  "create-http": lazy(() => import("./http-wizard").then((m) => ({ default: m.HttpWizard }))),
  "create-websocket": lazy(() => import("./ws-create").then((m) => ({ default: m.WsCreate }))),
};

export function ApisList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { do?: string; gone?: string };
  if (search.do === "create") return <CreateType />;
  const Form = search.do ? FORMS[search.do] : undefined;
  if (Form) return <Suspense fallback={<Skeleton className="h-96 rounded-[14px]" />}><Form /></Suspense>;
  return <Apis service={service} kind={kind} gone={search.gone} />;
}

function Apis({ service, kind, gone }: { service: string; kind: string; gone?: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs(service);
  const { region } = useWhere();
  const rest = useRestApis();
  const http = useHttpApis();
  const [typed, setTyped] = useState("");
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const [deleting, setDeleting] = useState<AnyApi | null>(null);
  const all = useMemo(() => anyApis(rest.data?.item ?? [], http.data?.items ?? []), [rest.data, http.data]);
  const shown = useMemo(() => all.filter((a) => a.name.toLowerCase().includes(typed.trim().toLowerCase())), [all, typed]);
  const answering = useAnswering(all);
  const failed = rest.isError ? rest : http.isError ? http : null;
  const go = (search: { do?: string; gone?: string }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search });
  const refresh = () => {
    void rest.refetch();
    void http.refetch();
  };
  const columns: DataColumn<AnyApi>[] = [
    { id: "name", header: "Name", min: 200, accessorFn: (a) => a.name, cell: (c) => <ResourceTo to={{ service, kind: apiKind(c.row.original.protocol), name: c.row.original.id }} className="font-mono text-[13.5px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
    { id: "description", header: "Description", min: 220, accessorFn: (a) => a.description, cell: (c) => c.row.original.description ? <span className="block whitespace-normal">{c.row.original.description}</span> : <span className="text-muted-foreground">–</span> },
    { id: "id", header: "API ID", size: 150, accessorFn: (a) => a.id, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.id}</span> },
    { id: "protocol", header: "Protocol type", size: 150, accessorFn: (a) => a.protocol, cell: (c) => <Badge variant="secondary" className="font-normal">{c.row.original.protocol}</Badge> },
    { id: "endpoint", header: "API endpoint type", size: 170, accessorFn: (a) => a.endpointType },
    { id: "created", header: "Created date", size: 200, accessorFn: (a) => a.created ?? 0, cell: (c) => when(c.row.original.created === null ? undefined : c.row.original.created / 1000) },
  ];
  const picked = all.filter((a) => chosen.has(a.id));
  const create = (
    <Button onClick={() => go({ do: "create" })} data-testid="create-open"><PlusIcon /> Create API</Button>
  );
  return (
    <>
      {gone && <ResultBanner className="mb-8" title="API deleted." returned={<>Deleted <b className="font-mono font-semibold text-bright">{gone}</b>. Its invoke URL no longer answers.</>} onDismiss={() => go({})} testId="deleted-banner" />}
      <PageHeader
        title="APIs"
        crumbs={[...crumbs]}
        sub={<>REST and HTTP APIs in {region}.{all.length > 0 && " Choose a name to open the API, or select one to delete it."}</>}
        refresh={<RefreshControl updatedAt={Math.max(rest.dataUpdatedAt, http.dataUpdatedAt)} fetching={rest.isFetching || http.isFetching} onRefresh={refresh} interval={false} />}
        actions={create}
      />
      {failed ? (
        <ErrorBlock what="The APIs could not be listed" error={failed.error} hint={HINT} onRetry={refresh} retrying={failed.isFetching} testId="list-error" />
      ) : !rest.data || !http.data ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : all.length === 0 ? (
        <>
          <AccountNav current="apis" />
          <EmptyState icon={NetworkIcon} title="No APIs" action={<Button variant="outline" className="bg-card" onClick={() => go({ do: "create" })} data-testid="create-empty"><PlusIcon /> Create API</Button>}>You have no APIs in this region. Create a REST or HTTP API to expose a backend.</EmptyState>
        </>
      ) : (
        <>
          <Said text={apisSentence(all, answering)} names={all.map((a) => a.name)} bold={[/(?<=its )[\w$-]+(?= stage)/]} />
          <AccountNav current="apis" />
          <SelectableTable<AnyApi>
            aria-label="APIs"
            data={shown}
            columns={columns}
            rowId={(a) => a.id}
            selected={chosen}
            onSelectedChange={(s) => setChosen(s)}
            noun="API"
            rowHeight={56}
            empty={`No API is named like “${typed}”.`}
            toolbar={
              <form className="flex flex-wrap items-center gap-3" role="search" onSubmit={(e) => e.preventDefault()}>
                <div className="relative w-[20rem] max-w-full">
                  <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                  <Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search APIs" aria-label="Search APIs" className="bg-card pl-9" data-testid="list-search" />
                </div>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild><Button type="button" variant="outline" className="bg-card" data-testid="list-actions">Actions <ChevronDownIcon /></Button></DropdownMenuTrigger>
                  <DropdownMenuContent align="start" className="min-w-48">
                    <DropdownMenuItem disabled={picked.length !== 1} onSelect={() => picked[0] && setDeleting(picked[0])} data-testid="action-delete">Delete{picked.length === 0 ? " (choose an API)" : picked.length > 1 ? " (choose one API)" : ""}</DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </form>
            }
          />
          <Pagination className="mt-3" shown={shown.length} total={shown.length} noun={shown.length === 1 ? "API" : "APIs"} />
        </>
      )}
      {deleting && <ApiDelete api={deleting} open onOpenChange={(o) => !o && setDeleting(null)} onDone={(name) => { setDeleting(null); setChosen(new Set()); go({ gone: name }); }} />}
    </>
  );
}

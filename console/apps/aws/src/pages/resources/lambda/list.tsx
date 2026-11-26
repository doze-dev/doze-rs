// Lambda's list of functions (lambda__functions-list, functions-none, function-deleted): the functions with their description,
// package type, runtime and when they were last modified, a sentence of what ran in the last hour, and Create Event Function.
import { useNavigate, useSearch, Link } from "@tanstack/react-router";
import { PlusIcon, SearchIcon, ZapIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { Callout } from "@doze/ui/components/app/callout";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Emphasis } from "@doze/ui/components/app/emphasis";
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
import { type MetricQuery, useMetricData } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo, Value } from "../value";
import { FunctionCreate } from "./create";
import { leftWords, readLeft, recalledLeft } from "./delete-model";
import { type FunctionRow, invokedSentence, rowsOf } from "./list-model";
import { HOUR } from "./monitor-model";

const COLUMNS: DataColumn<FunctionRow>[] = [
  { id: "name", header: "Function name", min: 170, accessorFn: (r) => r.name.toLowerCase(), meta: { mono: true }, cell: (c) => <ResourceTo to={{ service: "lambda", kind: "functions", name: c.row.original.name }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
  { id: "description", header: "Description", size: 320, min: 160, accessorFn: (r) => r.description, cell: (c) => c.row.original.description || <span className="text-muted-foreground">-</span> },
  { id: "package", header: "Package type", size: 140, fit: 112, accessorFn: (r) => r.pkg },
  { id: "runtime", header: "Runtime", size: 150, fit: 80, accessorFn: (r) => r.runtimeText },
  { id: "modified", header: "Last modified", size: 200, fit: 112, accessorFn: (r) => r.modified ?? "", cell: (c) => <Value value={c.row.original.modified} format="time" /> },
];

export function LambdaList({ service, kind }: { service: string; kind: string }) {
  const search = useSearch({ strict: false }) as { q?: string; create?: string; gone?: string };
  if (search.create === "yes") return <FunctionCreate />;
  return <FunctionsList service={service} kind={kind} q={search.q ?? ""} gone={search.gone} />;
}

function FunctionsList({ service, kind, q, gone }: { service: string; kind: string; q: string; gone?: string }) {
  const navigate = useNavigate();
  const about = useAbout().data;
  const list = useResourceList(service, kind);
  const [typed, setTyped] = useState(q);
  const { refetch } = list;
  const refresh = useCallback(() => void refetch(), [refetch]);
  const first = list.data?.pages[0];
  const all = useMemo(() => rowsOf(list.data?.pages.flatMap((p: ResourceList) => p.rows) ?? []), [list.data]);
  const rows = useMemo(() => (q ? all.filter((r) => r.name.includes(q)) : all), [all, q]);
  const region = about?.region ?? "this Region";
  const [now] = useState(() => Date.now());
  const queries: MetricQuery[] = useMemo(() => all.slice(0, 20).map((r, i) => ({ id: `f${i}`, label: r.name, namespace: "AWS/Lambda", metric: "Invocations", dimensions: { FunctionName: r.name }, stat: "Sum", period: 3600 })), [all]);
  const metrics = useMetricData(queries, { from: now - HOUR, to: now }, !gone && queries.length > 0);
  const counts = metrics.series.map((s) => ({ name: s.label, n: Math.round(s.points.reduce((a, p) => a + (p.v ?? 0), 0)) }));
  const go = (next: { q?: string; create?: "yes" }) => void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: next });
  const create = <Button onClick={() => go({ create: "yes" })} data-testid="create-open"><PlusIcon /> Create Event Function</Button>;
  const none = all.length === 0;
  const names = all.map((r) => r.name);
  const ran = invokedSentence(counts);
  const left = gone ? leftWords(gone, readLeft(recalledLeft(gone))) : [];
  const sentence = first?.sentence ? `${gone && all.length ? `${all.length} ${all.length === 1 ? "function runs" : "functions run"} here: ${all.map((r) => r.name).join(", ")}.` : first.sentence}${!gone && ran ? ` ${ran}` : ""}${left[0]?.includes("still point") ? ` ${left[0]}` : ""}` : null;
  return (
    <>
      {gone && <ResultBanner className="mb-6" title={`${gone} was deleted.`} returned="DeleteFunction removed the function, its versions and its aliases." onDismiss={() => go({})} testId="deleted-banner" />}
      <PageHeader
        title="Event Functions"
        crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.service_title, to: `/resources/${service}` }] : [])]}
        sub={none ? `Functions in ${region}.` : `Functions in ${region}. Choose a name to open it, or create a new one.`}
        refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={refresh} interval={false} />}
        actions={create}
      />
      {list.isError && !first ? (
        <ErrorBlock what="The functions could not be listed" error={list.error} hint={HINT} onRetry={refresh} retrying={list.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : none ? (
        <div data-testid="list-empty">
          <EmptyState icon={ZapIcon} title="No functions" action={<Button variant="outline" className="bg-card" onClick={() => go({ create: "yes" })} data-testid="create-empty">Create function</Button>}>
            You have no functions in this region. Create a function to run code without managing servers.
          </EmptyState>
        </div>
      ) : (
        <>
          {sentence && <p className="mb-5 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="list-sentence"><Emphasis text={sentence} names={names} bold={[/\b\d+ (?=functions? run)/]} /></p>}
          <nav aria-label="Other pages of the Lambda console" className="mb-4 flex items-center gap-2 text-[13.5px]">
            <span className="text-muted-foreground">Other pages of the Lambda console</span>
            <Link to="/resources/$service/$second" params={{ service: "lambda", second: "layers" }} search={{}} className="rounded-sm font-medium text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="layers-link">Layers</Link>
          </nav>
          <div className="mb-4 flex flex-wrap items-center gap-x-3 gap-y-2">
            <form className="flex items-center gap-3" role="search" onSubmit={(e) => { e.preventDefault(); go(typed.trim() ? { q: typed.trim() } : {}); }}>
              <div className="relative w-full min-w-[16rem] max-w-[22rem]">
                <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="Search functions" aria-label="Search functions by name" className="bg-card pl-9 font-mono text-[13px]" data-testid="list-search" />
              </div>
              <Button type="submit" variant="outline" className="bg-card" data-testid="list-search-go">Search</Button>
              {q && <Button type="button" variant="ghost" onClick={() => { setTyped(""); go({}); }} data-testid="search-clear">Clear search</Button>}
            </form>
          </div>
          {rows.length === 0 ? (
            <div data-testid="list-no-match"><EmptyState icon={SearchIcon} title="No function matches" action={<Button variant="outline" className="bg-card" onClick={() => { setTyped(""); go({}); }}>Show all {all.length}</Button>}>No function name contains <span className="font-mono text-foreground">“{q}”</span>.</EmptyState></div>
          ) : (
            <>
              <DataTable aria-label="Functions" data={rows} columns={COLUMNS} rowId={(r) => r.name} maxHeight={680} />
              <p className="mt-3 text-[13px] text-muted-foreground">The columns are fields of ListFunctions: FunctionName, Description, PackageType, Runtime and LastModified.</p>
              <Pagination className="mt-3" shown={rows.length} total={rows.length} more={list.hasNextPage} onMore={() => void list.fetchNextPage()} loading={list.isFetchingNextPage} noun={rows.length === 1 ? "function" : "functions"} />
            </>
          )}
          {left.length > 0 && (
            <Callout tone="note" className="mt-5" testId="left-behind">
              {left.slice(left[0]?.includes("still point") ? 1 : 0).join(" ")}
            </Callout>
          )}
        </>
      )}
    </>
  );
}

// An HTTP API's Routes (apigateway__http-routes, http-routes-none): the routes with the integration and authorization each has, the one
// chosen explained, how a request is matched, and Recent calls. A route is deleted from its row.
import { useNavigate } from "@tanstack/react-router";
import { PlusIcon, RouteIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { Button } from "@doze/ui/components/ui/button";
import type { CallRecord } from "../../../api/types/CallRecord";
import { PlainConfirm, RecentCalls, RuleHelp } from "../../../kit";
import { HINT } from "../index-page";
import { HTTP, useHttpAuthorizers, useHttpStages, useIntegrations, useRoutes, useWrite } from "./api";
import { sampleOf } from "./http-model";
import { httpSections } from "./sections";
import { ApiChips, ApiLink, Caption, Mono, Rows, Said } from "./parts";
import type { HttpApi, HttpIntegration, Route } from "./types";
import { authorizationWords, httpTarget, lambdaName, routesSentence } from "./words";

export function HttpRoutes({ id, api, calls, selected }: { id: string; api: HttpApi; calls: CallRecord[]; selected: string | undefined }) {
  const navigate = useNavigate();
  const routes = useRoutes(id);
  const integrations = useIntegrations(id);
  const authorizers = useHttpAuthorizers(id).data?.items ?? [];
  const stages = useHttpStages(id).data?.items ?? [];
  const del = useWrite(HTTP, "DeleteRoute");
  const [deleting, setDeleting] = useState<Route | null>(null);
  const go = (search: Record<string, string | undefined>) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "apis", name: id }, search: search as never });
  if (routes.isError) return <ErrorBlock what="The routes could not be read" error={routes.error} hint={HINT} onRetry={() => void routes.refetch()} retrying={routes.isFetching} testId="routes-error" />;
  if (!routes.data || !integrations.data) return null;
  const all = routes.data.items ?? [];
  const ints = integrations.data.items ?? [];
  const byId = (target: string | undefined): HttpIntegration | undefined => ints.find((i) => target === `integrations/${i.integrationId}`);
  const name = (r: Route): string | null => { const i = byId(r.target); return i ? (lambdaName(i.integrationUri) ?? httpTarget(i.integrationType, i.integrationUri)) : null; };
  const websocket = api.protocolType === "WEBSOCKET";
  const create = <Button variant="outline" className="bg-card" onClick={() => go({ do: "create-route" })} data-testid="create-route"><PlusIcon /> Create route</Button>;
  const chips = <ApiChips kind="apis" id={id} name={api.name} sections={httpSections(api.protocolType === "WEBSOCKET")} current="routes" />;
  if (all.length === 0) {
    return (
      <>
        {chips}
        <EmptyState icon={RouteIcon} title="No routes" action={create}>{websocket ? <>A route is chosen for each message a client sends, by the API&apos;s route selection expression <span className="font-mono">{api.routeSelectionExpression ?? "$request.body.action"}</span>, and maps it to an integration.</> : "A route maps a request method and path to an integration."}</EmptyState>
      </>
    );
  }
  const chosen = all.find((r) => r.routeId === selected) ?? all[0]!;
  const authorizer = authorizers.find((a) => a.authorizerId === chosen.authorizerId);
  const columns: DataColumn<Route>[] = [
    { id: "key", header: "Route key", min: 200, accessorFn: (r) => r.routeKey, cell: (c) => <span className="font-mono text-[13.5px] font-medium text-bright" data-testid="route-key">{c.row.original.routeKey}</span> },
    { id: "integration", header: "Integration", size: 260, accessorFn: (r) => name(r) ?? "", cell: (c) => name(c.row.original) ?? <StatusDot tone="warn">None</StatusDot> },
    { id: "auth", header: "Authorization type", size: 200, accessorFn: (r) => r.authorizationType ?? "NONE" },
    { id: "delete", header: "", size: 72, enableSorting: false, cell: (c) => <Button variant="ghost" size="icon" aria-label={`Delete route ${c.row.original.routeKey}`} onClick={(e) => { e.stopPropagation(); setDeleting(c.row.original); }} data-testid={`delete-${c.row.original.routeKey}`}><Trash2Icon /></Button> },
  ];
  const sentence = routesSentence(api.name, all, stages, { fn: (t) => { const i = byId(t); return i ? lambdaName(i.integrationUri) : null; }, authorizer: (aid) => { const a = authorizers.find((x) => x.authorizerId === aid); return a ? `the ${a.authorizerType === "JWT" ? "JWT" : "Lambda"} authorizer ${a.name}` : null; } });
  const auth = chosen.authorizationType ?? "NONE";
  return (
    <>
      <Said text={sentence} names={[api.name, ...stages.map((s) => s.stageName), ...authorizers.map((a) => a.name), ...all.map((r) => r.routeKey), ...ints.flatMap((i) => (lambdaName(i.integrationUri) ? [lambdaName(i.integrationUri)!] : []))]} bold={[/\b\d+ routes\b/]} />
      {chips}
      <div className="grid items-start gap-x-6 xl:grid-cols-[minmax(0,1fr)_minmax(0,30rem)]">
      <div className="min-w-0">
      <Caption title="Routes" action={create} />
      <DataTable<Route> aria-label="Routes" data={all} columns={columns} rowId={(r) => r.routeId} onRowClick={(r) => go({ key: r.routeId })} rowClassName={(r) => (r.routeId === chosen.routeId ? "[&>td]:bg-pick/50" : undefined)} />
      </div>
      <div className="min-w-0">
      <Caption title="Selected route" />
      <section className="rounded-[14px] border bg-card p-6 lg:p-7" aria-label="Selected route" data-testid="selected-route">
        <Rows rows={[
          { label: "Route key", value: <Mono>{chosen.routeKey}</Mono> },
          { label: "Route ID", value: <Mono>{chosen.routeId}</Mono> },
          { label: "Integration", value: byId(chosen.target) ? <><span className="font-medium">{name(chosen)}</span> <span className="text-muted-foreground">({byId(chosen.target)!.integrationType}, <ApiLink kind="apis" id={id} search={{ tab: "integrations", key: chosen.routeId }}>change</ApiLink>)</span></> : <>No integration is attached to this route. Attach one on the <ApiLink kind="apis" id={id} search={{ tab: "integrations", key: chosen.routeId }} testId="attach-link">Integrations</ApiLink> page.</> },
          { label: "Authorization type", value: auth === "NONE" ? <>NONE: no authorizer, the route is open.</> : <>{auth}: {authorizationWords(auth)}{authorizer ? <> <span className="font-mono">{authorizer.name}</span></> : null}</> },
        ]} />
        {websocket ? (
          <RuleHelp className="mt-6">A message goes to the route whose key is the value of the route selection expression <span className="font-mono">{api.routeSelectionExpression ?? "$request.body.action"}</span> in it; one that selects no route goes to <span className="font-mono">$default</span>. <span className="font-mono">$connect</span> runs when a client connects and may refuse it, <span className="font-mono">$disconnect</span> when it leaves.</RuleHelp>
        ) : (
          <RuleHelp className="mt-6">A request takes the most specific route: a full route and method, then a greedy path such as <span className="font-mono">{"{proxy+}"}</span>, then <span className="font-mono">$default</span>.{/\{[^}]+\+\}/.test(chosen.routeKey) && <> <span className="font-mono">{chosen.routeKey.split(" ")[0] === "ANY" ? "POST" : chosen.routeKey.split(" ")[0]} {sampleOf(chosen.routeKey)}</span> would land on this route.</>}</RuleHelp>
        )}
      </section>
      </div>
      </div>
      <RecentCalls className="mt-12" calls={calls} noun="API" title="Recent calls" scope={api.name} />
      <PlainConfirm open={deleting !== null} onOpenChange={(o) => { if (!o) { setDeleting(null); del.reset(); } }} title="Delete route?" description={<>Delete <b className="font-mono font-semibold text-bright">{deleting?.routeKey}</b>? Requests that matched it then fall to a less specific route, or get 404.</>} call="DeleteRoute" pending={del.isPending} error={del.error} onConfirm={() => deleting && del.mutate({ ApiId: id, RouteId: deleting.routeId } as never, { onSuccess: () => { setDeleting(null); go({}); } })} testId="route-delete" />
    </>
  );
}

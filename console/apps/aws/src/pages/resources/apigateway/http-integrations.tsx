// An HTTP API's Integrations (apigateway__http-route-integration, http-integrations-none): the routes with what each calls, attaching an
// integration to the route chosen (UpdateRoute), the integrations there are, and Create integration.
import { useNavigate } from "@tanstack/react-router";
import { PlugIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { Alert, ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { Button } from "@doze/ui/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { CallPanel, Field, PlainConfirm, RuleHelp } from "../../../kit";
import { HINT } from "../index-page";
import { HTTP, useIntegrations, useRoutes, useWrite } from "./api";
import { ApiChips, Caption } from "./parts";
import { httpSections } from "./sections";
import type { HttpApi, HttpIntegration, Route } from "./types";
import { httpTarget, lambdaName } from "./words";
import { sdkShown } from "./calls";

const words = (i: HttpIntegration): string => lambdaName(i.integrationUri) ? `${lambdaName(i.integrationUri)} Lambda` : (i.integrationUri ?? i.integrationType);
const explain = (i: HttpIntegration): React.ReactNode =>
  i.integrationType === "HTTP_PROXY" ? <>HTTP proxy to <span className="font-mono break-all">{i.integrationUri}</span>. A <span className="font-mono">{"{param}"}</span> placeholder is filled from the request path, and the request&apos;s method is used unless the integration names one.</> :
  lambdaName(i.integrationUri) ? <>Lambda function <span className="font-mono">{lambdaName(i.integrationUri)}</span>, payload format {i.payloadFormatVersion ?? "2.0"}. API Gateway sends the request to it as an event and returns what it answers.</> :
  <>{i.integrationType}: <span className="font-mono break-all">{i.integrationUri}</span></>;

export function HttpIntegrations({ id, api, selected }: { id: string; api: HttpApi; selected: string | undefined }) {
  const navigate = useNavigate();
  const routes = useRoutes(id);
  const integrations = useIntegrations(id);
  const attach = useWrite(HTTP, "UpdateRoute");
  const del = useWrite(HTTP, "DeleteIntegration");
  const [picked, setPicked] = useState<{ route: string; value: string } | null>(null);
  const [deleting, setDeleting] = useState<HttpIntegration | null>(null);
  const go = (search: Record<string, string | undefined>) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "apis", name: id }, search: search as never });
  const all = routes.data?.items ?? [];
  const ints = integrations.data?.items ?? [];
  const chosen = all.find((r) => r.routeId === selected) ?? all.find((r) => !r.target) ?? all[0];
  const current = ints.find((i) => chosen?.target === `integrations/${i.integrationId}`);
  // What is chosen belongs to the route it was chosen for; another route starts from what it calls, or the first integration.
  const pick = picked && picked.route === chosen?.routeId ? picked.value : (current?.integrationId ?? ints[0]?.integrationId ?? "");
  if (routes.isError || integrations.isError) return <ErrorBlock what="The integrations could not be read" error={routes.error ?? integrations.error} hint={HINT} onRetry={() => { void routes.refetch(); void integrations.refetch(); }} retrying={routes.isFetching} testId="integrations-error" />;
  if (!routes.data || !integrations.data) return null;
  const chips = <ApiChips kind="apis" id={id} name={api.name} sections={httpSections(api.protocolType === "WEBSOCKET")} current="integrations" />;
  const create = <Button variant="outline" className="bg-card" onClick={() => go({ do: "create-integration", key: chosen?.routeId })} data-testid="create-integration"><PlusIcon /> Create integration</Button>;
  const bound = (r: Route) => ints.find((i) => r.target === `integrations/${i.integrationId}`);
  const input = chosen && pick ? { ApiId: id, RouteId: chosen.routeId, target: `integrations/${pick}` } : null;
  const failed = attach.error ? errorWords(attach.error) : null;
  const routeColumns: DataColumn<Route>[] = [
    { id: "key", header: "Route key", min: 220, accessorFn: (r) => r.routeKey, cell: (c) => <span className="font-mono text-[13.5px] font-medium text-bright" data-testid="route-key">{c.row.original.routeKey}</span> },
    { id: "integration", header: "Integration", accessorFn: (r) => (bound(r) ? words(bound(r)!) : ""), cell: (c) => (bound(c.row.original) ? words(bound(c.row.original)!) : <span className="text-muted-foreground">None</span>) },
  ];
  const intColumns: DataColumn<HttpIntegration>[] = [
    { id: "id", header: "Integration ID", size: 170, accessorFn: (i) => i.integrationId, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.integrationId}</span> },
    { id: "type", header: "Integration type", size: 190, accessorFn: (i) => i.integrationType, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.integrationType}</span> },
    { id: "uri", header: "Integration URI", min: 240, accessorFn: (i) => i.integrationUri ?? "", cell: (c) => <span className="font-mono text-[13px]">{lambdaName(c.row.original.integrationUri) ? `function:${lambdaName(c.row.original.integrationUri)}` : httpTarget(c.row.original.integrationType, c.row.original.integrationUri)}</span> },
    { id: "delete", header: "", size: 72, enableSorting: false, cell: (c) => <Button variant="ghost" size="icon" aria-label={`Delete integration ${c.row.original.integrationId}`} onClick={() => setDeleting(c.row.original)} data-testid={`delete-${c.row.original.integrationId}`}><Trash2Icon /></Button> },
  ];
  if (ints.length === 0) {
    return (
      <>
        {chips}
        <Caption title="Existing integrations" />
        <EmptyState icon={PlugIcon} title="No integrations" action={create}>{api.name} has no integrations. An integration connects a route to a backend such as a Lambda function or an HTTP endpoint.</EmptyState>
      </>
    );
  }
  const options = ints;
  return (
    <>
      {chips}
      <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_22.5rem]">
        <div className="min-w-0">
          <Caption title="Routes" />
          <DataTable<Route> aria-label="Routes" data={all} columns={routeColumns} rowId={(r) => r.routeId} onRowClick={(r) => go({ tab: "integrations", key: r.routeId })} rowClassName={(r) => (r.routeId === chosen?.routeId ? "[&>td]:bg-pick/50" : undefined)} empty="No route yet: create a route first, then attach an integration to it." />
          {chosen && (
            <section className="mt-8 rounded-[14px] border bg-card p-6 lg:p-7" aria-label="Attach an integration" data-testid="attach">
              <h2 className="text-[17px] leading-6 font-semibold text-bright">Attach an integration to <span className="font-mono">{chosen.routeKey}</span></h2>
              <form className="mt-5 grid gap-5" onSubmit={(e) => { e.preventDefault(); if (input) attach.mutate(input as never); }}>
                <Field label="Choose an existing integration" hint={pick && options.find((i) => i.integrationId === pick) ? explain(options.find((i) => i.integrationId === pick)!) : undefined}>
                  {(fid) => (
                    <Select value={pick} onValueChange={(v) => { setPicked({ route: chosen.routeId, value: v }); attach.reset(); }}>
                      <SelectTrigger id={fid} className="w-full bg-card font-mono" data-testid="attach-pick"><SelectValue placeholder="Choose an integration" /></SelectTrigger>
                      <SelectContent>{options.map((i) => <SelectItem key={i.integrationId} value={i.integrationId} className="font-mono">{lambdaName(i.integrationUri) ? `${lambdaName(i.integrationUri)} (${i.integrationId})` : `${i.integrationUri} (${i.integrationId})`}</SelectItem>)}</SelectContent>
                    </Select>
                  )}
                </Field>
                {failed && <Alert tone="bad" title={`UpdateRoute was refused${failed.code ? `: ${failed.code}` : ""}`} testId="attach-error">{failed.message}</Alert>}
                {attach.isSuccess && (attach.variables as { RouteId?: string } | undefined)?.RouteId === chosen.routeId && <p className="text-[13.5px] text-good" role="status" data-testid="attach-done">Attached: {chosen.routeKey} now calls {words(options.find((i) => i.integrationId === pick) ?? options[0]!)}.</p>}
                <div><Button type="submit" disabled={!input || attach.isPending || (current?.integrationId === pick)} data-testid="attach-submit">{attach.isPending ? "Attaching…" : "Attach integration"}</Button></div>
              </form>
            </section>
          )}
          <div className="mt-10" />
          <Caption title="Existing integrations" action={create} />
          <DataTable<HttpIntegration> aria-label="Existing integrations" data={ints} columns={intColumns} rowId={(i) => i.integrationId} />
          <RuleHelp className="mt-4">One integration can serve several routes. A new integration is made with Create integration, whose form has an &quot;Attach this integration to a route&quot; field.</RuleHelp>
        </div>
        <div className="xl:sticky xl:top-6"><CallPanel request={input ? { service: "apigatewayv2", operation: "UpdateRoute", input } : null} shown={sdkShown} showInput note={<>The route points at the integration by ID. The integrations here are <span className="font-mono">AWS_PROXY</span> (a Lambda function) and <span className="font-mono">HTTP_PROXY</span> (a URL).</>} /></div>
      </div>
      <PlainConfirm open={deleting !== null} onOpenChange={(o) => { if (!o) { setDeleting(null); del.reset(); } }} title="Delete integration?" description={<>Delete integration <b className="font-mono font-semibold text-bright">{deleting?.integrationId}</b>? {all.filter((r) => r.target === `integrations/${deleting?.integrationId}`).length > 0 ? <>The routes that call it ({all.filter((r) => r.target === `integrations/${deleting?.integrationId}`).map((r) => r.routeKey).join(", ")}) name it, and the service refuses the delete while a route does: attach another integration to each first.</> : "No route calls it."}</>} call="DeleteIntegration" pending={del.isPending} error={del.error} onConfirm={() => deleting && del.mutate({ ApiId: id, IntegrationId: deleting.integrationId } as never, { onSuccess: () => setDeleting(null) })} testId="integration-delete" />
    </>
  );
}

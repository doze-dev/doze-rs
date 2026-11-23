// An HTTP API's Authorization (apigateway__http-authorizer-create's parent): attach an authorizer to a route, or manage the authorizers
// (Create authorizer). A Lambda authorizer makes the route's type CUSTOM, a JWT one JWT.
import { useNavigate } from "@tanstack/react-router";
import { PlusIcon, ShieldCheckIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { Alert, ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { Button } from "@doze/ui/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { CallPanel, Field, PlainConfirm, RuleHelp } from "../../../kit";
import { HINT } from "../index-page";
import { HTTP, useHttpAuthorizers, useRoutes, useWrite } from "./api";
import { ApiChips } from "./parts";
import { httpSections } from "./sections";
import type { HttpApi, HttpAuthorizer, Route } from "./types";
import { lambdaName, list } from "./words";
import { sdkShown } from "./calls";

const NONE = "__none__";
const kindWords = (a: HttpAuthorizer) => (a.authorizerType === "JWT" ? "JWT" : "Lambda (request)");

export function HttpAuthorization({ id, api, mode }: { id: string; api: HttpApi; mode: string | undefined }) {
  const navigate = useNavigate();
  const routes = useRoutes(id);
  const authorizers = useHttpAuthorizers(id);
  const attach = useWrite(HTTP, "UpdateRoute");
  const del = useWrite(HTTP, "DeleteAuthorizer");
  const [routeId, setRouteId] = useState("");
  const [picked, setPicked] = useState<{ route: string; value: string } | null>(null);
  const [deleting, setDeleting] = useState<HttpAuthorizer | null>(null);
  const all = authorizers.data?.items ?? [];
  const rs = routes.data?.items ?? [];
  const route = rs.find((r) => r.routeId === routeId) ?? rs[0];
  const manage = mode === "manage";
  // What is chosen belongs to the route it was chosen for; another route starts from its own authorizer.
  const pick = picked && picked.route === route?.routeId ? picked.value : (route?.authorizerId ?? NONE);
  const go = (search: Record<string, string | undefined>) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "apis", name: id }, search: search as never });
  if (routes.isError || authorizers.isError) return <ErrorBlock what="The authorizers could not be read" error={routes.error ?? authorizers.error} hint={HINT} onRetry={() => { void routes.refetch(); void authorizers.refetch(); }} retrying={routes.isFetching} testId="authorization-error" />;
  if (!routes.data || !authorizers.data) return null;
  const chosen = all.find((a) => a.authorizerId === pick);
  const input = route ? { ApiId: id, RouteId: route.routeId, authorizationType: chosen ? (chosen.authorizerType === "JWT" ? "JWT" : "CUSTOM") : "NONE", ...(chosen ? { authorizerId: chosen.authorizerId } : {}) } : null;
  const failed = attach.error ? errorWords(attach.error) : null;
  const usedBy = (a: HttpAuthorizer) => rs.filter((r) => r.authorizerId === a.authorizerId).map((r) => r.routeKey);
  const create = <Button onClick={() => go({ do: "create-authorizer" })} data-testid="create-authorizer"><PlusIcon /> Create authorizer</Button>;
  const columns: DataColumn<HttpAuthorizer>[] = [
    { id: "name", header: "Name", min: 180, accessorFn: (a) => a.name, cell: (c) => <span className="font-mono text-[13.5px] font-medium text-bright" data-testid="authorizer-name">{c.row.original.name}</span> },
    { id: "type", header: "Type", size: 170, accessorFn: kindWords },
    { id: "target", header: "Function or issuer", min: 200, accessorFn: (a) => lambdaName(a.authorizerUri) ?? a.jwtConfiguration?.issuer ?? "", cell: (c) => <span className="font-mono text-[13px]">{lambdaName(c.row.original.authorizerUri) ?? c.row.original.jwtConfiguration?.issuer ?? "–"}</span> },
    { id: "source", header: "Identity source", size: 220, accessorFn: (a) => (a.identitySource ?? []).join(", "), cell: (c) => <span className="font-mono text-[13px]">{(c.row.original.identitySource ?? []).join(", ") || "–"}</span> },
    { id: "ttl", header: "TTL", size: 120, accessorFn: (a) => a.authorizerResultTtlInSeconds ?? 0, cell: (c) => (c.row.original.authorizerType === "JWT" ? "–" : `${c.row.original.authorizerResultTtlInSeconds ?? 0} seconds`) },
    { id: "delete", header: "", size: 72, enableSorting: false, cell: (c) => <Button variant="ghost" size="icon" aria-label={`Delete authorizer ${c.row.original.name}`} onClick={() => setDeleting(c.row.original)} data-testid={`delete-${c.row.original.name}`}><Trash2Icon /></Button> },
  ];
  return (
    <>
      <ApiChips kind="apis" id={id} name={api.name} sections={httpSections(api.protocolType === "WEBSOCKET")} current="authorization" />
      <Tabs value={manage ? "manage" : "attach"} onValueChange={(v) => go({ tab: "authorization", mode: v })} className="gap-0">
        <TabsList variant="line" className="h-11 w-full justify-start gap-4 rounded-none border-b p-0" aria-label="Authorization pages">
          <TabsTrigger value="attach" className="h-11 flex-none px-2 text-[14.5px] after:bottom-[-1px]" data-testid="tab-attach">Attach authorizer</TabsTrigger>
          <TabsTrigger value="manage" className="h-11 flex-none px-2 text-[14.5px] after:bottom-[-1px]" data-testid="tab-manage">Manage authorizers</TabsTrigger>
        </TabsList>
      </Tabs>
      <div className="pt-8">
        {manage ? (
          all.length === 0 ? (
            <EmptyState icon={ShieldCheckIcon} title="No authorizers" action={create}>{api.name} has no authorizer. An authorizer decides, before a route runs, whether the caller may pass.</EmptyState>
          ) : (
            <>
              <div className="mb-3 flex justify-end">{create}</div>
              <DataTable<HttpAuthorizer> aria-label="Authorizers" data={all} columns={columns} rowId={(a) => a.authorizerId} />
              <RuleHelp className="mt-4 max-w-3xl">A <b>Lambda</b> authorizer calls a function and, with simple responses, accepts <span className="font-mono">{"{\"isAuthorized\": true}"}</span>. A <b>JWT</b> authorizer checks a token&apos;s signature against the issuer&apos;s keys (RSA algorithms only), its issuer, expiry, audience and the route&apos;s scopes.</RuleHelp>
            </>
          )
        ) : rs.length === 0 ? (
          <EmptyState icon={ShieldCheckIcon} title="No routes" action={<Button variant="outline" className="bg-card" onClick={() => go({ do: "create-route" })}>Create route</Button>}>An authorizer is attached to a route, and {api.name} has none yet.</EmptyState>
        ) : (
          <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_22.5rem]">
            <section className="min-w-0 rounded-[14px] border bg-card p-6 lg:p-7" aria-label="Attach an authorizer" data-testid="attach-authorizer">
              <h2 className="text-[17px] leading-6 font-semibold text-bright">Attach an authorizer to a route</h2>
              <form className="mt-5 grid gap-5" onSubmit={(e) => { e.preventDefault(); if (input) attach.mutate(input as never); }}>
                <Field label="Route">
                  {(fid) => (
                    <Select value={route?.routeId ?? ""} onValueChange={(v) => { setRouteId(v); attach.reset(); }}>
                      <SelectTrigger id={fid} className="w-full bg-card font-mono" data-testid="attach-route"><SelectValue /></SelectTrigger>
                      <SelectContent>{rs.map((r: Route) => <SelectItem key={r.routeId} value={r.routeId} className="font-mono">{r.routeKey} {r.authorizationType && r.authorizationType !== "NONE" ? `(${r.authorizationType})` : ""}</SelectItem>)}</SelectContent>
                    </Select>
                  )}
                </Field>
                <Field label="Authorizer" hint={all.length === 0 ? <>{api.name} has no authorizer yet. Make one on the Manage authorizers page.</> : <>The route&apos;s authorization type becomes <span className="font-mono">{chosen ? input?.authorizationType : "NONE"}</span>.{chosen && usedBy(chosen).length > 0 && <> {list(usedBy(chosen))} already {usedBy(chosen).length === 1 ? "uses" : "use"} it.</>}</>}>
                  {(fid) => (
                    <Select value={pick} onValueChange={(v) => { setPicked({ route: route!.routeId, value: v }); attach.reset(); }}>
                      <SelectTrigger id={fid} className="w-full bg-card font-mono" data-testid="attach-authorizer-pick"><SelectValue /></SelectTrigger>
                      <SelectContent><SelectItem value={NONE} className="font-sans">None: the route is open</SelectItem>{all.map((a) => <SelectItem key={a.authorizerId} value={a.authorizerId} className="font-mono">{a.name}</SelectItem>)}</SelectContent>
                    </Select>
                  )}
                </Field>
                {failed && <Alert tone="bad" title={`UpdateRoute was refused${failed.code ? `: ${failed.code}` : ""}`} testId="attach-error">{failed.message}</Alert>}
                {attach.isSuccess && <p className="text-[13.5px] text-good" role="status" data-testid="attach-done">Done: {route?.routeKey} is now {chosen ? `checked by ${chosen.name}` : "open"}.</p>}
                <div className="flex flex-wrap gap-3"><Button type="submit" disabled={!input || attach.isPending || pick === (route?.authorizerId ?? NONE)} data-testid="attach-submit">{attach.isPending ? "Attaching…" : chosen ? "Attach authorizer" : "Detach authorizer"}</Button>{create}</div>
              </form>
            </section>
            <div className="xl:sticky xl:top-6"><CallPanel request={input ? { service: "apigatewayv2", operation: "UpdateRoute", input } : null} shown={sdkShown} showInput note={<>Creating an authorizer does not protect a route: attaching it is this second call. The route&apos;s authorization type becomes <span className="font-mono">CUSTOM</span> (Lambda) or <span className="font-mono">JWT</span>.</>} /></div>
          </div>
        )}
      </div>
      <PlainConfirm open={deleting !== null} onOpenChange={(o) => { if (!o) { setDeleting(null); del.reset(); } }} title="Delete authorizer?" description={<>Delete <b className="font-mono font-semibold text-bright">{deleting?.name}</b>? {deleting && usedBy(deleting).length > 0 ? <>{list(usedBy(deleting))} still {usedBy(deleting).length === 1 ? "uses" : "use"} it, and the service refuses the delete while a route does.</> : "No route uses it."}</>} call="DeleteAuthorizer" pending={del.isPending} error={del.error} onConfirm={() => deleting && del.mutate({ ApiId: id, AuthorizerId: deleting.authorizerId } as never, { onSuccess: () => setDeleting(null) })} testId="authorizer-delete" />
    </>
  );
}

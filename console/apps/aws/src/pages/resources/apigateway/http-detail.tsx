// An HTTP (or WebSocket) API's page (apigateway__http-routes, http-integrations, http-cors, http-stages and what they open): the header (the
// section as the title, the API as a chip), the chips between the sections, and the forms a section opens.
// `?tab=` routes | integrations | authorization | cors | stages; `?do=` create-route, create-integration, create-authorizer.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { PlusIcon } from "lucide-react";
import { lazy, Suspense } from "react";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useResourceDetail } from "../../../api/resources";
import { DetailError, useServiceCrumbs } from "../../../kit";
import { useHttpApi, useRefreshReads } from "./api";
import { HttpAuthorization } from "./http-authorization";
import { HttpCors } from "./http-cors";
import { HttpIntegrations } from "./http-integrations";
import { HttpRoutes } from "./http-routes";
import { HttpStages } from "./http-stages";
import { NameChips } from "./parts";
import { httpSections } from "./sections";
import { when } from "./words";

const RouteCreate = lazy(() => import("./route-create").then((m) => ({ default: m.RouteCreate })));
const IntegrationCreate = lazy(() => import("./integration-create").then((m) => ({ default: m.IntegrationCreate })));
const HttpAuthorizerCreate = lazy(() => import("./http-authorizer-create").then((m) => ({ default: m.HttpAuthorizerCreate })));

type Search = { tab?: string; do?: string; key?: string; sel?: string; mode?: string; done?: string; name?: string };
const SUBS: Record<string, string> = {
  integrations: "Choose a route, then attach an integration to it. An integration connects a route to a backend; one integration can serve several routes.",
  authorization: "Have API Gateway ask a Lambda function, or check a token, before a request may pass. Attach an authorizer to a route.",
  cors: "Let a web app on another origin call this API from the browser. API Gateway answers preflight requests itself.",
  stages: "A stage is a named reference to a deployment, which is a snapshot of the API.",
};

export function HttpDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const api = useHttpApi(name);
  const refresh = useRefreshReads();
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={() => void detail.refetch()} retrying={detail.isFetching} />;
  if (!detail.data || !api.data) return <div className="grid gap-4" data-testid="resource-page-loading"><Skeleton className="h-12 w-80" /><Skeleton className="h-72 rounded-[14px]" /></div>;
  const a = api.data;
  return <HttpView id={name} api={a} calls={detail.data.calls} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={() => { refresh(); void detail.refetch(); }} />;
}

function HttpView({ id, api, calls, updatedAt, fetching, refresh }: { id: string; api: import("./types").HttpApi; calls: import("../../../api/types/CallRecord").CallRecord[]; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("apigateway");
  const s = useSearch({ strict: false }) as Search;
  const websocket = api.protocolType === "WEBSOCKET";
  const sections = httpSections(websocket);
  const section = sections.find((x) => x.id === s.tab)?.id ?? "routes";
  const go = (search: Search) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "apis", name: id }, search: search as never });
  const label = sections.find((x) => x.id === section)!.label;
  const back = [...crumbs, { label: "APIs", to: "/resources/apigateway/restapis" }, { label: api.name, to: `/resources/apigateway/apis/${id}` }];
  const form = s.do ? (
    <Suspense fallback={<Skeleton className="h-96 rounded-[14px]" />}>
      {s.do === "create-route" && <RouteCreate api={id} apiName={api.name} websocket={websocket} crumbs={back} onClose={(k) => go(k ? { done: "route", name: k } : {})} />}
      {s.do === "create-integration" && <IntegrationCreate api={id} apiName={api.name} route={s.key} crumbs={back} onClose={(done) => go({ tab: "integrations", ...(done ? { done: "integration", name: done } : {}) })} />}
      {s.do === "create-authorizer" && <HttpAuthorizerCreate api={id} apiName={api.name} crumbs={back} onClose={(n) => go({ tab: "authorization", mode: "manage", ...(n ? { done: "authorizer", name: n } : {}) })} />}
    </Suspense>
  ) : null;
  if (form) return form;
  const banner =
    s.done === "created" ? <ResultBanner title={`${websocket ? "WebSocket" : "HTTP"} API ${api.name} created.`} returned={websocket ? <>CreateApi returned its ID and its address. The API has no route, integration or stage yet: add a <span className="font-mono">$connect</span> or <span className="font-mono">$default</span> route with an integration, and a stage, before a client can connect.</> : "Its integration, route and stage were made, and the stage deploys every change."} onDismiss={() => go({})} testId="created-banner" /> :
    s.done === "route" ? <ResultBanner title={`Route ${s.name} created.`} returned="CreateRoute returned its ID. Attach an integration to it on the Integrations page." onDismiss={() => go({})} testId="route-banner" /> :
    s.done === "integration" ? <ResultBanner title={`Integration ${s.name} created.`} returned="CreateIntegration returned its ID. Attach it to a route below." onDismiss={() => go({ tab: "integrations" })} testId="integration-banner" /> :
    s.done === "authorizer" ? <ResultBanner title={`Authorizer ${s.name} created.`} returned="CreateAuthorizer returned its ID. Attach it to a route below." onDismiss={() => go({ tab: "authorization" })} testId="authorizer-banner" /> :
    s.done === "cors" ? <ResultBanner title="CORS configuration saved." returned="UpdateApi stored it on the API; preflight requests are answered with it at once." onDismiss={() => go({ tab: "cors" })} testId="cors-banner" /> : undefined;
  return (
    <div data-testid="http-page">
      <PageHeader
        title={section === "cors" ? "Configure CORS" : label}
        crumbs={[...back, { label, to: `/resources/apigateway/apis/${id}` }]}
        titleExtra={<NameChips name={api.name} kind={websocket ? "WebSocket" : "HTTP"} />}
        sub={section === "routes" ? <>API ID <span className="font-mono">{id}</span>. Created {when(api.createdDate)}.</> : (SUBS[section] ?? "")}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        actions={section === "routes" ? <Button onClick={() => go({ do: "create-route" })} data-testid="create-route-top"><PlusIcon /> Create</Button> : undefined}
      />
      {banner && <div className="mb-8">{banner}</div>}
      {section === "routes" && <HttpRoutes id={id} api={api} calls={calls} selected={s.key} />}
      {section === "integrations" && <HttpIntegrations id={id} api={api} selected={s.key} />}
      {section === "authorization" && <HttpAuthorization id={id} api={api} mode={s.mode} />}
      {section === "cors" && <HttpCors id={id} api={api} />}
      {section === "stages" && <HttpStages id={id} api={api} calls={calls} selected={s.name} />}
    </div>
  );
}

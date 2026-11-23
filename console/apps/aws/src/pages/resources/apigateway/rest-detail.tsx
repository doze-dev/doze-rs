// A REST API's page (apigateway__rest-resources, rest-stage, rest-authorizers and what they open): the header (the section as the title,
// the API as a chip, API actions and Deploy API), the chips between the sections, and the forms a section opens (`?do=`).
// `?tab=` resources | stages | authorizers; `?do=` create-resource, create-method, create-authorizer, method, create-stage.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { ChevronDownIcon, PlusIcon, UploadIcon } from "lucide-react";
import { lazy, Suspense, useState } from "react";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useResourceDetail } from "../../../api/resources";
import { DetailError, useServiceCrumbs } from "../../../kit";
import { ApiDelete } from "./api-delete";
import { useRefreshReads, useRestApi } from "./api";
import { Deploy } from "./deploy";
import { NameChips, useWhere } from "./parts";
import { RestResources } from "./rest-resources";
import { RestStages } from "./rest-stages";
import { RestAuthorizers } from "./rest-authorizers";
import { endpointWords, when } from "./words";

const ResourceCreate = lazy(() => import("./resource-create").then((m) => ({ default: m.ResourceCreate })));
const MethodCreate = lazy(() => import("./method-create").then((m) => ({ default: m.MethodCreate })));
const AuthorizerCreate = lazy(() => import("./rest-authorizer-create").then((m) => ({ default: m.AuthorizerCreate })));
const MethodPage = lazy(() => import("./method-page").then((m) => ({ default: m.MethodPage })));
const StageCreate = lazy(() => import("./stage-create").then((m) => ({ default: m.StageCreate })));

type Search = { tab?: string; do?: string; key?: string; sel?: string; mode?: string; done?: string; name?: string; dialog?: string };
const SECTIONS = [{ id: "resources", label: "Resources" }, { id: "stages", label: "Stages" }, { id: "authorizers", label: "Authorizers" }] as const;

export function RestDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const api = useRestApi(name);
  const refresh = useRefreshReads();
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={() => void detail.refetch()} retrying={detail.isFetching} />;
  if (!detail.data || !api.data) return <div className="grid gap-4" data-testid="resource-page-loading"><Skeleton className="h-12 w-80" /><Skeleton className="h-72 rounded-[14px]" /></div>;
  return <RestView id={name} apiName={api.data.name} created={api.data.createdDate} types={api.data.endpointConfiguration?.types} calls={detail.data.calls} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={() => { refresh(); void detail.refetch(); }} />;
}

function RestView({ id, apiName, created, types, calls, updatedAt, fetching, refresh }: { id: string; apiName: string; created?: number; types?: string[]; calls: import("../../../api/types/CallRecord").CallRecord[]; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("apigateway");
  const { region } = useWhere();
  const s = useSearch({ strict: false }) as Search;
  const [deleting, setDeleting] = useState(false);
  const section = SECTIONS.find((x) => x.id === s.tab)?.id ?? "resources";
  const go = (search: Search) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "restapis", name: id }, search: search as never });
  const back = [...crumbs, { label: "APIs", to: "/resources/apigateway/restapis" }, { label: apiName, to: `/resources/apigateway/restapis/${id}` }];
  const form = s.do ? (
    <Suspense fallback={<Skeleton className="h-96 rounded-[14px]" />}>
      {s.do === "create-resource" && <ResourceCreate api={id} apiName={apiName} parent={s.key} crumbs={back} onClose={(p) => go({ ...(p ? { done: "resource", name: p } : {}) })} />}
      {s.do === "create-method" && <MethodCreate api={id} apiName={apiName} resource={s.key} crumbs={back} onClose={(m, r) => go(m ? { done: "method", name: m, key: r } : { key: r })} />}
      {s.do === "create-authorizer" && <AuthorizerCreate api={id} apiName={apiName} crumbs={back} onClose={(a) => go({ tab: "authorizers", ...(a ? { done: "authorizer", name: a } : {}) })} />}
      {s.do === "create-stage" && <StageCreate api={id} apiName={apiName} crumbs={back} onClose={(n) => go({ tab: "stages", ...(n ? { done: "stage", name: n } : {}) })} />}
      {s.do === "method" && <MethodPage api={id} apiName={apiName} resource={s.key ?? ""} method={s.sel ?? ""} mode={s.mode} crumbs={back} calls={calls} go={(p) => go({ ...s, ...p })} />}
    </Suspense>
  ) : null;
  const deploy = <Deploy api={id} apiName={apiName} open={s.dialog === "deploy"} onOpenChange={(o) => { if (!o) go({ ...s, dialog: undefined }); }} onDone={(stage) => go({ tab: "stages", done: "deployed", name: stage })} />;
  if (form) return <>{form}{deploy}</>;
  const banner =
    s.done === "created" ? <ResultBanner title={`REST API ${apiName} created.`} returned="CreateRestApi returned its ID and the ID of its root resource." onDismiss={() => go({})} testId="created-banner" /> :
    s.done === "resource" ? <ResultBanner title={`Resource ${s.name} created.`} returned="CreateResource returned its ID and full path. Add a method to it next." onDismiss={() => go({})} testId="resource-banner" /> :
    s.done === "method" ? <ResultBanner title={`Method ${s.name} created.`} returned="PutMethod and PutIntegration were made. Deploy the API to put the change on a stage." onDismiss={() => go({ key: s.key })} testId="method-banner" /> :
    s.done === "authorizer" ? <ResultBanner title={`Authorizer ${s.name} created.`} returned="CreateAuthorizer returned its ID. Choose it under Authorization on a method request." onDismiss={() => go({ tab: "authorizers" })} testId="authorizer-banner" /> :
    s.done === "deployed" ? <ResultBanner title={`Deployed to ${s.name ?? "a stage"}.`} returned="CreateDeployment took a snapshot of the API, and the stage now serves it. A change made after this shows on the stage only when you deploy again." onDismiss={() => go({ tab: "stages", name: s.name })} testId="deployed-banner" /> :
    s.done === "stage" ? <ResultBanner title={`Stage ${s.name} created.`} returned="CreateStage pointed it at a deployment." onDismiss={() => go({ tab: "stages" })} testId="stage-banner" /> : undefined;
  const sub = <>API ID <span className="font-mono">{id}</span>. Created {when(created)}.</>;
  return (
    <div data-testid="rest-page">
      <PageHeader
        title={SECTIONS.find((x) => x.id === section)?.label ?? "Resources"}
        crumbs={[...back, { label: SECTIONS.find((x) => x.id === section)?.label ?? "Resources", to: `/resources/apigateway/restapis/${id}` }]}
        titleExtra={<NameChips name={apiName} kind="REST" extra={endpointWords(types?.[0])} />}
        sub={section === "stages" ? "A stage is a named reference to a deployment. Callers reach the API through the stage's URL." : section === "authorizers" ? "An authorizer decides, before a method runs, whether the caller may invoke it." : sub}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        actions={
          <>
            {section === "authorizers" ? (
              <Button onClick={() => go({ do: "create-authorizer" })} data-testid="create-authorizer-top"><PlusIcon /> Create authorizer</Button>
            ) : (
              <>
                {section === "resources" && (
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild><Button variant="outline" className="bg-card" data-testid="api-actions">API actions <ChevronDownIcon /></Button></DropdownMenuTrigger>
                    <DropdownMenuContent align="end" className="min-w-48">
                      <DropdownMenuItem onSelect={() => setDeleting(true)} data-testid="action-delete-api">Delete API</DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                )}
                <Button onClick={() => go({ ...s, dialog: "deploy" })} data-testid="deploy-open"><UploadIcon /> Deploy API</Button>
              </>
            )}
          </>
        }
      />
      {banner && <div className="mb-8">{banner}</div>}
      {section === "resources" && <RestResources id={id} name={apiName} selected={s.key} calls={calls} />}
      {section === "stages" && <RestStages id={id} name={apiName} stage={s.name} calls={calls} region={region} />}
      {section === "authorizers" && <RestAuthorizers id={id} name={apiName} />}
      {deploy}
      {deleting && <ApiDelete api={{ id, name: apiName, protocol: "REST" }} open onOpenChange={(o) => !o && setDeleting(false)} onDone={(n) => void navigate({ to: "/resources/$service/$second", params: { service: "apigateway", second: "restapis" }, search: { gone: n } })} />}
    </div>
  );
}


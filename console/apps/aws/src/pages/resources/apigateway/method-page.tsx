// A method's execution page (apigateway__rest-method-request, rest-integration-request, rest-integration-response, rest-method-response):
// the request path as a strip, and the four pages of a method (`?mode=` request | integration | iresponse | mresponse). The method is
// read with the rest of the API's tree, so what a page shows of it is one read.
import { Link } from "@tanstack/react-router";
import { UploadIcon } from "lucide-react";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { Button } from "@doze/ui/components/ui/button";
import { Badge } from "@doze/ui/components/ui/badge";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { SearchXIcon } from "lucide-react";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import type { CallRecord } from "../../../api/types/CallRecord";
import { FlowStrip, type FlowGroup } from "../../../kit";
import { useAuthorizers, useResources } from "./api";
import { IntegrationRequest } from "./integration-request";
import { IntegrationResponses } from "./integration-responses";
import { MethodRequest } from "./method-request";
import { MethodResponses } from "./method-responses";
import { Said } from "./parts";
import { backendName, integrationBadge, methodSentence } from "./words";

type Go = (search: Record<string, string | undefined>) => void;
const MODES = [{ id: "request", label: "Method request" }, { id: "integration", label: "Integration request" }, { id: "iresponse", label: "Integration response" }, { id: "mresponse", label: "Method response" }] as const;
const SUB: Record<string, string> = {
  request: "Method execution: a request enters at the method request and leaves by the integration request; the answer returns by the integration response and the method response.",
  integration: "The integration request is what the backend receives. It may differ from what the caller sent.",
  iresponse: "The integration response is what the backend returned, before it is shaped into the method response.",
  mresponse: "The method response is the contract with the caller: which status codes, headers and body shapes can come back.",
};

export function MethodPage({ api, apiName, resource, method, mode, crumbs, go }: { api: string; apiName: string; resource: string; method: string; mode: string | undefined; crumbs: { label: string; to: string }[]; calls: CallRecord[]; go: Go; done?: string; doneName?: string }) {
  const tree = useResources(api);
  const authorizers = useAuthorizers(api).data?.item ?? [];
  const current = MODES.find((m) => m.id === mode)?.id ?? "request";
  if (!tree.data) return <div className="grid gap-4" data-testid="resource-page-loading"><Skeleton className="h-12 w-80" /><Skeleton className="h-72 rounded-[var(--radius-card)]" /></div>;
  const res = tree.data.item?.find((r) => r.id === resource);
  const m = res?.resourceMethods?.[method];
  if (!res || !m) {
    return (
      <div data-testid="detail-error" data-missing="">
        <PageHeader title="Method not found" crumbs={[...crumbs, { label: "Resources", to: `/resources/apigateway/restapis/${api}` }]} />
        <EmptyState icon={SearchXIcon} title={`There is no ${method || "such"} method on ${res?.path ?? "that resource"}`} action={<Button asChild variant="outline" className="bg-card"><Link to="/resources/$service/$kind/$name" params={{ service: "apigateway", kind: "restapis", name: api }} search={{}}>Back to resources</Link></Button>}>
          <p>{apiName} has no such method. It may have been deleted; choose one from the resources.</p>
        </EmptyState>
      </div>
    );
  }
  const backend = backendName(m.methodIntegration);
  const response = current === "iresponse" || current === "mresponse";
  const at = (id: string) => ({ label: id === "request" ? "Method request" : id === "integration" ? "Integration request" : id === "iresponse" ? "Integration response" : "Method response", self: id === current, id });
  const stripe: FlowGroup[] = response
    ? current === "mresponse" ? [{ label: "Response path", chips: [at("iresponse")] }, { chips: [at("mresponse")] }, { chips: [{ label: "Client" }] }] : [{ label: "Response path", chips: [{ label: backend }] }, { chips: [at("iresponse")] }, { chips: [at("mresponse")] }, { chips: [{ label: "Client" }] }]
    : [{ label: "Request path", chips: [{ label: "Client" }] }, { chips: [at("request")] }, { chips: [at("integration")] }, { chips: [{ label: backend }] }];
  const authorizer = authorizers.find((a) => a.id === m.authorizerId)?.name;
  const here = `${m.httpMethod} ${res.path}`;
  return (
    <div data-testid="method-page">
      <PageHeader
        title={here}
        mono
        crumbs={[...crumbs, { label: here, to: `/resources/apigateway/restapis/${api}?do=method&key=${res.id}&sel=${m.httpMethod}` }, { label: MODES.find((x) => x.id === current)!.label, to: "#" }]}
        titleExtra={<Badge variant="secondary" className="font-normal" data-testid="integration-badge">{integrationBadge(m.methodIntegration)}</Badge>}
        sub={SUB[current]}
        actions={<Button onClick={() => go({ dialog: "deploy" })} data-testid="deploy-open"><UploadIcon /> Deploy API</Button>}
      />
      {current === "request" && <Said text={methodSentence(m, res.path, authorizer)} names={[m.httpMethod, res.path, backend, ...(Object.keys(m.requestParameters ?? {}).map((k) => k.split(".").pop() ?? ""))]} bold={[]} />}
      <FlowStrip groups={stripe} className="mb-10" />
      <Tabs value={current} onValueChange={(v) => go({ do: "method", key: res.id, sel: m.httpMethod, mode: v })} className="gap-0">
        <TabsList variant="line" className="h-11 w-full justify-start gap-4 rounded-none border-b p-0" aria-label={`${here} pages`}>
          {MODES.map((t) => <TabsTrigger key={t.id} value={t.id} className="h-11 flex-none px-2 text-[14.5px] after:bottom-[-1px]" data-testid={`tab-${t.id}`}>{t.label}</TabsTrigger>)}
        </TabsList>
      </Tabs>
      <div className="grid gap-6 pt-8" data-testid={`method-tab-${current}`}>
        {current === "request" && <MethodRequest api={api} apiName={apiName} method={m} resource={res} authorizers={authorizers} />}
        {current === "integration" && <IntegrationRequest api={api} resource={res.id} method={m} />}
        {current === "iresponse" && <IntegrationResponses api={api} resource={res.id} method={m} />}
        {current === "mresponse" && <MethodResponses api={api} resource={res.id} method={m} />}
      </div>
    </div>
  );
}

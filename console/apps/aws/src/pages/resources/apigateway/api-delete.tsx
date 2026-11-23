// Delete API (apigateway__rest-delete, http-delete): what goes with the API, counted from what it has now, and for a REST API the
// usage plans that name it. A REST API asks for `confirm` to be typed (the board's word); an HTTP API is a plain confirm.
import { useMutation } from "@tanstack/react-query";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { TypedConfirm } from "@doze/ui/components/app/typed-confirm";
import { client } from "../../../api/client";
import { RESOURCE_PATHS } from "../../../api/resources";
import { PlainConfirm } from "../../../kit";
import { useForget, useHttpAuthorizers, useHttpStages, useIntegrations, useResources, useRestPlans, useRoutes, useStages } from "./api";
import type { AnyApi } from "./types";
import { apiKind, count, list } from "./words";

/** The words of what goes with a REST API, from what it holds now. */
function useRestImpact(id: string, name: string, on: boolean): { words: React.ReactNode; plans: React.ReactNode } {
  const tree = useResources(id, on).data?.item ?? [];
  const stages = useStages(id, on).data?.item ?? [];
  const all = useRestPlans().data?.item ?? [];
  const methods = tree.reduce((n, r) => n + Object.keys(r.resourceMethods ?? {}).length, 0);
  const using = all.filter((p) => p.apiStages?.some((s) => s.apiId === id));
  return {
    words: (
      <span data-testid="delete-impact">Its <b className="font-semibold text-bright">{count(tree.length, "resource")}</b>, <b className="font-semibold text-bright">{count(methods, "method")}</b>{stages.length > 0 ? <> and {stages.length === 1 ? "the stage" : "the stages"} <b className="font-mono font-semibold text-bright">{list(stages.map((s) => s.stageName))}</b></> : " and deployments"} go with it{stages.length > 0 ? ", and its invoke URL stops answering" : ""}.</span>
    ),
    plans: using.length > 0 && (
      <p data-testid="delete-plans">The usage {using.length === 1 ? "plan" : "plans"} <b className="font-mono font-semibold text-bright">{list(using.map((p) => p.name))}</b> {using.length === 1 ? "lists" : "list"} <b className="font-mono font-semibold text-bright">{list(using.flatMap((p) => (p.apiStages ?? []).filter((s) => s.apiId === id).map((s) => `${name}:${s.stage}`)))}</b>. The plan keeps its keys; the stage leaves it.</p>
    ),
  };
}

/** What the delete cannot say: who called the API lately. A call to an API's address is a plain HTTP request with no AWS signature, and the Live record keeps
 * only the calls that are signed, so it holds none of them; the page says so rather than let a quiet record read as an API nobody calls. */
function Callers() {
  return <p className="mt-3 text-[13.5px] leading-[21px] text-muted-foreground" data-testid="delete-callers">Who called this API lately is not shown: requests to its address carry no AWS signature, so the Live record does not hold them, and a quiet record does not mean nobody calls it.</p>;
}

/** The words of what goes with an HTTP or WebSocket API. */
function useHttpImpact(id: string, on: boolean): React.ReactNode {
  const routes = useRoutes(id, on).data?.items ?? [];
  const integrations = useIntegrations(id, on).data?.items ?? [];
  const authorizers = useHttpAuthorizers(id, on).data?.items ?? [];
  const stages = useHttpStages(id, on).data?.items ?? [];
  const parts = [count(routes.length, "route"), ...(integrations.length > 0 ? ["the integrations behind them"] : []), ...(authorizers.length > 0 ? [authorizers.length === 1 ? `the ${authorizers[0]!.authorizerType === "JWT" ? "JWT" : "Lambda"} authorizer` : `the ${count(authorizers.length, "authorizer")}`] : []), ...(stages.length > 0 ? [`the ${list(stages.map((s) => s.stageName))} ${stages.length === 1 ? "stage" : "stages"}`] : [])];
  return <span data-testid="delete-impact">Its {list(parts)} go with it.</span>;
}

/** The delete of an API: the generic endpoint, then what was read of it is marked stale (not read again). */
export function ApiDelete({ api, open, onOpenChange, onDone }: { api: Pick<AnyApi, "id" | "name" | "protocol">; open: boolean; onOpenChange: (o: boolean) => void; onDone: (name: string) => void }) {
  const forget = useForget();
  const kind = apiKind(api.protocol);
  const del = useMutation({ mutationFn: () => client.api(RESOURCE_PATHS.one("apigateway", kind, api.id), { method: "DELETE" }) });
  const rest = useRestImpact(api.id, api.name, api.protocol === "REST");
  const http = useHttpImpact(api.id, api.protocol !== "REST");
  const words = (
    <>
      Delete <b className="font-mono font-semibold text-bright">{api.name}</b> (<span className="font-mono">{api.id}</span>)? {api.protocol === "REST" ? rest.words : http}
    </>
  );
  const confirm = () => del.mutate(undefined, { onSuccess: () => { forget(api.id); onDone(api.name); } });
  const failed = del.error ? errorWords(del.error) : null;
  if (api.protocol === "REST") {
    return (
      <TypedConfirm
        open={open}
        onOpenChange={(o) => { onOpenChange(o); if (!o) del.reset(); }}
        title="Delete API"
        description={words}
        words="confirm"
        prompt={<>Enter <span className="font-mono font-semibold">confirm</span> to delete the API</>}
        verb="Delete"
        pending={del.isPending}
        refusal={failed && <Alert tone="bad" title={`DeleteRestApi was refused${failed.code ? `: ${failed.code}` : ""}`}>{failed.message}</Alert>}
        onConfirm={confirm}
        testId="api-delete"
      >
        <div className="text-[13.5px] leading-[21px] text-foreground">{rest.plans}</div>
        <Callers />
      </TypedConfirm>
    );
  }
  return (
    <PlainConfirm open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) del.reset(); }} title="Delete API" description={words} call="DeleteApi" pending={del.isPending} error={del.error} onConfirm={confirm} testId="api-delete">
      <Callers />
    </PlainConfirm>
  );
}

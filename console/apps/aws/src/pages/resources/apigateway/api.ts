// The calls the API Gateway pages make: each is an operation the kind allows (`ops` in crates/aws-console/src/resources/apigateway/),
// made through the console as the caller `console`. The REST service takes its members as the model names them (`restApiId`); the v2
// one takes its path members capitalised (`ApiId`, `RouteId`) and writes its bodies in camelCase. Reads are made when a page opens
// and when the person asks.
import { useQueryClient } from "@tanstack/react-query";
import { type Kind, callKindOp, useKindOp, useKindOpRead } from "../../../kit";
import type { Authorizers, Deployments, HttpApi, HttpApis, HttpAuthorizers, HttpDeployments, HttpIntegrations, HttpStages, Resources, RestApi, RestApis, Routes, Stages, UsagePlans, ApiKeys, PlanKeys } from "./types";

export const REST: Kind = { service: "apigateway", kind: "restapis" };
export const HTTP: Kind = { service: "apigateway", kind: "apis" };
export const PLANS: Kind = { service: "apigateway", kind: "usageplans" };

export const useRestApis = () => useKindOpRead<RestApis>(REST, "GetRestApis", { limit: 500 });
export const useHttpApis = () => useKindOpRead<HttpApis>(HTTP, "GetApis", { MaxResults: "500" });
export const useRestApi = (id: string) => useKindOpRead<RestApi>(REST, "GetRestApi", { restApiId: id });
export const useHttpApi = (id: string) => useKindOpRead<HttpApi>(HTTP, "GetApi", { ApiId: id });

/** The whole tree of a REST API in one read, each resource with its methods (and their integrations and responses). */
export const useResources = (id: string) => useKindOpRead<Resources>(REST, "GetResources", { restApiId: id, embed: ["methods"], limit: 500 });
export const useStages = (id: string) => useKindOpRead<Stages>(REST, "GetStages", { restApiId: id });
export const useDeployments = (id: string) => useKindOpRead<Deployments>(REST, "GetDeployments", { restApiId: id, limit: 500 });
export const useAuthorizers = (id: string) => useKindOpRead<Authorizers>(REST, "GetAuthorizers", { restApiId: id, limit: 500 });
/** The plans of the account, for what names an API (a delete says which plan covers it). */
export const useRestPlans = () => useKindOpRead<UsagePlans>(REST, "GetUsagePlans", { limit: 500 });

export const useRoutes = (id: string) => useKindOpRead<Routes>(HTTP, "GetRoutes", { ApiId: id });
export const useIntegrations = (id: string) => useKindOpRead<HttpIntegrations>(HTTP, "GetIntegrations", { ApiId: id });
export const useHttpAuthorizers = (id: string) => useKindOpRead<HttpAuthorizers>(HTTP, "GetAuthorizers", { ApiId: id });
export const useHttpStages = (id: string) => useKindOpRead<HttpStages>(HTTP, "GetStages", { ApiId: id });
export const useHttpDeployments = (id: string) => useKindOpRead<HttpDeployments>(HTTP, "GetDeployments", { ApiId: id });

export const usePlans = () => useKindOpRead<UsagePlans>(PLANS, "GetUsagePlans", { limit: 500 });
export const usePlanKeys = (id: string) => useKindOpRead<PlanKeys>(PLANS, "GetUsagePlanKeys", { usagePlanId: id, limit: 500 });
export const useApiKeys = (enabled = true) => useKindOpRead<ApiKeys>(PLANS, "GetApiKeys", { limit: 500, includeValues: false }, enabled);

/** A write: what the pages read of API Gateway (the lists, the pickers, an API's page) is stale after it. */
export function useWrite<T = unknown, I extends object = object>(k: Kind, operation: string) {
  const qc = useQueryClient();
  const op = useKindOp<T, I>(k, operation);
  return {
    ...op,
    mutate: ((input: I, options?: Parameters<typeof op.mutate>[1]) =>
      op.mutate(input, {
        ...options,
        onSuccess: (...args) => {
          void qc.invalidateQueries({ queryKey: ["resources", "kindop", "apigateway"] });
          void qc.invalidateQueries({ queryKey: ["resources", "one", "apigateway"] });
          options?.onSuccess?.(...args);
        },
      })) as typeof op.mutate,
  };
}

/** What the pages read of API Gateway is stale (a call made with `callKindOp`, or a delete the generic endpoint made). */
export function useRefreshReads(): () => void {
  const qc = useQueryClient();
  return () => {
    void qc.invalidateQueries({ queryKey: ["resources", "kindop", "apigateway"] });
    void qc.invalidateQueries({ queryKey: ["resources", "one", "apigateway"] });
  };
}

/** After a delete: what was read about `id` is marked stale and not read again (the page is still on screen until the list opens,
 * and a read of a deleted API is a refusal the browser logs). */
export function useForget(): (id: string) => void {
  const qc = useQueryClient();
  return (id) => {
    void qc.invalidateQueries({ predicate: (q) => q.queryKey[0] === "resources" && JSON.stringify(q.queryKey).includes(id), refetchType: "none" });
    void qc.invalidateQueries({ queryKey: ["resources", "list", "apigateway"] });
    void qc.invalidateQueries({ queryKey: ["resources", "index"] });
  };
}

/** A patch of the API's own: `{op, path, value}`, how REST edits what it has made. */
export type Patch = { op: "add" | "remove" | "replace"; path: string; value?: string };

/** Makes the calls of a form one after the other, as its panel lists them; stops at the first the service refuses (the ones before it were made). */
export async function runCalls(k: Kind, calls: readonly { operation: string; input: object }[], onStep?: (operation: string) => void): Promise<void> {
  for (const c of calls) {
    onStep?.(c.operation);
    await callKindOp(k, c.operation, c.input);
  }
}

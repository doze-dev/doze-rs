// What a delete of a part of an API says, from what the API holds now: which resources and methods go with a resource, which stage
// holds a deployment, which usage plan lists a stage. Pure, so it is tested without a browser.
import type { RestMethod, RestResource, UsagePlan } from "./types";
import { count, list } from "./words";

/** The resource and everything below it (a child's `parentId` is its parent's id), the resource first. */
export function subtree(resources: readonly RestResource[], id: string): RestResource[] {
  const out: RestResource[] = [];
  const seen = new Set<string>();
  const walk = (at: string) => {
    if (seen.has(at)) return;
    seen.add(at);
    const r = resources.find((x) => x.id === at);
    if (r) out.push(r);
    for (const c of resources) if (c.parentId === at) walk(c.id);
  };
  walk(id);
  return out;
}

const methodsIn = (rs: readonly RestResource[]): string[] => rs.flatMap((r) => Object.keys(r.resourceMethods ?? {}).map((m) => `${m} ${r.path}`));

/** What deleting a resource takes with it: the resources below it and every method of it and of them. `null` for the root, which cannot be deleted. */
export function resourceImpact(resources: readonly RestResource[], id: string): { children: string[]; methods: string[]; words: string } | null {
  const mine = resources.find((r) => r.id === id);
  if (!mine || !mine.parentId) return null;
  const all = subtree(resources, id);
  const children = all.slice(1).map((r) => r.path);
  const methods = methodsIn(all);
  const parts = [children.length > 0 ? `its ${count(children.length, "child resource")} (${list(children)})` : "", methods.length > 0 ? `${children.length > 0 && methods.length > 1 ? "all " : "its "}${count(methods.length, "method")} (${list(methods)})` : ""].filter(Boolean);
  return { children, methods, words: parts.length === 0 ? "It has no method and no child resource." : `${parts.join(" and ")} go${parts.length === 1 && children.length + methods.length === 1 ? "es" : ""} with it.`.replace(/^its/, "Its") };
}

/** What deleting a method takes with it: its integration (and the integration's responses) and its method responses. */
export function methodImpact(m: Pick<RestMethod, "methodIntegration" | "methodResponses">): string {
  const integration = m.methodIntegration ? (Object.keys(m.methodIntegration.integrationResponses ?? {}).length > 0 ? "its integration with its responses" : "its integration") : "";
  const n = Object.keys(m.methodResponses ?? {}).length;
  const responses = n > 0 ? `its ${count(n, "method response")}` : "";
  const parts = [integration, responses].filter(Boolean);
  return parts.length === 0 ? "It has no integration and no method response." : `${parts.join(" and ").replace(/^its/, "Its")} ${parts.length === 1 && (integration !== "" || n === 1) ? "goes" : "go"} with it.`;
}

/** The stages of an API (REST or HTTP) that serve this deployment: the service refuses to delete a deployment while one does. */
export const stagesServing = (stages: readonly { stageName: string; deploymentId?: string }[], deploymentId: string): string[] => stages.filter((s) => s.deploymentId === deploymentId).map((s) => s.stageName);

/** The words that say why a deployment cannot be deleted yet, or `null` when no stage serves it. */
export function deploymentBlocker(stages: readonly string[]): string | null {
  if (stages.length === 0) return null;
  return `${list(stages)} ${stages.length === 1 ? "serves" : "serve"} this deployment, and the service refuses to delete one a stage serves. Deploy the API again so ${stages.length === 1 ? "the stage points" : "the stages point"} at a newer one, or delete ${stages.length === 1 ? "the stage" : "the stages"} first.`;
}

/** The usage plans that list `api:stage` (a delete of the stage leaves them listing it: the service does not touch plans). */
export const plansListing = (plans: readonly Pick<UsagePlan, "name" | "apiStages">[], api: string, stage: string): string[] => plans.filter((p) => p.apiStages?.some((s) => s.apiId === api && s.stage === stage)).map((p) => p.name);

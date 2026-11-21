// A resource's form and the calls it makes: CreateResource, and with CORS on the four calls of the OPTIONS method that answers a
// preflight (a mock integration, its two responses, and the headers the browser asks about).
import { pathPartProblem } from "./words";

export type ResourceForm = { proxy: boolean; parent: string; name: string; cors: boolean };

export const proxyName = "{proxy+}";
export const partOf = (f: ResourceForm): string => (f.proxy ? proxyName : f.name);

export function resourceProblem(f: ResourceForm): string | null {
  if (!f.parent) return "Choose where the resource goes.";
  return pathPartProblem(partOf(f));
}

export const createResourceInput = (api: string, f: ResourceForm) => ({ restApiId: api, parentId: f.parent || "<parent>", pathPart: partOf(f) || "<name>" });

/** The path the new resource will have. */
export const fullPath = (parentPath: string, part: string): string => `${parentPath === "/" ? "" : parentPath}/${part || "<name>"}`;

/** What a browser asks of a preflight, answered by the OPTIONS method's integration response. */
export const CORS_HEADERS = { "method.response.header.Access-Control-Allow-Headers": "'Content-Type,Authorization,X-Api-Key'", "method.response.header.Access-Control-Allow-Methods": "'GET,POST,PUT,PATCH,DELETE,OPTIONS'", "method.response.header.Access-Control-Allow-Origin": "'*'" };

/** The calls that make the OPTIONS method of `resource`, in order. */
export function corsCalls(api: string, resource: string): { operation: string; input: Record<string, unknown> }[] {
  const at = { restApiId: api, resourceId: resource };
  return [
    { operation: "PutMethod", input: { ...at, httpMethod: "OPTIONS", authorizationType: "NONE" } },
    { operation: "PutIntegration", input: { ...at, requestHttpMethod: "OPTIONS", type: "MOCK", requestTemplates: { "application/json": '{"statusCode": 200}' } } },
    { operation: "PutMethodResponse", input: { ...at, httpMethod: "OPTIONS", statusCode: "200", responseParameters: Object.fromEntries(Object.keys(CORS_HEADERS).map((k) => [k, false])) } },
    { operation: "PutIntegrationResponse", input: { ...at, httpMethod: "OPTIONS", statusCode: "200", responseParameters: CORS_HEADERS } },
  ];
}

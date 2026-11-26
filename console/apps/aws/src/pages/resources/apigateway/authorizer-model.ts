// An authorizer's form and the call it makes, for a REST API (CreateAuthorizer of `apigateway`) and an HTTP API (`apigatewayv2`).
import type { HttpAuthorizer, Route } from "./types";
import { count, lambdaName, restLambdaUri } from "./words";

export type RestAuthorizerForm = {
  name: string;
  kind: "lambda" | "cognito";
  fn: string | null;
  role: string | null;
  payload: "TOKEN" | "REQUEST";
  source: string;
  validation: string;
  cache: boolean;
  ttl: string;
  pools: string;
};
export const emptyRest: RestAuthorizerForm = { name: "", kind: "lambda", fn: null, role: null, payload: "TOKEN", source: "Authorization", validation: "", cache: true, ttl: "300", pools: "" };

const ttlOk = (t: string) => /^\d+$/.test(t) && Number(t) <= 3600;

export function restAuthorizerProblem(f: RestAuthorizerForm): string | null {
  if (f.name.trim() === "") return "Enter an authorizer name.";
  if (f.kind === "lambda" && !f.fn) return "Choose the Lambda function.";
  if (f.kind === "cognito" && f.pools.trim() === "") return "Enter the ARN of a user pool.";
  if (f.source.trim() === "") return f.payload === "TOKEN" || f.kind === "cognito" ? "Enter the header that carries the token." : "Enter at least one identity source.";
  if (f.validation !== "") {
    try { new RegExp(f.validation); } catch { return "The token validation is not a regular expression."; }
  }
  if (f.cache && !ttlOk(f.ttl)) return "The cache TTL is 0 to 3,600 seconds.";
  return null;
}

/** A header name (or an identity-source list) as the service takes it: `Authorization` is `method.request.header.Authorization`. */
export const identitySource = (source: string, payload: "TOKEN" | "REQUEST"): string =>
  payload === "TOKEN" ? (source.startsWith("method.request.") ? source : `method.request.header.${source.trim()}`) : source.split(",").map((s) => s.trim()).filter(Boolean).map((s) => (s.startsWith("method.request.") || s.startsWith("context.") || s.startsWith("stageVariables.") ? s : `method.request.header.${s}`)).join(",");

export function restAuthorizerInput(f: RestAuthorizerForm, o: { api: string; region: string; account: string }): Record<string, unknown> {
  const lambda = f.kind === "lambda";
  const type = lambda ? f.payload : "COGNITO_USER_POOLS";
  return {
    restApiId: o.api,
    name: f.name || "<name>",
    type,
    ...(lambda ? { authorizerUri: restLambdaUri(o.region, f.fn ? `arn:aws:lambda:${o.region}:${o.account}:function:${f.fn}` : "<function>"), ...(f.role?.trim() ? { authorizerCredentials: f.role.trim().startsWith("arn:") ? f.role.trim() : `arn:aws:iam::${o.account}:role/${f.role.trim()}` } : {}) } : { providerARNs: f.pools.split(",").map((s) => s.trim()).filter(Boolean) }),
    identitySource: identitySource(f.source, lambda ? f.payload : "TOKEN"),
    ...(f.validation ? { identityValidationExpression: f.validation } : {}),
    ...(f.cache && ttlOk(f.ttl) ? { authorizerResultTtlInSeconds: Number(f.ttl) } : { authorizerResultTtlInSeconds: 0 }),
  };
}

export type HttpAuthorizerForm = {
  name: string;
  kind: "lambda" | "jwt";
  fn: string | null;
  version: "2.0" | "1.0";
  simple: boolean;
  sources: string[];
  ttl: string;
  issuer: string;
  audience: string;
};
export const emptyHttp: HttpAuthorizerForm = { name: "", kind: "lambda", fn: null, version: "2.0", simple: true, sources: ["$request.header.Authorization"], ttl: "300", issuer: "", audience: "" };

export function httpAuthorizerProblem(f: HttpAuthorizerForm): string | null {
  if (f.name.trim() === "") return "Enter an authorizer name.";
  const sources = f.sources.filter((s) => s.trim() !== "");
  if (f.kind === "lambda") {
    if (!f.fn) return "Choose the Lambda function.";
    if (!/^\d+$/.test(f.ttl) || Number(f.ttl) > 3600) return "The result TTL is 0 to 3,600 seconds.";
    if (Number(f.ttl) > 0 && sources.length === 0) return "Caching needs at least one identity source.";
    return null;
  }
  if (!/^https?:\/\//.test(f.issuer.trim())) return "The issuer is a URL, such as https://idp.example.com/.";
  if (f.audience.split(",").every((a) => a.trim() === "")) return "Enter at least one audience.";
  if (sources.length === 0) return "Enter the identity source that carries the token.";
  return null;
}

export function httpAuthorizerInput(f: HttpAuthorizerForm, o: { api: string; region: string; account: string }): Record<string, unknown> {
  const sources = f.sources.map((s) => s.trim()).filter(Boolean);
  if (f.kind === "jwt") {
    return { ApiId: o.api, name: f.name || "<name>", authorizerType: "JWT", identitySource: sources, jwtConfiguration: { issuer: f.issuer, audience: f.audience.split(",").map((a) => a.trim()).filter(Boolean) } };
  }
  return {
    ApiId: o.api,
    name: f.name || "<name>",
    authorizerType: "REQUEST",
    authorizerUri: restLambdaUri(o.region, f.fn ? `arn:aws:lambda:${o.region}:${o.account}:function:${f.fn}` : "<function>"),
    authorizerPayloadFormatVersion: f.version,
    ...(f.version === "2.0" ? { enableSimpleResponses: f.simple } : {}),
    identitySource: sources,
    authorizerResultTtlInSeconds: /^\d+$/.test(f.ttl) ? Number(f.ttl) : 0,
  };
}

/** What the Lambda function hint says of the request authorizers the API already has: each one, how many routes name it, and the function it
 * calls (`admin-request-auth, already on 2 routes, calls the function admin-auth.`). `null` when there is none. */
export function existingAuthorizerWords(authorizers: readonly Pick<HttpAuthorizer, "authorizerId" | "name" | "authorizerType" | "authorizerUri">[], routes: readonly Pick<Route, "authorizerId">[]): string | null {
  const lines = authorizers.flatMap((a) => {
    const fn = lambdaName(a.authorizerUri);
    if (a.authorizerType !== "REQUEST" || !fn) return [];
    const n = routes.filter((r) => r.authorizerId === a.authorizerId).length;
    return [`${a.name}, ${n === 0 ? "on no route yet" : `already on ${count(n, "route")}`}, calls the function ${fn}.`];
  });
  return lines.length === 0 ? null : lines.join(" ");
}

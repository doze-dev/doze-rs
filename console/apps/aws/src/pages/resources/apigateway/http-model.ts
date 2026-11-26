// The forms of an HTTP API and the calls they make: an integration, a route, the four calls of the create wizard, and the CORS
// configuration (a part of the API itself, set with UpdateApi). This service takes its path members capitalised (`ApiId`) and writes its
// bodies in camelCase.
import type { Cors, HttpApi, Route } from "./types";
import { restLambdaUri } from "./words";

export const ROUTE_METHODS = ["ANY", "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] as const;
export type Ctx = { region: string; account: string };

export type IntegrationForm = { kind: "lambda" | "http"; fn: string | null; url: string; method: string; version: "2.0" | "1.0" };
export const emptyIntegration: IntegrationForm = { kind: "lambda", fn: null, url: "", method: "ANY", version: "2.0" };

export const lambdaArn = (c: Ctx, fn: string): string => `arn:aws:lambda:${c.region}:${c.account}:function:${fn}`;

export function integrationProblem(f: IntegrationForm): string | null {
  if (f.kind === "lambda") return f.fn ? null : "Choose the Lambda function.";
  return /^https?:\/\/\S+$/.test(f.url.trim()) ? null : "Enter the endpoint URL, starting with https:// or http://.";
}

/** A Lambda integration is a proxy integration (`AWS_PROXY`) of payload version 2.0 unless told otherwise; an HTTP one is `HTTP_PROXY`. */
export function integrationInput(api: string, f: IntegrationForm, c: Ctx): Record<string, unknown> {
  if (f.kind === "lambda") return { ApiId: api, integrationType: "AWS_PROXY", integrationUri: f.fn ? lambdaArn(c, f.fn) : "<function>", payloadFormatVersion: f.version };
  return { ApiId: api, integrationType: "HTTP_PROXY", integrationMethod: f.method, integrationUri: f.url.trim() || "<endpoint URL>", payloadFormatVersion: "1.0" };
}

/** `GET /reports`, or a bare key (`$default`, `$connect`) for a WebSocket API. */
export const routeKeyOf = (method: string, path: string): string => (method === "" ? path.trim() : `${method} ${path.trim()}`);
export function routeProblem(method: string, path: string, taken: readonly string[], websocket: boolean): string | null {
  const key = websocket ? path.trim() : routeKeyOf(method, path);
  if (websocket) return key === "" ? "Enter the route key, such as $default or sendMessage." : taken.includes(key) ? `The API already has the route ${key}.` : null;
  if (!path.trim().startsWith("/") && path.trim() !== "$default") return "A path starts with /, such as /reports.";
  if (/\s/.test(path.trim())) return "A path has no spaces.";
  return taken.includes(key) ? `The API already has the route ${key}.` : null;
}
export const routeInput = (api: string, key: string, integration: string | null): Record<string, unknown> => ({ ApiId: api, routeKey: key, ...(integration ? { target: `integrations/${integration}` } : {}) });

/** One backend of the wizard: an integration, and the route that sends requests to it (the wizard makes one route for each integration). */
export type Backend = { integration: IntegrationForm; method: string; path: string };
export const emptyBackend = (): Backend => ({ integration: emptyIntegration, method: "ANY", path: "" });
export type Wizard = { name: string; ip: "ipv4" | "dualstack"; backends: Backend[]; stage: string; auto: boolean };
export const emptyWizard: Wizard = { name: "", ip: "ipv4", backends: [emptyBackend()], stage: "$default", auto: true };
/** The path the first route takes by default: the function's name, or the endpoint's last part. */
export const defaultPath = (i: IntegrationForm): string => (i.kind === "lambda" ? (i.fn ? `/${i.fn}` : "") : "/");
/** The path a backend's route has: what was typed, else the default. */
export const pathOf = (b: Backend): string => b.path || defaultPath(b.integration);
export const keyOfBackend = (b: Backend): string => routeKeyOf(b.method, pathOf(b));

/** The calls of the wizard, in order: the API, then for each backend its integration and its route, then the stage; the later ones carry the ids the earlier ones
 * return (`<API ID>`, `<integration ID>` until they are made; `ids.integrations` has one for each backend made so far). */
export function wizardCalls(w: Wizard, c: Ctx, ids: { api?: string; integrations?: readonly string[] } = {}): { operation: string; input: Record<string, unknown> }[] {
  const api = ids.api ?? "<API ID>";
  const many = w.backends.length > 1;
  return [
    { operation: "CreateApi", input: { name: w.name || "<name>", protocolType: "HTTP", ...(w.ip === "dualstack" ? { ipAddressType: "dualstack" } : {}) } },
    ...w.backends.flatMap((b, i) => [
      { operation: "CreateIntegration", input: integrationInput(api, b.integration, c) },
      { operation: "CreateRoute", input: { ApiId: api, routeKey: keyOfBackend(b), target: `integrations/${ids.integrations?.[i] ?? (many ? `<integration ${i + 1} ID>` : "<integration ID>")}` } },
    ]),
    { operation: "CreateStage", input: { ApiId: api, stageName: w.stage, autoDeploy: w.auto } },
  ];
}
/** What is missing at a step: the name and each integration, then each route (a key is used once), then the stage. */
export function wizardProblem(step: number, w: Wizard, taken: readonly string[]): string | null {
  const of = (i: number, problem: string | null) => (problem && w.backends.length > 1 ? `Integration ${i + 1}: ${problem}` : problem);
  if (step === 0) return (w.name.trim() === "" ? "Enter an API name." : w.name.length > 128 ? "An API name is at most 128 characters." : null) ?? w.backends.map((b, i) => of(i, integrationProblem(b.integration))).find(Boolean) ?? null;
  if (step === 1) {
    const keys = w.backends.map(keyOfBackend);
    return w.backends.map((b, i) => of(i, routeProblem(b.method, pathOf(b), keys.filter((_, j) => j < i), false))).find(Boolean) ?? null;
  }
  if (step === 2) return /^(\$default|[A-Za-z0-9_-]{1,128})$/.test(w.stage) ? null : "A stage name is $default, or letters, digits, hyphens and underscores.";
  void taken;
  return null;
}

export type CorsForm = { origins: string[]; headers: string[]; methods: string[]; expose: string[]; maxAge: string; credentials: boolean };
export const corsOf = (a: Pick<HttpApi, "corsConfiguration"> | undefined): CorsForm => {
  const c = a?.corsConfiguration;
  return { origins: c?.allowOrigins ?? [], headers: c?.allowHeaders ?? [], methods: c?.allowMethods ?? [], expose: c?.exposeHeaders ?? [], maxAge: c?.maxAge === undefined ? "" : String(c.maxAge), credentials: Boolean(c?.allowCredentials) };
};
const filled = (l: readonly string[]) => l.map((s) => s.trim()).filter(Boolean);
export function corsProblem(f: CorsForm): string | null {
  if (f.maxAge !== "" && !(/^-?\d+$/.test(f.maxAge) && Number(f.maxAge) >= -1 && Number(f.maxAge) <= 86400)) return "Max age is -1 to 86,400 seconds.";
  if (f.credentials && filled(f.origins).includes("*")) return "Browsers refuse a * origin when credentials are allowed: name each origin.";
  return null;
}
/** The configuration as UpdateApi takes it; every list the form leaves empty is left out. */
export function corsConfiguration(f: CorsForm): Cors {
  const c: Cors = {};
  if (filled(f.origins).length > 0) c.allowOrigins = filled(f.origins);
  if (filled(f.headers).length > 0) c.allowHeaders = filled(f.headers);
  if (filled(f.methods).length > 0) c.allowMethods = filled(f.methods);
  if (filled(f.expose).length > 0) c.exposeHeaders = filled(f.expose);
  if (f.maxAge !== "") c.maxAge = Number(f.maxAge);
  c.allowCredentials = f.credentials;
  return c;
}
export const corsInput = (api: string, f: CorsForm) => ({ ApiId: api, corsConfiguration: corsConfiguration(f) });
/** A route with the greedy variable replaced by a sample, for the sentence that says where a request would land. */
export const sampleOf = (key: string): string => key.split(" ").slice(-1)[0]!.replace(/\{[^}+]+\+\}/g, "12").replace(/\{[^}]+\}/g, "12");
export const methodOfKey = (r: Pick<Route, "routeKey">): string => (r.routeKey.includes(" ") ? r.routeKey.split(" ")[0]! : "ANY");
/** The routes another API already has, all of them (up to four, then how many more): `GET /reports, POST /reports/run and ANY /orders/{proxy+}`. */
export function routeKeysWords(keys: readonly string[]): string {
  const shown = keys.slice(0, 4);
  const more = keys.length - shown.length;
  const head = shown.length <= 1 ? shown.join("") : `${shown.slice(0, -1).join(", ")}${more > 0 ? ", " : " and "}${shown[shown.length - 1]}`;
  return more > 0 ? `${head} and ${more} more` : head;
}
export { restLambdaUri };

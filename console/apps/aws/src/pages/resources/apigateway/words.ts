// What the API Gateway pages say, as pure functions: the one list of both kinds of API, the sentences over the lists, the URLs a
// stage answers at, and how an integration, a throttle and a quota read in a table.
import { dateTimeUtc } from "@doze/ui/lib/format";
import type { AnyApi, Deployment, HttpApi, HttpDeployment, HttpStage, Integration, RestApi, RestMethod, RestResource, Stage, UsagePlan } from "./types";
import { pluralWord } from "@doze/ui/lib/format";

/** `1 route`, `3 routes`. */
export const count = (n: number, one: string, many = pluralWord(one)): string => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;
/** `a`, `a and b`, `a, b and c`. */
export const list = (items: readonly string[]): string => (items.length <= 1 ? items.join("") : `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`);
/** Seconds since 1970 (REST) or ISO text (v2) as the console writes a time; a dash for none. */
export const when = (t: number | string | undefined | null): string => {
  if (t === undefined || t === null || t === "") return "–";
  const ms = typeof t === "number" ? t * 1000 : Date.parse(t);
  return Number.isNaN(ms) ? "–" : dateTimeUtc(ms);
};
const ms = (t: number | string | undefined | null): number => (t === undefined || t === null ? 0 : typeof t === "number" ? t * 1000 : Date.parse(t) || 0);

/** `REGIONAL` as the console words it. */
export const endpointWords = (type: string | undefined): string => ({ REGIONAL: "Regional", EDGE: "Edge-optimized", PRIVATE: "Private" })[type ?? ""] ?? (type ? type.charAt(0) + type.slice(1).toLowerCase() : "–");

/** Both kinds of API as one list, oldest first (the order the account made them in). */
export function anyApis(rest: readonly RestApi[], http: readonly HttpApi[]): AnyApi[] {
  const r: AnyApi[] = rest.map((a) => ({ id: a.id, name: a.name, description: a.description ?? "", protocol: "REST", endpointType: endpointWords(a.endpointConfiguration?.types?.[0]), created: a.createdDate ? a.createdDate * 1000 : null }));
  const h: AnyApi[] = http.map((a) => ({ id: a.apiId, name: a.name, description: a.description ?? "", protocol: a.protocolType === "WEBSOCKET" ? "WEBSOCKET" : "HTTP", endpointType: "Regional", created: a.createdDate ? Date.parse(a.createdDate) : null }));
  return [...r, ...h].sort((a, b) => (a.created ?? 0) - (b.created ?? 0));
}

/** The address of an API's page: REST APIs and the v2 service's are two kinds. */
export const apiKind = (protocol: AnyApi["protocol"]): "restapis" | "apis" => (protocol === "REST" ? "restapis" : "apis");

/** The function in an integration's URI: the ARN itself (a v2 one) or inside the invocation path (a REST one). */
export function lambdaName(uri: string | undefined): string | null {
  const m = /function:([A-Za-z0-9_-]+)/.exec(uri ?? "");
  return m?.[1] ?? null;
}
/** The ARN of the function an integration calls. */
export function lambdaArnOf(uri: string | undefined): string | null {
  const m = /(arn:aws:lambda:[^/]+:function:[A-Za-z0-9_:-]+)/.exec(uri ?? "");
  return m?.[1] ?? null;
}
/** The invocation URI of a REST integration with a function. */
export const restLambdaUri = (region: string, functionArn: string): string => `arn:aws:apigateway:${region}:lambda:path/2015-03-31/functions/${functionArn}/invocations`;

/** What an integration of a REST method is, as the Methods table says it. */
export function integrationKind(i: Integration | undefined): string {
  if (!i) return "None";
  if (i.type === "MOCK") return "Mock";
  if (i.type === "HTTP" || i.type === "HTTP_PROXY") return "HTTP";
  if (lambdaName(i.uri)) return "Lambda function";
  return i.type === "AWS" || i.type === "AWS_PROXY" ? "AWS service" : i.type;
}
/** The words under a method's name: `HTTP integration`, `Lambda function integration`, `Mock integration`. */
export const integrationBadge = (i: Integration | undefined): string => (i ? `${integrationKind(i)} integration` : "No integration");

/** What an HTTP API integration is: its type and what it calls. */
export function httpTarget(type: string, uri: string | undefined): string {
  const fn = lambdaName(uri);
  if (fn) return `${fn} Lambda`;
  return uri ?? type;
}

export const authorizationWords = (t: string | undefined): string => ({ NONE: "None", AWS_IAM: "IAM", CUSTOM: "Lambda authorizer", COGNITO_USER_POOLS: "Cognito", JWT: "JWT" })[t ?? "NONE"] ?? (t ?? "None");

/** A REST resource tree, in the order a person reads it: each path, its children after it, each with its depth. */
export function flattenTree(resources: readonly RestResource[]): { resource: RestResource; depth: number }[] {
  const kids = new Map<string, RestResource[]>();
  for (const r of resources) if (r.parentId) kids.set(r.parentId, [...(kids.get(r.parentId) ?? []), r]);
  const out: { resource: RestResource; depth: number }[] = [];
  const walk = (r: RestResource, depth: number) => {
    out.push({ resource: r, depth });
    for (const k of [...(kids.get(r.id) ?? [])].sort((a, b) => a.path.localeCompare(b.path))) walk(k, depth + 1);
  };
  for (const r of resources.filter((x) => !x.parentId)) walk(r, 0);
  return out;
}

const METHOD_ORDER = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "ANY"];
/** A resource's methods in the order a person reads them (GET first, OPTIONS last). */
export const methodsOf = (r: RestResource): RestMethod[] =>
  Object.values(r.resourceMethods ?? {}).sort((a, b) => METHOD_ORDER.indexOf(a.httpMethod) - METHOD_ORDER.indexOf(b.httpMethod) || a.httpMethod.localeCompare(b.httpMethod));

/** The stage a REST deployment is named by (the stage that points at it), for the sentence. */
export function stageOfDeployment(stages: readonly Stage[], id: string): Stage | undefined {
  return stages.find((s) => s.deploymentId === id);
}

/** `larder-public has 3 resources and 3 methods, and one stage: prod, pointing at deployment d4h7n2 from <time>.` */
export function restSentence(name: string, resources: readonly RestResource[], stages: readonly Stage[], deployments: readonly Deployment[]): string {
  const methods = resources.reduce((n, r) => n + Object.keys(r.resourceMethods ?? {}).length, 0);
  const head = `${name} has ${count(resources.length, "resource")} and ${count(methods, "method")}`;
  if (stages.length === 0) return `${head}, and has never been deployed.`;
  if (stages.length === 1) {
    const s = stages[0]!;
    const d = deployments.find((x) => x.id === s.deploymentId);
    return `${head}, and one stage: ${s.stageName}${s.deploymentId ? `, pointing at deployment ${s.deploymentId}${d?.createdDate ? ` from ${when(d.createdDate)}` : ""}` : ""}.`;
  }
  return `${head}, and ${count(stages.length, "stage")}: ${list(stages.map((s) => s.stageName))}.`;
}

const NUMBER_WORDS = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];
/** A small number as a word (`two`), a larger one as figures. */
export const numberWord = (n: number): string => NUMBER_WORDS[n] ?? String(n);
const capital = (t: string): string => t.charAt(0).toUpperCase() + t.slice(1);

/** `larder-admin has 3 routes on one stage, $default. Two routes run the Lambda function export-report behind the Lambda authorizer admin-request-auth; ANY /orders/{proxy+} has no integration yet.` */
export function routesSentence(name: string, routes: readonly { routeKey: string; target?: string; authorizerId?: string }[], stages: readonly HttpStage[], info: { fn: (target: string | undefined) => string | null; authorizer: (id: string | undefined) => string | null }): string {
  const names = stages.map((s) => s.stageName);
  const where = names.length === 0 ? "no stage" : names.length === 1 ? `one stage, ${names[0]}` : `${numberWord(names.length)} stages, ${list(names)}`;
  const head = `${name} has ${count(routes.length, "route")} on ${where}.`;
  const groups = new Map<string, number>();
  for (const r of routes) {
    const fn = r.target ? info.fn(r.target) : null;
    if (!fn) continue;
    const by = info.authorizer(r.authorizerId);
    const key = `the Lambda function ${fn}${by ? ` behind ${by}` : ""}`;
    groups.set(key, (groups.get(key) ?? 0) + 1);
  }
  const parts = [...groups].map(([what, n]) => `${n === 1 ? "One route runs" : `${capital(numberWord(n))} routes run`} ${what}`);
  const bare = routes.filter((r) => !r.target);
  const open = bare.length > 0 ? `${list(bare.map((r) => r.routeKey))} ${bare.length === 1 ? "has" : "have"} no integration yet` : "";
  const tail = [parts.join(" and "), open].filter(Boolean).join("; ");
  return tail ? `${head} ${tail}.` : head;
}

/** The two addresses a stage answers at: AWS's, and this doze-aws's. A WebSocket API's is `wss://` (here `ws://`) and always has the stage in its path
 * (`$default` too); an HTTP API's `$default` stage is the base of the address. */
export function invokeUrls(o: { id: string; region: string; endpoint: string; stage: string; http: boolean; websocket?: boolean }): { aws: string; local: string } {
  const root = o.http && !o.websocket && o.stage === "$default";
  const path = root ? "" : `/${o.stage}`;
  return {
    aws: `${o.websocket ? "wss" : "https"}://${o.id}.execute-api.${o.region}.amazonaws.com${path}`,
    local: `${o.websocket ? o.endpoint.replace(/^http/, "ws") : o.endpoint}/_aws/execute-api/${o.id}${path}`,
  };
}

/** The ARN a method authorizes by. */
export const methodArn = (region: string, account: string, apiId: string, method: string, path: string): string => `arn:aws:execute-api:${region}:${account}:${apiId}/*/${method}${path}`;

/** `50 per second, burst 100`. */
export function throttleWords(t: UsagePlan["throttle"]): string {
  if (!t || t.rateLimit === undefined) return "Not set";
  return `${t.rateLimit.toLocaleString("en-US")} per second, burst ${(t.burstLimit ?? 0).toLocaleString("en-US")}`;
}
/** `10,000 per month`. */
export function quotaWords(q: UsagePlan["quota"]): string {
  if (!q || q.limit === undefined) return "Not set";
  return `${q.limit.toLocaleString("en-US")} per ${(q.period ?? "month").toLowerCase()}`;
}
/** `larder-public:prod`. */
export const stageWords = (s: { apiId: string; stage: string }, names: ReadonlyMap<string, string>): string => `${names.get(s.apiId) ?? s.apiId}:${s.stage}`;

/** A deployment of an HTTP API as the Stages sentence says it. */
export function httpStagesSentence(name: string, stages: readonly HttpStage[], deployments: readonly HttpDeployment[]): string {
  if (stages.length === 0) return `${name} has no stage, so it cannot be invoked.`;
  const auto = stages.filter((s) => s.autoDeploy);
  const by = (s: HttpStage) => deployments.find((d) => d.deploymentId === s.deploymentId);
  const parts: string[] = [];
  for (const s of auto) {
    const d = by(s);
    parts.push(`${s.stageName} releases every change by itself${d?.createdDate ? `; its last deployment, ${s.deploymentId}, went out ${when(d.createdDate)}` : ""}.`);
  }
  for (const s of stages.filter((x) => !x.autoDeploy)) parts.push(s.deploymentId ? `${s.stageName} is deployed by hand and points at ${s.deploymentId}${by(s)?.createdDate ? `, from ${when(by(s)?.createdDate)}` : ""}.` : `${s.stageName} is deployed by hand and has no deployment yet.`);
  return parts.join(" ");
}

/** The newest deployment first. */
export const newestFirst = <T extends { createdDate?: number | string }>(items: readonly T[]): T[] => [...items].sort((a, b) => ms(b.createdDate) - ms(a.createdDate));

/** A REST name of 1 to 128 characters: letters, digits, hyphen, underscore (a stage's). */
export const stageNameProblem = (n: string): string | null => (n === "" ? "Enter a stage name." : /^[A-Za-z0-9_-]{1,128}$/.test(n) ? null : "Use letters, digits, hyphens and underscores, up to 128 characters.");
/** A path part: letters, digits and `-._~`, or a `{variable}` (with `+` for a greedy one). */
export const pathPartProblem = (p: string): string | null => (p === "" ? "Enter a resource name." : /^([A-Za-z0-9._~:-]+|\{[A-Za-z0-9._:-]+\+?\})$/.test(p) ? null : "Use letters, digits and . _ ~ : -, or wrap a name in curly braces for a path parameter.");
/** An API name: 1 to 128 characters. */
export const apiNameProblem = (n: string): string | null => (n.trim() === "" ? "Enter an API name." : n.length > 128 ? "Use 128 characters or fewer." : null);
/** A stage variable's name and value. */
export const variableProblem = (k: string, v: string): string | null => (!/^[A-Za-z0-9_]+$/.test(k) ? `${k || "A name"}: use letters, digits and underscores.` : !/^[A-Za-z0-9-._~:/?#&=,]*$/.test(v) ? `${k}: a value may hold letters, digits and - . _ ~ : / ? # & = ,` : null);

/** A stage as the one-line summary of the list needs it. */
export type Answering = { stage: string; auto: boolean; deployed: string | null };

/** `larder-public answers through its prod stage, deployed …. larder-admin answers at its $default stage, which deploys every change you save.` */
export function apisSentence(apis: readonly AnyApi[], by: Readonly<Record<string, readonly Answering[]>>, most = 4): string {
  const one = (a: AnyApi): string => {
    const st = by[a.id];
    if (!st) return "";
    if (st.length === 0) return `${a.name} has no stage yet, so it cannot be invoked.`;
    if (a.protocol === "REST") {
      const names = st.map((s) => s.stage);
      const when1 = st.length === 1 && st[0]?.deployed ? `, deployed ${st[0].deployed}` : "";
      return `${a.name} answers through its ${list(names)} ${names.length === 1 ? "stage" : "stages"}${when1}.`;
    }
    const first = st.find((s) => s.auto) ?? st[0]!;
    return `${a.name} answers at its ${first.stage} stage${first.auto ? ", which deploys every change you save" : ", deployed by hand"}.`;
  };
  const said = apis.map(one).filter(Boolean);
  return said.slice(0, most).join(" ") + (said.length > most ? ` ${said.length - most === 1 ? "1 more API answers" : `${count(said.length - most, "more API")} answer`} too.` : "");
}

/** `Stage prod points at deployment m2c8q5, created … with the description "…". The one before it, d4h7n2 from …, is kept in the deployment history.` */
export function stageSentence(stage: Stage, deployments: readonly Deployment[]): string {
  const sorted = newestFirst(deployments);
  const now = sorted.find((d) => d.id === stage.deploymentId);
  if (!now) return `Stage ${stage.stageName} points at no deployment yet.`;
  const before = sorted.find((d) => d.id !== now.id && ms(d.createdDate) < ms(now.createdDate));
  return [`Stage ${stage.stageName} points at deployment ${now.id}, created ${when(now.createdDate)}${now.description ? ` with the description “${now.description}”` : ""}.`, before ? `The one before it, ${before.id} from ${when(before.createdDate)}, is kept in the deployment history.` : ""].filter(Boolean).join(" ");
}

/** The backend a method's integration calls, as the request path names it: a host, a function, `Mock`, an action. */
export function backendName(i: Integration | undefined): string {
  if (!i) return "No integration";
  if (i.type === "MOCK") return "Mock";
  const fn = lambdaName(i.uri);
  if (fn) return fn;
  if (i.type === "AWS" || i.type === "AWS_PROXY") {
    const m = /^arn:aws:apigateway:[^:]*:([^:]+):(?:action|path)\/(.+)$/.exec(i.uri ?? "");
    return m ? `${m[1]} ${m[2]}` : (i.uri ?? "AWS service");
  }
  try { return new URL((i.uri ?? "").replace(/\{[^}]+\}/g, "x")).host; } catch { return i.uri ?? "HTTP endpoint"; }
}

/** `A GET to /products needs no authorization and no API key. It accepts two query strings, category and page, and the catalog.larder.example.com backend answers.` */
export function methodSentence(m: RestMethod, path: string, authorizer: string | undefined): string {
  const auth = m.authorizationType === "AWS_IAM" ? "needs IAM authorization" : m.authorizationType === "CUSTOM" || m.authorizationType === "COGNITO_USER_POOLS" ? `is checked by the authorizer ${authorizer ?? "named on it"}` : "needs no authorization";
  const queries = Object.keys(m.requestParameters ?? {}).filter((k) => k.startsWith("method.request.querystring.")).map((k) => k.slice("method.request.querystring.".length));
  const numbers = ["no", "one", "two", "three", "four", "five"];
  const q = queries.length === 0 ? "It accepts no query string" : `It accepts ${numbers[queries.length] ?? queries.length} query ${queries.length === 1 ? "string" : "strings"}, ${list(queries)}`;
  const i = m.methodIntegration;
  const answers = !i ? "no backend is attached yet" : i.type === "MOCK" ? "API Gateway answers it itself" : lambdaName(i.uri) ? `the function ${backendName(i)} answers` : `the ${backendName(i)} backend answers`;
  return `A ${m.httpMethod} to ${path} ${auth}${m.apiKeyRequired ? " and an API key" : " and no API key"}. ${q}, and ${answers}.`;
}

/** What the Region already has, as the create forms say it: `a REST API, larder-public, and an HTTP API, larder-admin`. */
export function haveApis(rest: readonly string[], http: readonly string[]): string {
  const one = (all: readonly string[], a: string, many: string) => { const names = [...all].sort(); return (names.length === 0 ? "" : names.length === 1 ? `${a}, ${names[0]}` : `${names.length} ${many}, ${list(names)}`); };
  return [one(rest, "a REST API", "REST APIs"), one(http, "an HTTP API", "HTTP APIs")].filter(Boolean).join(", and ");
}

/** `a GET and an OPTIONS method` (a verb with the article it is spoken with). */
export const methodPhrase = (verbs: readonly string[]): string => (verbs.length === 0 ? "no method" : `${list(verbs.map((v) => `${/^[AEIOU]/.test(v) ? "an" : "a"} ${v}`))} method`);

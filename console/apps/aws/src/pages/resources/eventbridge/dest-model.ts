// API destinations and their connections: the rules of the forms' fields and the requests they make.
import { type KeyValueRow, filled } from "@doze/ui/components/app/key-values";

export const METHODS = ["POST", "PUT", "PATCH", "GET", "DELETE", "HEAD", "OPTIONS"] as const;

export type DestForm = { name: string; description: string; endpoint: string; method: string; rate: string; connection: string | null };
export const emptyDest = (connection: string | null = null): DestForm => ({ name: "", description: "", endpoint: "", method: "POST", rate: "", connection });

/** An API destination's or a connection's name: 1 to 64 letters, digits, dot, hyphen and underscore. */
export function nameProblem(name: string, what: string): string | null {
  if (name === "") return `Name the ${what}.`;
  if (name.length > 64) return `A ${what} name is at most 64 characters.`;
  return /^[.\-_A-Za-z0-9]+$/.test(name) ? null : `A ${what} name takes letters, digits, dot, hyphen and underscore only.`;
}

/** AWS requires https; doze-aws also accepts http, so a server on localhost can be a destination. */
export function endpointProblem(endpoint: string): string | null {
  if (endpoint === "") return "Give the endpoint.";
  try {
    const u = new URL(endpoint.replace(/\*/g, "x"));
    return u.protocol === "https:" || u.protocol === "http:" ? null : "The endpoint is an http or https address.";
  } catch {
    return "The endpoint is not an address.";
  }
}

export function rateProblem(rate: string): string | null {
  if (rate.trim() === "") return null;
  const n = Number(rate);
  return Number.isInteger(n) && n > 0 ? null : "The rate limit is a whole number above 0.";
}

export function destProblem(f: DestForm, taken: readonly string[]): string | null {
  return nameProblem(f.name, "API destination") ?? (taken.includes(f.name) ? `${f.name} already exists.` : null) ?? endpointProblem(f.endpoint) ?? rateProblem(f.rate) ?? (f.connection ? null : "Choose the connection.");
}

/** CreateApiDestination's input. */
export function createDestInput(f: DestForm, connectionArn: string): Record<string, unknown> {
  return {
    Name: f.name,
    ...(f.description ? { Description: f.description } : {}),
    InvocationEndpoint: f.endpoint,
    HttpMethod: f.method,
    ...(f.rate.trim() ? { InvocationRateLimitPerSecond: Number(f.rate) } : {}),
    ConnectionArn: connectionArn,
  };
}

export type AuthType = "API_KEY" | "BASIC" | "OAUTH_CLIENT_CREDENTIALS";
export type ParamKind = "header" | "query" | "body";
export type HttpParam = KeyValueRow & { kind: ParamKind; secret: boolean };
export type ConnForm = { name: string; description: string; auth: AuthType; keyName: string; keyValue: string; username: string; password: string; tokenEndpoint: string; tokenMethod: string; clientId: string; clientSecret: string; params: HttpParam[] };
export const emptyConn = (): ConnForm => ({ name: "", description: "", auth: "API_KEY", keyName: "X-Api-Key", keyValue: "", username: "", password: "", tokenEndpoint: "", tokenMethod: "POST", clientId: "", clientSecret: "", params: [] });

/** What is missing for the authorization type chosen; `null` when it can be sent. */
export function connProblem(f: ConnForm, taken: readonly string[]): string | null {
  const own = nameProblem(f.name, "connection") ?? (taken.includes(f.name) ? `${f.name} already exists.` : null);
  if (own) return own;
  if (f.auth === "API_KEY") return f.keyName && f.keyValue ? null : "An API key needs a name and a value.";
  if (f.auth === "BASIC") return f.username && f.password ? null : "Basic authorization needs a username and a password.";
  return f.tokenEndpoint && f.clientId && f.clientSecret ? (endpointProblem(f.tokenEndpoint) ? "The authorization endpoint is an http or https address." : null) : "OAuth needs the authorization endpoint, a client ID and a client secret.";
}

/** What the call shows in place of a secret. */
export const HIDDEN = "...";

/** CreateConnection's input: the authorization for its type and the invocation HTTP parameters that were filled in.
 * `shown`: the call as it is put on the page and in the copied CLI and boto3, where a secret (an API key, a password, a client secret, a
 * parameter marked secret) is `...`, not what was typed; what is sent is never `shown`. */
export function createConnInput(f: ConnForm, shown = false): Record<string, unknown> {
  const hide = (v: string) => (shown && v !== "" ? HIDDEN : v);
  const params = filled(f.params as KeyValueRow[]) as HttpParam[];
  const of = (k: ParamKind) => params.filter((p) => p.kind === k).map((p) => ({ Key: p.key, Value: p.secret ? hide(p.value) : p.value, IsValueSecret: p.secret }));
  const http = { HeaderParameters: of("header"), QueryStringParameters: of("query"), BodyParameters: of("body") };
  const used = Object.fromEntries(Object.entries(http).filter(([, v]) => v.length > 0));
  const auth =
    f.auth === "API_KEY" ? { ApiKeyAuthParameters: { ApiKeyName: f.keyName, ApiKeyValue: hide(f.keyValue) } }
    : f.auth === "BASIC" ? { BasicAuthParameters: { Username: f.username, Password: hide(f.password) } }
    : { OAuthParameters: { AuthorizationEndpoint: f.tokenEndpoint, HttpMethod: f.tokenMethod, ClientParameters: { ClientID: f.clientId, ClientSecret: hide(f.clientSecret) } } };
  return {
    Name: f.name,
    ...(f.description ? { Description: f.description } : {}),
    AuthorizationType: f.auth,
    AuthParameters: { ...auth, ...(Object.keys(used).length > 0 ? { InvocationHttpParameters: used } : {}) },
  };
}

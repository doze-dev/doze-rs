// The console API, as much of it as the kit's components call, answered in the page (the gallery has no server). Deterministic:
// the same request gets the same answer, so a screenshot is comparable. A request is answered by patching `fetch` for
// `/api/v1/*` only; everything else goes through. Tests steer it with `window.__mock`:
//   delay   ms every answer waits (a loading state to look at)
//   fail    paths (substring) that answer 500
// Operations with a name the mock reserves: `Unbuilt` (404 NOT_BUILT), `Hang` (never answers: the loading state).
export type MockControl = { delay: number; fail: string[]; calls: { method: string; path: string; body: unknown }[] };
declare global {
  interface Window {
    __mock: MockControl;
  }
}

const REQUIRED: Record<string, string[]> = { CreateQueue: ["QueueName"], CreateTopic: ["Name"], CreateBucket: ["Bucket"], CreatePolicy: ["PolicyName", "PolicyDocument"], CreateSecret: ["Name"], PutBucketCors: ["Bucket", "CORSConfiguration"], IncreaseStreamRetentionPeriod: ["StreamName", "RetentionPeriodHours"] };
const kebab = (s: string) => s.replace(/([a-z0-9])([A-Z])/g, "$1-$2").toLowerCase();
const snake = (s: string) => kebab(s).replace(/-/g, "_");
const lit = (v: unknown): string => JSON.stringify(v);

function preview(body: { service: string; operation: string; input: Record<string, unknown> }) {
  const { service, operation, input } = body;
  const missing = (REQUIRED[operation] ?? []).filter((k) => input[k] === undefined || input[k] === "");
  const flags = Object.entries(input).map(([k, v]) => `--${kebab(k)} ${typeof v === "string" ? v : `'${lit(v)}'`}`);
  const args = Object.entries(input).map(([k, v]) => `${k}=${lit(v)}`);
  return { json: input, cli: `aws ${service} ${kebab(operation)}${flags.length ? ` ${flags.join(" ")}` : ""}`, boto3: `import boto3\n\n${service} = boto3.client("${service}", endpoint_url="http://127.0.0.1:14590")\n${service}.${snake(operation)}(${args.join(", ")})`, missing };
}

const QUEUES = ["pick-lists", "pick-lists-dlq", "orders", "stock-moves.fifo", "returns", "a-queue-whose-name-is-very-long-indeed-and-goes-on-past-any-reasonable-column-width"];
const NAMES: Record<string, string[]> = { "sqs/queues": QUEUES, "sns/topics": ["payment-settled", "order-placed", "alerts"], "lambda/functions": ["resize-images", "report-job", "pick-list-builder"], "iam/roles": ["larder-admin", "lambda-exec", "report-job"], "kms/keys": ["alias/larder-app", "alias/backups"], "eventbridge/buses": ["default", "larder"], "iam/policies": ["AdministratorAccess", "ReadOnlyAccess", "larder-queue-send"] };
const rowOf = (service: string, name: string) => ({ name, label: name, id: service === "sqs" ? `http://127.0.0.1:14590/123456789012/${name}` : `arn:aws:${service}:us-east-1:123456789012:${name}`, cells: [], link: null });

let tags = [{ key: "app", value: "fulfilment" }, { key: "env", value: "dev" }, { key: "owner", value: "" }];

function metrics(input: { MetricDataQueries?: { Id: string; Label?: string }[]; StartTime?: number; EndTime?: number }) {
  const from = input.StartTime ?? 0;
  const to = input.EndTime ?? 0;
  const n = 36;
  return {
    MetricDataResults: (input.MetricDataQueries ?? []).map((q, qi) => {
      const ts = Array.from({ length: n }, (_, i) => Math.round(from + ((to - from) * i) / (n - 1)));
      return { Id: q.Id, Label: q.Label ?? q.Id, Timestamps: ts, Values: ts.map((_, i) => Math.max(0, Math.round((qi === 0 ? 22 : 3) + (qi === 0 ? 9 : 2) * Math.sin(i / 3 + qi) - (qi === 0 ? i / 4 : 0)))) };
    }),
  };
}

type Answer = { status: number; body: unknown } | "hang";
function answer(method: string, path: string, body: unknown): Answer {
  if (path === "/api/v1/preview" && method === "POST") {
    const b = body as { service: string; operation: string; input: Record<string, unknown> };
    if (b.operation === "Unbuilt") return { status: 404, body: { code: "NOT_BUILT", message: "doze-aws has not built Unbuilt, so there is no call to show.", status: 404 } };
    if (b.operation === "Hang") return "hang";
    return { status: 200, body: preview(b) };
  }
  const list = /^\/api\/v1\/resources\/([^/]+)\/([^/?]+)(?:\?.*)?$/.exec(path);
  if (list && method === "GET") {
    const [, service, kind] = list as unknown as [string, string, string];
    if (kind === "broken") return { status: 500, body: { code: "InternalFailure", message: "The list could not be read.", status: 500 } };
    const names = NAMES[`${service}/${kind}`] ?? [];
    return { status: 200, body: { service, service_title: service, group: "", kind, title: kind, one: kind, about: "", operation: "List", columns: [], rows: names.map((n) => rowOf(service, n)), next: null, create: null, delete: null, openable: true, others: null, all: false, create_elsewhere: null } };
  }
  const tagged = /^\/api\/v1\/resources\/[^/]+\/[^/]+\/([^/]+)\/tags$/.exec(path);
  if (tagged) {
    if (decodeURIComponent(tagged[1]!) === "locked") return { status: 200, body: { tags: tags.slice(0, 2), editable: false } };
    if (method === "PUT") {
      const c = body as { set: { key: string; value: string }[]; remove: string[] };
      if (c.set.some((t) => t.key.toLowerCase().startsWith("aws:"))) return { status: 400, body: { code: "InvalidParameterValue", message: "Tag keys starting with aws: are reserved for AWS use.", status: 400 } };
      tags = [...tags.filter((t) => !c.remove.includes(t.key) && !c.set.some((s) => s.key === t.key)), ...c.set].sort((a, b) => a.key.localeCompare(b.key));
    }
    return { status: 200, body: { tags, editable: true } };
  }
  if (/\/blockers$/.test(path)) return { status: 200, body: { blockers: [{ title: "Attached policies", items: [{ name: "AdministratorAccess", link: { service: "iam", kind: "policies", name: "AdministratorAccess" } }], clears: true, blocks: true, more: false }], kept: ["The policy itself stays.", "The log group the function wrote to stays."], words: "larder-admin" } };
  if (/\/op\/GetMetricData$/.test(path)) return { status: 200, body: metrics(body as never) };
  return { status: 404, body: { code: "NOT_MOCKED", message: `The gallery does not answer ${method} ${path}.`, status: 404 } };
}

/** Patches `fetch` for the API's paths; call once, before the first render. */
export function installMockApi(): void {
  window.__mock = { delay: 0, fail: [], calls: [] };
  const real = window.fetch.bind(window);
  window.fetch = Object.assign(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
    if (!url.pathname.startsWith("/api/v1/")) return real(input, init);
    const method = (init?.method ?? "GET").toUpperCase();
    const path = url.pathname + url.search;
    const body: unknown = typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
    window.__mock.calls.push({ method, path, body });
    if (window.__mock.delay > 0) await new Promise((r) => setTimeout(r, window.__mock.delay));
    const a = window.__mock.fail.some((f) => path.includes(f)) ? { status: 500, body: { code: "InternalFailure", message: "The mock was told to fail.", status: 500 } } : answer(method, path, body);
    if (a === "hang") {
      return new Promise<Response>((_, reject) => init?.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError"))));
    }
    return new Response(JSON.stringify(a.body), { status: a.status, headers: { "Content-Type": "application/json" } });
  }, { preconnect: real.preconnect });
}

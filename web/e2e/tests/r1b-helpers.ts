// Slice R1b's specs make their own topics through the console's API (the same POSTs the pages send, with the console's
// header and an Origin, the same-origin proof), so they never change what the seeded world's other specs count on.
import type { APIRequestContext, TestInfo } from "@playwright/test";

let n = 0;
/** A topic name no other test or browser uses. */
export function fresh(info: TestInfo, what: string): string {
  n += 1;
  return `r1b-${what}-${info.project.name}-${Date.now().toString(36)}-${n}`;
}

function headers(baseURL: string) {
  return { "X-Console": "1", Origin: new URL(baseURL).origin, "Content-Type": "application/x-www-form-urlencoded" };
}

export async function call(request: APIRequestContext, baseURL: string, path: string, fields: Record<string, string>) {
  const res = await request.post(path, { headers: headers(baseURL), data: new URLSearchParams(fields).toString() });
  return { status: res.status(), body: await res.json() };
}

export async function get(request: APIRequestContext, path: string) {
  const res = await request.get(path, { headers: { "X-Console": "1" } });
  return { status: res.status(), body: await res.json() };
}

/** Makes a topic; `template` compact makes it compacted. */
export async function topic(request: APIRequestContext, baseURL: string, name: string, partitions = 1, template: "event" | "compact" | "short" = "event", retention?: string) {
  const r = await call(request, baseURL, "/api/v1/create-topic", { name, partitions: String(partitions), template, ...(retention ? { "retention.ms": retention } : {}), mode: "create" });
  if (r.status !== 200) throw new Error(`create ${name}: ${r.status} ${JSON.stringify(r.body)}`);
}

export async function produce(request: APIRequestContext, baseURL: string, name: string, key: string, value: string, partition = "") {
  const r = await call(request, baseURL, `/api/v1/topics/${encodeURIComponent(name)}/produce`, { key, value, partition, acks: "-1" });
  if (r.status !== 200) throw new Error(`produce ${name}: ${r.status} ${JSON.stringify(r.body)}`);
  return r.body.produced.offset as number;
}

export async function setConfig(request: APIRequestContext, baseURL: string, name: string, key: string, value: string) {
  const r = await call(request, baseURL, `/api/v1/topics/${encodeURIComponent(name)}/config/edit`, { key, op: "set", value, other: "" });
  if (r.status !== 200) throw new Error(`config ${name} ${key}: ${r.status} ${JSON.stringify(r.body)}`);
}

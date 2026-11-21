// A resource's own pages talk to crates/aws-console/src/api/deep.rs: one operation at a time, named by the page and allowed by
// the descriptor, made through the services' bus as the caller `console` on this resource alone (the server sets the queue,
// bucket or table). Nothing here polls: a read is made when the page opens and when the person asks.
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { client } from "../../../api/client";
import { RESOURCE_PATHS } from "../../../api/resources";
import type { Uploaded } from "../../../api/types/Uploaded";

/** Where one resource's operations go. */
export type Target = { service: string; kind: string; name: string };

/** The resources whose pages read through each other's (a function's log group) or from a page of their own (an execution's
 * machine, a stream's group). */
export const on = {
  fn: (name: string): Target => ({ service: "lambda", kind: "functions", name }),
  group: (name: string): Target => ({ service: "logs", kind: "groups", name }),
  stack: (name: string): Target => ({ service: "cloudformation", kind: "stacks", name }),
  machine: (name: string): Target => ({ service: "stepfunctions", kind: "machines", name }),
};

/** The machine an execution belongs to, by its ARN (`arn:aws:states:<region>:<account>:execution:<machine>:<name>`): the
 * server makes the execution's calls on that machine alone, and refuses an ARN that is not one of its executions. */
export const machineOf = (executionArn: string) => executionArn.split(":")[6] ?? "";

const json = (body: unknown): RequestInit => ({ method: "POST", body: JSON.stringify(body), headers: { "Content-Type": "application/json" } });

/** `POST …/<name>/op/<operation>`: the service's answer as it gave it. */
export function callOp<T>(t: Target, operation: string, input: object = {}, signal?: AbortSignal): Promise<T> {
  return client.api<T>(`${RESOURCE_PATHS.one(t.service, t.kind, t.name)}/op/${encodeURIComponent(operation)}`, json(input), signal);
}

/** The query key of a read of one resource's page: what a write invalidates. */
export const deepKey = (t: Target, ...rest: unknown[]) => ["resources", "deep", t.service, t.kind, t.name, ...rest];

/** A read made through an operation (a scan, a listing): once when the page opens with these inputs, then on asking. */
export function useOpQuery<T>(t: Target, operation: string, input: object, key: unknown[], enabled = true) {
  return useQuery({
    queryKey: deepKey(t, operation, ...key),
    queryFn: ({ signal }) => callOp<T>(t, operation, input, signal),
    enabled,
    retry: false,
  });
}

/** A read whose input is its key: a new input is a new read. */
export function useOpRead<T>(t: Target, operation: string, input: object, enabled = true) {
  return useOpQuery<T>(t, operation, input, [input], enabled);
}

/** A write (or a read that changes something, as a peek does): made when the person asks; the page's reads are stale after. */
export function useOp<T, I extends object = object>(t: Target, operation: string, { keep = false }: { keep?: boolean } = {}) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: I) => callOp<T>(t, operation, input),
    onSuccess: () => {
      if (keep) return;
      void qc.invalidateQueries({ queryKey: deepKey(t) });
      void qc.invalidateQueries({ queryKey: ["resources", "one", t.service, t.kind, t.name] });
    },
  });
}

/** The most an upload through the console may be (the server's `UPLOAD_MAX`). */
export const UPLOAD_MAX = 25 * 1024 * 1024;

/** The address of an object of a bucket. */
export const objectPath = (bucket: string, key: string) => `${RESOURCE_PATHS.one("s3", "buckets", bucket)}/object?key=${encodeURIComponent(key)}`;

/** PutObject of `body` as `key` (a folder is an empty body and a key ending in `/`). */
export function putObject(bucket: string, key: string, body: Blob, type: string): Promise<Uploaded> {
  return client.api<Uploaded>(objectPath(bucket, key), { method: "POST", body, headers: { "Content-Type": type || "application/octet-stream" } });
}

/** GetObject, saved as a file. The console's API answers only requests that carry its proof header, which a plain link cannot
 * send: the bytes are fetched (the server streams them from S3) and handed to the browser to save. */
export async function downloadObject(bucket: string, key: string): Promise<void> {
  const res = await fetch(`${client.basePath}${objectPath(bucket, key)}`, { headers: { "X-Console": "1" }, credentials: "same-origin" });
  if (!res.ok) {
    const body = (await res.json().catch(() => null)) as { code?: string; message?: string } | null;
    throw new Error(body?.message ?? `The download was refused (${res.status}).`);
  }
  const blob = await res.blob();
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = key.split("/").filter(Boolean).pop() ?? "object";
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

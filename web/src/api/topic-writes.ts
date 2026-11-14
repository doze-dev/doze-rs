// The topic writes and the Config tab (slice R1b): the form previews (GETs the dialogs refetch as a person types, so the
// call, the teaching and a button's label are the server's words) and the writes (form-encoded POSTs, as the classic
// forms post). After a write every page query is marked stale, so the pages behind the overlay show the new state at once
// (and the others when they are next shown); the form previews are left alone.
import { keepPreviousData, type QueryClient, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api, ApiFailure } from "./client";
import { segment } from "./path";
import type { AddForm } from "./types/AddForm";
import type { ConfigEditForm } from "./types/ConfigEditForm";
import type { ConfigResetForm } from "./types/ConfigResetForm";
import type { ConfigTab } from "./types/ConfigTab";
import type { CreateForm } from "./types/CreateForm";
import type { DeleteForm } from "./types/DeleteForm";
import type { ProduceForm } from "./types/ProduceForm";
import type { RecordsForm } from "./types/RecordsForm";

/** The first element of every key of a form preview: never invalidated by a write. */
export const FORM = "form" as const;

export const writeKeys = {
  config: (topic: string) => ["topic-config", topic] as const,
  form: (what: string, topic: string, q: string) => [FORM, what, topic, q] as const,
};

const enc = segment;

/** `?a=1&b=2` from the fields that have a value (`""` for none). */
export function queryOf(fields: Record<string, string | undefined>): string {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(fields)) if (v !== undefined && v !== "") p.set(k, v);
  const s = p.toString();
  return s ? `?${s}` : "";
}

/** A form-encoded POST: what the classic forms send, behind the same-origin proof the client adds. */
export async function post<T>(path: string, fields: Record<string, string>): Promise<T> {
  const body = await api<T | Refused>(path, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams(fields).toString(),
  });
  // Every answer to a write is a 200 with an `outcome` (as R2's group writes): a refused or invalid one is thrown here, so
  // a mutation's error is Kafka's words and its state (`detail`). Its status says which: 422 refused, 400 invalid.
  if (isRefused(body)) throw new ApiFailure(body.outcome === "refused" ? 422 : 400, body);
  return body as T;
}

type Refused = { outcome: "refused" | "invalid"; code: string; message: string; detail?: unknown };
function isRefused(v: unknown): v is Refused {
  const o = (v as Refused | null)?.outcome;
  return (o === "refused" || o === "invalid") && typeof (v as Refused).code === "string";
}

/** The refusal's state when the server sent one (`detail`), typed by the caller. */
export function detailOf<T>(e: unknown): T | undefined {
  return e instanceof ApiFailure ? (e.detail as T | undefined) : undefined;
}

/** After a write: every query but the form previews is stale; the ones on screen refetch now. */
export function afterWrite(qc: QueryClient) {
  return qc.invalidateQueries({ predicate: (q) => q.queryKey[0] !== FORM });
}

/** A form GET: `{missing}` (no such topic or config, a 200 as R1a's lookups) is thrown as an ApiFailure with its words. */
async function lookup<T>(path: string, signal: AbortSignal): Promise<T> {
  const body = await api<T | { missing: { code: string; message: string } }>(path, {}, signal);
  if (body && typeof body === "object" && "missing" in body) throw new ApiFailure(404, body.missing);
  return body as T;
}

/** A value that follows `v` once it has been still for `ms`. */
export function useDebounced<T>(v: T, ms = 250): T {
  const [d, setD] = useState(v);
  useEffect(() => {
    const t = setTimeout(() => setD(v), ms);
    return () => clearTimeout(t);
  }, [v, ms]);
  return d;
}

function useForm<T>(what: string, topic: string, path: string, q: string, enabled = true) {
  return useQuery({
    queryKey: writeKeys.form(what, topic, q),
    queryFn: ({ signal }) => lookup<T>(`${path}${q}`, signal),
    staleTime: Infinity,
    gcTime: 30_000,
    placeholderData: keepPreviousData,
    enabled,
    retry: false,
  });
}

export const topicPath = (topic: string, tail: string) => `/api/v1/topics/${enc(topic)}${tail}`;

export function useCreateForm(q: string) {
  return useForm<CreateForm>("create", "", "/api/v1/create-topic", q);
}
export function useProduceForm(topic: string, q: string, enabled: boolean) {
  return useForm<ProduceForm>("produce", topic, topicPath(topic, "/produce"), q, enabled);
}
export function useAddForm(topic: string, q: string, enabled: boolean) {
  return useForm<AddForm>("add", topic, topicPath(topic, "/add-partitions"), q, enabled);
}
export function useRecordsForm(topic: string, q: string, enabled: boolean) {
  return useForm<RecordsForm>("records", topic, topicPath(topic, "/delete-records"), q, enabled);
}
export function useDeleteForm(topic: string, enabled: boolean) {
  return useForm<DeleteForm>("delete", topic, topicPath(topic, "/delete"), "", enabled);
}
export function useConfigEditForm(topic: string, q: string, enabled: boolean) {
  return useForm<ConfigEditForm>("config-edit", topic, topicPath(topic, "/config/edit"), q, enabled);
}
export function useConfigResetForm(topic: string, q: string, enabled: boolean) {
  return useForm<ConfigResetForm>("config-reset", topic, topicPath(topic, "/config/reset"), q, enabled);
}

/** The Config tab: a snapshot with no refresh control (live-design.md's table: Config is "Snapshot, no control"). */
export function useConfigTab(topic: string) {
  return useQuery({
    queryKey: writeKeys.config(topic),
    queryFn: ({ signal }) => lookup<ConfigTab>(topicPath(topic, "/config"), signal),
    staleTime: Infinity,
  });
}

export { useQueryClient };

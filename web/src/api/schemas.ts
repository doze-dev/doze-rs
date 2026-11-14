// The Query hooks of the Schemas pages (crates/kafka-console/src/api/schemas*.rs) and of a topic's Schema tab. The pages are
// snapshots that follow the RefreshControl; a write invalidates every schema query and the shell's count. The live check and
// Try are POSTs that store nothing, answered as a person types.
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { usePollInterval } from "@/lib/refresh";
import { api, ApiFailure } from "./client";
import { keys } from "./queries";
import { segment } from "./path";
import type { CheckAnswer } from "./types/CheckAnswer";
import type { ConfigPost } from "./types/ConfigPost";
import type { DeletePost } from "./types/DeletePost";
import type { DiffResponse } from "./types/DiffResponse";
import type { Encoded } from "./types/Encoded";
import type { Missing } from "./types/Missing";
import type { SchemaPost } from "./types/SchemaPost";
import type { SchemasResponse } from "./types/SchemasResponse";
import type { SchemaWrite } from "./types/SchemaWrite";
import type { SubjectResponse } from "./types/SubjectResponse";
import type { TopicSchemas } from "./types/TopicSchemas";
import type { TryPost } from "./types/TryPost";

export const schemaKeys = {
  all: ["schemas"] as const,
  list: ["schemas", "list"] as const,
  subject: (s: string, v?: number) => ["schemas", "subject", s, v ?? "latest"] as const,
  diff: (s: string, from?: number, to?: number) => ["schemas", "diff", s, from ?? 0, to ?? 0] as const,
  topic: (t: string) => ["schemas", "topic", t] as const,
};

export const subjectPath = (s: string, tail = "") => `/api/v1/schemas/subjects/${segment(s)}${tail}`;

async function lookup<T>(path: string, signal: AbortSignal): Promise<T> {
  const body = await api<T | Missing>(path, {}, signal);
  if (body && typeof body === "object" && "missing" in body) throw new ApiFailure(404, (body as Missing).missing);
  return body as T;
}

export function useSchemas() {
  const refetchInterval = usePollInterval();
  return useQuery({
    queryKey: schemaKeys.list,
    queryFn: ({ signal }) => api<SchemasResponse>("/api/v1/schemas", {}, signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
  });
}

export function useSubject(subject: string, version?: number) {
  const refetchInterval = usePollInterval();
  return useQuery({
    queryKey: schemaKeys.subject(subject, version),
    queryFn: ({ signal }) => lookup<SubjectResponse>(subjectPath(subject, version ? `?version=${version}` : ""), signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
    retry: false,
    enabled: subject !== "",
  });
}

export function useDiff(subject: string, from?: number, to?: number) {
  const q = new URLSearchParams();
  if (from) q.set("from", String(from));
  if (to) q.set("to", String(to));
  const qs = q.toString();
  return useQuery({
    queryKey: schemaKeys.diff(subject, from, to),
    queryFn: ({ signal }) => lookup<DiffResponse>(subjectPath(subject, `/diff${qs ? `?${qs}` : ""}`), signal),
    staleTime: Infinity,
    placeholderData: keepPreviousData,
    retry: false,
  });
}

export function useTopicSchemas(topic: string) {
  const refetchInterval = usePollInterval();
  return useQuery({
    queryKey: schemaKeys.topic(topic),
    queryFn: ({ signal }) => lookup<TopicSchemas>(`/api/v1/topics/${segment(topic)}/schemas`, signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
    retry: false,
  });
}

function postJson<T>(path: string, body: unknown, signal?: AbortSignal): Promise<T> {
  return api<T>(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }, signal);
}

/** The live check of a draft: a POST that stores nothing, keyed by the draft so a repeat is not asked again. */
export function useCheck(draft: SchemaPost | null) {
  return useQuery({
    queryKey: ["schemas-check", draft],
    queryFn: ({ signal }) => postJson<CheckAnswer>("/api/v1/schemas/check", draft, signal),
    enabled: !!draft && draft.schema.trim() !== "" && draft.subject.trim() !== "",
    staleTime: Infinity,
    gcTime: 30_000,
    placeholderData: keepPreviousData,
    retry: false,
  });
}

/** Try: a value encoded with a version, nothing sent. */
export function useTry(t: TryPost | null) {
  return useQuery({
    queryKey: ["schemas-try", t],
    queryFn: ({ signal }) => postJson<Encoded>("/api/v1/schemas/try", t, signal),
    enabled: !!t && t.value.trim() !== "",
    staleTime: Infinity,
    gcTime: 30_000,
    placeholderData: keepPreviousData,
    retry: false,
  });
}

function useSchemaWrite<V>(path: (v: V) => string, body: (v: V) => unknown) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (v: V) => postJson<SchemaWrite>(path(v), body(v)),
    onSuccess: (w) => {
      if (w.outcome === "refused") return;
      void qc.invalidateQueries({ queryKey: schemaKeys.all });
      void qc.invalidateQueries({ queryKey: keys.shell });
      void qc.invalidateQueries({ queryKey: ["topics"] });
    },
  });
}

export function useRegister() {
  return useSchemaWrite<SchemaPost>(() => "/api/v1/schemas/register", (v) => v);
}

export function useConfigure(subject: string | null) {
  return useSchemaWrite<ConfigPost>(() => (subject === null ? "/api/v1/schemas/config" : subjectPath(subject, "/config")), (v) => v);
}

export function useDeleteSchema(subject: string) {
  return useSchemaWrite<DeletePost>(() => subjectPath(subject, "/delete"), (v) => v);
}

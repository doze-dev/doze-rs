// The kit's calls: a kind-level operation (contract §2), a resource's tags (§3), what blocks a delete (§4).
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { client } from "../api/client";
import { RESOURCE_PATHS } from "../api/resources";
import type { Blockers, TagsAnswer, TagsChange } from "./contract";
import type { Deleted } from "../api/types/Deleted";

const enc = encodeURIComponent;
const json = (method: string, body: unknown): RequestInit => ({ method, body: JSON.stringify(body), headers: { "Content-Type": "application/json" } });

/** A resource without a name in front of it: one kind of one service. */
export type Kind = { service: string; kind: string };
/** One resource. */
export type Target = Kind & { name: string };

export const kindOpPath = (k: Kind, operation: string) => `${RESOURCE_PATHS.list(k.service, k.kind)}/op/${enc(operation)}`;
export const tagsPath = (t: Target) => `${RESOURCE_PATHS.one(t.service, t.kind, t.name)}/tags`;
export const blockersPath = (t: Target) => `${RESOURCE_PATHS.one(t.service, t.kind, t.name)}/blockers`;

/** `POST /resources/<service>/<kind>/op/<Operation>`: the service's own answer to an operation the kind allows. */
export function callKindOp<T>(k: Kind, operation: string, input: object = {}, signal?: AbortSignal): Promise<T> {
  return client.api<T>(kindOpPath(k, operation), json("POST", input), signal);
}

/** A kind-level operation as a mutation (a send, a publish, a create). A kind's list is stale after one, unless `keep`. */
export function useKindOp<T, I extends object = object>(k: Kind, operation: string, { keep = false }: { keep?: boolean } = {}) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: I) => callKindOp<T>(k, operation, input),
    onSuccess: () => {
      if (!keep) void qc.invalidateQueries({ queryKey: ["resources", "list", k.service, k.kind] });
    },
  });
}

/** A kind-level read (a metric query), made when the page opens and when the input changes. */
export function useKindOpRead<T>(k: Kind, operation: string, input: object, enabled = true) {
  return useQuery({ queryKey: ["resources", "kindop", k.service, k.kind, operation, input], queryFn: ({ signal }) => callKindOp<T>(k, operation, input, signal), enabled, retry: false });
}

const tagsKey = (t: Target) => ["resources", "tags", t.service, t.kind, t.name];

export function useTags(t: Target, enabled = true) {
  return useQuery({ queryKey: tagsKey(t), queryFn: ({ signal }) => client.api<TagsAnswer>(tagsPath(t), {}, signal), enabled, retry: false });
}

/** Saves a change of tags; the answer is the new set, which replaces what is cached. */
export function useSetTags(t: Target) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (change: TagsChange) => client.api<TagsAnswer>(tagsPath(t), json("PUT", change)),
    onSuccess: (answer) => qc.setQueryData(tagsKey(t), answer),
  });
}

/** What stands in the way of deleting `t` and what survives it; read when the delete dialog opens. */
export function useBlockers(t: Target, enabled = true) {
  return useQuery({ queryKey: ["resources", "blockers", t.service, t.kind, t.name], queryFn: ({ signal }) => client.api<Blockers>(blockersPath(t), {}, signal), enabled, retry: false, staleTime: 0, gcTime: 0 });
}

/** The delete itself, clearing the blockers that can be cleared first when `clear` (contract §4). */
export function useDeleteClearing(t: Target) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ clear }: { clear: boolean }) => client.api<Deleted>(`${RESOURCE_PATHS.one(t.service, t.kind, t.name)}${clear ? "?clear=yes" : ""}`, { method: "DELETE" }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["resources", "list", t.service, t.kind] });
      void qc.invalidateQueries({ queryKey: ["resources", "index"] });
    },
  });
}

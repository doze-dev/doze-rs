// The Resources space's queries and actions (crates/aws-console/src/api/resources.rs). Every read makes calls through the
// services' bus as the caller `console`, which Live shows, so nothing here polls: a page is read when it is opened and when the
// person asks (the RefreshControl's button). The catalogue (which kinds each service has) makes no call and is kept.
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { client } from "./client";
import type { Created } from "./types/Created";
import type { Deleted } from "./types/Deleted";
import type { ResourceDetail } from "./types/ResourceDetail";
import type { ResourceList } from "./types/ResourceList";
import type { ResourcesIndex } from "./types/ResourcesIndex";
import type { Revealed } from "./types/Revealed";

const enc = encodeURIComponent;

export const RESOURCE_PATHS = {
  index: "/api/v1/resources",
  catalogue: "/api/v1/resources?counts=no",
  list: (service: string, kind: string, token?: string, all = false, params: Record<string, string> = {}) => {
    const p = Object.entries(params).filter(([, v]) => v !== "").map(([k, v]) => `p.${enc(k)}=${enc(v)}`);
    const q = [token ? `token=${enc(token)}` : "", all ? "all=yes" : "", ...p].filter(Boolean).join("&");
    return `/api/v1/resources/${enc(service)}/${enc(kind)}${q ? `?${q}` : ""}`;
  },
  one: (service: string, kind: string, name: string) => `/api/v1/resources/${enc(service)}/${enc(kind)}/${enc(name)}`,
};

/** `GET /api/v1/resources`: every service with its kinds counted (each count is a list call, which Live shows). */
export function useResourcesIndex(enabled = true) {
  return useQuery({ queryKey: ["resources", "index"], queryFn: ({ signal }) => client.api<ResourcesIndex>(RESOURCE_PATHS.index, {}, signal), enabled, staleTime: 15_000 });
}

/** The services and their kinds, with no call made: what the pages route by. */
export function useCatalogue() {
  return useQuery({ queryKey: ["resources", "catalogue"], queryFn: ({ signal }) => client.api<ResourcesIndex>(RESOURCE_PATHS.catalogue, {}, signal), staleTime: Number.POSITIVE_INFINITY });
}

/** A kind's pages: the first when opened, the next when asked for (`fetchNextPage`), each by the token the one before gave.
 * `all`: with the rows the kind sets aside (AWS managed keys). */
export function useResourceList(service: string, kind: string, all = false, params: Record<string, string> = {}) {
  return useInfiniteQuery({
    queryKey: ["resources", "list", service, kind, all, params],
    queryFn: ({ signal, pageParam }) => client.api<ResourceList>(RESOURCE_PATHS.list(service, kind, pageParam || undefined, all, params), {}, signal),
    initialPageParam: "",
    getNextPageParam: (last) => last.next ?? undefined,
    retry: false,
  });
}

export function useResourceDetail(service: string, kind: string, name: string, enabled = true) {
  return useQuery({ queryKey: ["resources", "one", service, kind, name], queryFn: ({ signal }) => client.api<ResourceDetail>(RESOURCE_PATHS.one(service, kind, name), {}, signal), retry: false, enabled });
}

const json = (body: unknown): RequestInit => ({ method: "POST", body: JSON.stringify(body), headers: { "Content-Type": "application/json" } });

/** Lists and counts are stale after a change: they are read again (a list on screen at once, the index when next shown). */
function useInvalidate() {
  const qc = useQueryClient();
  return (service: string, kind: string) => {
    void qc.invalidateQueries({ queryKey: ["resources", "list", service, kind] });
    void qc.invalidateQueries({ queryKey: ["resources", "index"] });
  };
}

export function useCreate(service: string, kind: string) {
  const invalidate = useInvalidate();
  return useMutation({
    mutationFn: (form: Record<string, string>) => client.api<Created>(RESOURCE_PATHS.list(service, kind), json(form)),
    onSuccess: () => invalidate(service, kind),
  });
}

/** Delete one. Its own page's query is left alone (the page is still on screen until the list opens; removing it would read
 * the resource again and be refused): it is read afresh if the name is opened again. */
export function useDelete(service: string, kind: string) {
  const invalidate = useInvalidate();
  return useMutation({
    mutationFn: (name: string) => client.api<Deleted>(RESOURCE_PATHS.one(service, kind, name), { method: "DELETE" }),
    onSuccess: () => invalidate(service, kind),
  });
}

export function useReveal(service: string, kind: string, name: string) {
  return useMutation({ mutationFn: () => client.api<Revealed>(`${RESOURCE_PATHS.one(service, kind, name)}/reveal`, json({})) });
}

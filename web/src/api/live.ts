// The Live endpoints (slice R3). Snapshots, like every page (live-design.md): `staleTime: Infinity`, the previous data kept
// while a refresh is in flight, the RefreshControl's interval the only timer. The tail is not a query: `pages/live/tail.ts`.
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { intervalMs, useRefreshInterval } from "@/lib/refresh";
import { api } from "./client";
import type { ClientsView } from "./types/ClientsView";
import type { FirstResponse } from "./types/FirstResponse";
import type { GroupWireView } from "./types/GroupWireView";
import type { LiveResponse } from "./types/LiveResponse";
import type { RequestResponse } from "./types/RequestResponse";
import type { TopicWireView } from "./types/TopicWireView";

export const liveKeys = {
  live: (q: string) => ["live", q] as const,
  request: (id: number) => ["live-request", id] as const,
  clients: ["live-clients"] as const,
  first: ["live-first"] as const,
  topicWire: (t: string) => ["live-topic-wire", t] as const,
  groupWire: (g: string, q: string) => ["live-group-wire", g, q] as const,
};

function useInterval(): number | false {
  const [v] = useRefreshInterval();
  const ms = intervalMs(v);
  return ms > 0 ? ms : false;
}

/** The tape as a snapshot, for the filters in `query` (the server's own names). `poll`: false while the tail runs. */
export function useLive(query: string, poll = true) {
  const refetchInterval = useInterval();
  return useQuery({
    queryKey: liveKeys.live(query),
    queryFn: ({ signal }) => api<LiveResponse>(`/api/v1/live${query ? `?${query}` : ""}`, {}, signal),
    staleTime: Infinity,
    refetchInterval: poll ? refetchInterval : false,
    placeholderData: keepPreviousData,
  });
}

/** One request in full (the Sheet). Read once: a request does not change. */
export function useRequest(id: number | undefined) {
  return useQuery({
    queryKey: liveKeys.request(id ?? -1),
    queryFn: ({ signal }) => api<RequestResponse>(`/api/v1/live/request/${id}`, {}, signal),
    staleTime: Infinity,
    enabled: id !== undefined,
    retry: false,
  });
}

export function useClients(enabled: boolean) {
  return useQuery({
    queryKey: liveKeys.clients,
    queryFn: ({ signal }) => api<ClientsView>("/api/v1/live/clients", {}, signal),
    staleTime: Infinity,
    enabled,
  });
}

export function useFirst() {
  const refetchInterval = useInterval();
  return useQuery({
    queryKey: liveKeys.first,
    queryFn: ({ signal }) => api<FirstResponse>("/api/v1/live/first", {}, signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
  });
}

export function useTopicWire(topic: string) {
  const refetchInterval = useInterval();
  return useQuery({
    queryKey: liveKeys.topicWire(topic),
    queryFn: ({ signal }) => api<TopicWireView>(`/api/v1/live/topic-wire?topic=${encodeURIComponent(topic)}`, {}, signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
  });
}

export function useGroupWire(group: string, query: string) {
  const refetchInterval = useInterval();
  return useQuery({
    queryKey: liveKeys.groupWire(group, query),
    queryFn: ({ signal }) => api<GroupWireView>(`/api/v1/live/group-wire?group=${encodeURIComponent(group)}${query ? `&${query}` : ""}`, {}, signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
  });
}

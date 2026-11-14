// The Query hooks of the Groups pages (crates/kafka-console/src/api/groups.rs). Every page is a snapshot that follows the
// RefreshControl; a write invalidates the group's queries, the list and the shell's counts, so what changed is on screen.
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "./client";
import { keys, useInterval } from "./queries";
import { segment } from "./path";
import type { GroupDeleteView } from "./types/GroupDeleteView";
import type { GroupLagView } from "./types/GroupLagView";
import type { GroupsResponse } from "./types/GroupsResponse";
import type { GroupWriteOutcome } from "./types/GroupWriteOutcome";
import type { OffsetDeleteRequest } from "./types/OffsetDeleteRequest";
import type { OffsetDeleteView } from "./types/OffsetDeleteView";
import type { ResetRequest } from "./types/ResetRequest";
import type { ResetView } from "./types/ResetView";
import type { StoryView } from "./types/StoryView";

const seg = segment;

export const groupKeys = {
  list: (q: string, states: string[]) => ["groups", q, ...states] as const,
  all: ["groups"] as const,
  group: (id: string) => ["group", id] as const,
  lag: (id: string, expiring: boolean) => ["group", id, "lag", expiring] as const,
  story: (id: string) => ["group", id, "story"] as const,
  reset: (id: string, r: ResetRequest) => ["group", id, "reset", r.topic, r.to, r.value] as const,
  delete: (id: string) => ["group", id, "delete"] as const,
  offsets: (id: string) => ["group", id, "offsets"] as const,
};

function query(params: [string, string | undefined][]): string {
  const u = new URLSearchParams();
  for (const [k, v] of params) if (v) u.append(k, v);
  const s = u.toString();
  return s ? `?${s}` : "";
}

export function useGroups(q: string, states: string[]) {
  const refetchInterval = useInterval();
  return useQuery({
    queryKey: groupKeys.list(q, states),
    queryFn: ({ signal }) => api<GroupsResponse>(`/api/v1/groups${query([["q", q], ...states.map((s) => ["state", s] as [string, string])])}`, {}, signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
  });
}

/** One group and its lag; `expiring` asks when the commits are forgotten. */
export function useGroup(id: string, expiring = false) {
  const refetchInterval = useInterval();
  return useQuery({
    queryKey: groupKeys.lag(id, expiring),
    queryFn: ({ signal }) => api<GroupLagView>(`/api/v1/groups/${seg(id)}${expiring ? "?expiring=1" : ""}`, {}, signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
    retry: false,
  });
}

export function useStory(id: string) {
  const refetchInterval = useInterval();
  return useQuery({
    queryKey: groupKeys.story(id),
    queryFn: ({ signal }) => api<StoryView>(`/api/v1/groups/${seg(id)}/story`, {}, signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
    retry: false,
  });
}

/** The Reset preview: a GET that reads OffsetFetch and ListOffsets and never writes. */
export function useResetPreview(id: string, r: ResetRequest, enabled: boolean) {
  return useQuery({
    queryKey: groupKeys.reset(id, r),
    queryFn: ({ signal }) => api<ResetView>(`/api/v1/groups/${seg(id)}/reset${query([["topic", r.topic], ["to", r.to], ["value", r.value]])}`, {}, signal),
    staleTime: 0,
    gcTime: 0,
    enabled,
    placeholderData: keepPreviousData,
  });
}

export function useDeletePreview(id: string, enabled: boolean) {
  return useQuery({ queryKey: groupKeys.delete(id), queryFn: ({ signal }) => api<GroupDeleteView>(`/api/v1/groups/${seg(id)}/delete`, {}, signal), staleTime: 0, gcTime: 0, enabled });
}

export function useOffsetsPreview(id: string, enabled: boolean) {
  return useQuery({ queryKey: groupKeys.offsets(id), queryFn: ({ signal }) => api<OffsetDeleteView>(`/api/v1/groups/${seg(id)}/offsets/delete`, {}, signal), staleTime: 0, gcTime: 0, enabled });
}

function post<T>(path: string, body: unknown) {
  return api<T>(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
}

/** After a write: the group's pages, the list and the sidebar's count are fetched again (a refusal changed nothing). */
function useAfterWrite(id: string) {
  const qc = useQueryClient();
  return (o: GroupWriteOutcome) => {
    if (o.outcome !== "done") return;
    void qc.invalidateQueries({ queryKey: groupKeys.group(id) });
    void qc.invalidateQueries({ queryKey: groupKeys.all });
    void qc.invalidateQueries({ queryKey: keys.shell });
  };
}

export function useResetOffsets(id: string) {
  const after = useAfterWrite(id);
  return useMutation({ mutationFn: (r: ResetRequest) => post<GroupWriteOutcome>(`/api/v1/groups/${seg(id)}/reset`, r), onSuccess: after });
}

/** Deletes a group. The list and the shell are fetched again; the group's own queries are for `forgetGroup`, once its
 * page is gone (dropped while the page still shows, they would be asked again, and answer 404). */
export function useDeleteGroup(id: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => post<GroupWriteOutcome>(`/api/v1/groups/${seg(id)}/delete`, {}),
    onSuccess: (o) => {
      if (o.outcome !== "done") return;
      void qc.invalidateQueries({ queryKey: groupKeys.all });
      void qc.invalidateQueries({ queryKey: keys.shell });
    },
  });
}

/** Drops a deleted group's queries without asking for them again. */
export function useForgetGroup() {
  const qc = useQueryClient();
  return (id: string) => qc.removeQueries({ queryKey: groupKeys.group(id) });
}

export function useDeleteOffsets(id: string) {
  const after = useAfterWrite(id);
  return useMutation({ mutationFn: (r: OffsetDeleteRequest) => post<GroupWriteOutcome>(`/api/v1/groups/${seg(id)}/offsets/delete`, r), onSuccess: after });
}

// The Data page (docs/port/data-control.md): what is using the disk, and the three ways to give it back. The page follows the
// RefreshControl (each look also measures growth); what Empty topic will remove is a form preview, fetched when its dialog
// opens. The writes are the topic writes' form-encoded POSTs, answered `done`, `refused` or `invalid`.
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { usePollInterval } from "@/lib/refresh";
import { FORM, lookup } from "./topic-writes";
import type { DataResponse } from "./types/DataResponse";
import type { EmptyForm } from "./types/EmptyForm";

export const dataKeys = {
  page: ["data"] as const,
  empty: (topic: string) => [FORM, "data-empty", topic] as const,
};

/** The page; `{missing}` (the console may not describe the cluster) is thrown with its words. */
export function useData() {
  const refetchInterval = usePollInterval();
  return useQuery({
    queryKey: dataKeys.page,
    queryFn: ({ signal }) => lookup<DataResponse>("/api/v1/data", signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
    retry: false,
  });
}

/** What Empty topic will remove from `topic`. */
export function useEmptyForm(topic: string, enabled: boolean) {
  return useQuery({
    queryKey: dataKeys.empty(topic),
    queryFn: ({ signal }) => lookup<EmptyForm>(`/api/v1/data/empty?topic=${encodeURIComponent(topic)}`, signal),
    staleTime: 0,
    gcTime: 30_000,
    enabled: enabled && topic !== "",
    retry: false,
  });
}

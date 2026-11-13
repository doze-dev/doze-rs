// TanStack Query hooks, one per endpoint. Every page is a snapshot (live-design.md): `staleTime: Infinity`, no refetch on
// focus; the RefreshControl's interval (Off by default) is the only timer, and `keepPreviousData` keeps what is on screen
// while a refresh is in flight, so nothing blanks or flickers. Structural sharing keeps unchanged objects identical, so
// unchanged rows do not re-render.
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { intervalMs, useRefreshInterval } from "@/lib/refresh";
import { api } from "./client";
import type { ConnectResponse } from "./types/ConnectResponse";
import type { HomeResponse } from "./types/HomeResponse";
import type { JumpView } from "./types/JumpView";
import type { ProbeView } from "./types/ProbeView";
import type { ShellView } from "./types/ShellView";

export const keys = {
  shell: ["shell"] as const,
  home: ["home"] as const,
  connect: ["connect"] as const,
  jump: (q: string) => ["jump", q] as const,
};

/** The interval of a page that follows the RefreshControl. */
export function useInterval(): number | false {
  const [v] = useRefreshInterval();
  const ms = intervalMs(v);
  return ms > 0 ? ms : false;
}

export function useShell() {
  const refetchInterval = useInterval();
  return useQuery({
    queryKey: keys.shell,
    queryFn: ({ signal }) => api<ShellView>("/api/v1/shell", {}, signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
  });
}

export function useHome() {
  const refetchInterval = useInterval();
  return useQuery({
    queryKey: keys.home,
    queryFn: ({ signal }) => api<HomeResponse>("/api/v1/home", {}, signal),
    staleTime: Infinity,
    refetchInterval,
    placeholderData: keepPreviousData,
  });
}

/** Connect is a snapshot with no refresh control (live-design.md's table). */
export function useConnect() {
  return useQuery({
    queryKey: keys.connect,
    queryFn: ({ signal }) => api<ConnectResponse>("/api/v1/connect", {}, signal),
    staleTime: Infinity,
  });
}

export function useJump(q: string, enabled: boolean) {
  return useQuery({
    queryKey: keys.jump(q),
    queryFn: ({ signal }) => api<JumpView>(`/api/v1/jump?q=${encodeURIComponent(q)}`, {}, signal),
    staleTime: 10_000,
    enabled,
    placeholderData: keepPreviousData,
  });
}

/** Run the advertised-listener check again; the answer replaces the probe on Home and Connect. */
export function useProbe() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api<ProbeView>("/api/v1/probe", { method: "POST" }),
    onSuccess: (probe) => {
      qc.setQueryData<HomeResponse>(keys.home, (old) => (old ? { ...old, home: { ...old.home, probe } } : old));
      qc.setQueryData<ConnectResponse>(keys.connect, (old) => (old ? { ...old, connect: { ...old.connect, probe } } : old));
      void qc.invalidateQueries({ queryKey: keys.home });
      void qc.invalidateQueries({ queryKey: keys.connect });
    },
  });
}

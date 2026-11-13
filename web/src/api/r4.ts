// The Query hooks of Settings, Access and the Lab (slice R4). Pages are snapshots (live-design.md): Settings and Access have
// no refresh control (the table in live-design.md); the Lab follows the RefreshControl because its rules count hits.
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { intervalMs, useRefreshInterval } from "@/lib/refresh";
import { api } from "./client";
import type { AccessResponse } from "./types/AccessResponse";
import type { AclCreateView } from "./types/AclCreateView";
import type { AclDeleteView } from "./types/AclDeleteView";
import type { LabCompact } from "./types/LabCompact";
import type { LabForm } from "./types/LabForm";
import type { LabOnceForm } from "./types/LabOnceForm";
import type { LabResponse } from "./types/LabResponse";
import type { LabRuleResponse } from "./types/LabRuleResponse";
import type { SettingsEditView } from "./types/SettingsEditView";
import type { SettingsResponse } from "./types/SettingsResponse";

export const r4keys = {
  settings: ["settings"] as const,
  settingsEdit: (key: string, scope: string) => ["settings", "edit", key, scope] as const,
  access: (state: string) => ["access", state] as const,
  aclNew: (q: string) => ["access", "new", q] as const,
  aclDelete: (q: string) => ["access", "delete", q] as const,
  lab: ["lab"] as const,
  labRule: (id: string) => ["lab", "rule", id] as const,
  labForm: (kind: string) => ["lab", "form", kind] as const,
  labOnce: (kind: string, q: string) => ["lab", "once", kind, q] as const,
  labCompact: (topic: string, partition: number) => ["lab", "compact", topic, partition] as const,
};

/** A POST with a JSON body (the client adds the console's proof header). */
export function post<T>(path: string, body: unknown): Promise<T> {
  return api<T>(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
}

/** The query string of an object's non-empty values. */
export function qs(o: Record<string, string | number | undefined>): string {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(o)) if (v !== undefined && v !== "") p.set(k, String(v));
  return p.toString();
}

function useInterval(): number | false {
  const [v] = useRefreshInterval();
  const ms = intervalMs(v);
  return ms > 0 ? ms : false;
}

export function useSettings() {
  return useQuery({ queryKey: r4keys.settings, queryFn: ({ signal }) => api<SettingsResponse>("/api/v1/settings", {}, signal), staleTime: Infinity, placeholderData: keepPreviousData });
}

export function useSettingsEdit(key: string | undefined, scope: string) {
  return useQuery({
    queryKey: r4keys.settingsEdit(key ?? "", scope),
    queryFn: ({ signal }) => api<SettingsEditView>(`/api/v1/settings/edit?${qs({ key, scope })}`, {}, signal),
    enabled: !!key,
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  });
}

export function useAccess(state = "") {
  return useQuery({ queryKey: r4keys.access(state), queryFn: ({ signal }) => api<AccessResponse>(`/api/v1/access${state ? `?state=${encodeURIComponent(state)}` : ""}`, {}, signal), staleTime: Infinity, placeholderData: keepPreviousData });
}

export function useAclNew(q: string, enabled: boolean) {
  return useQuery({ queryKey: r4keys.aclNew(q), queryFn: ({ signal }) => api<AclCreateView>(`/api/v1/access/acls/new?${q}`, {}, signal), enabled, staleTime: Infinity, placeholderData: keepPreviousData });
}

export function useAclDelete(q: string | undefined) {
  return useQuery({ queryKey: r4keys.aclDelete(q ?? ""), queryFn: ({ signal }) => api<AclDeleteView>(`/api/v1/access/acls/delete?${q}`, {}, signal), enabled: !!q, staleTime: Infinity });
}

export function useLab() {
  const refetchInterval = useInterval();
  return useQuery({ queryKey: r4keys.lab, queryFn: ({ signal }) => api<LabResponse>("/api/v1/lab", {}, signal), staleTime: Infinity, refetchInterval, placeholderData: keepPreviousData });
}

export function useLabRule(id: string | undefined) {
  const refetchInterval = useInterval();
  return useQuery({ queryKey: r4keys.labRule(id ?? ""), queryFn: ({ signal }) => api<LabRuleResponse>(`/api/v1/lab/rules/${encodeURIComponent(id ?? "")}`, {}, signal), enabled: !!id, staleTime: Infinity, refetchInterval, placeholderData: keepPreviousData, retry: false });
}

export function useLabForm(kind: "slow" | "fail", enabled: boolean) {
  return useQuery({ queryKey: r4keys.labForm(kind), queryFn: ({ signal }) => api<LabForm>(`/api/v1/lab/form?kind=${kind}`, {}, signal), enabled, staleTime: 0 });
}

export function useLabOnce(kind: "drop" | "expire" | "rebalance", q: string, enabled: boolean) {
  return useQuery({ queryKey: r4keys.labOnce(kind, q), queryFn: ({ signal }) => api<LabOnceForm>(`/api/v1/lab/once/${kind}?${q}`, {}, signal), enabled, staleTime: 0, placeholderData: keepPreviousData });
}

export function useLabCompact(topic: string, partition: number, enabled: boolean) {
  return useQuery({ queryKey: r4keys.labCompact(topic, partition), queryFn: ({ signal }) => api<LabCompact>(`/api/v1/lab/compact?${qs({ topic, partition })}`, {}, signal), enabled: enabled && topic !== "", staleTime: 0, retry: false });
}

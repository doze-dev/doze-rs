// The Lab's rules and forced events (crates/aws-console/src/api/{lab_rules,lab_once}.rs). Reading the rules makes no call to a service, so it
// follows the page's refresh choice like every data page; the forms of the forced events read the services (through the bus, as the console),
// and only when one is open.
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { usePollInterval } from "@doze/ui/lib/refresh";
import { client } from "./client";
import type { LabCheck } from "./types/LabCheck";
import type { LabCleared } from "./types/LabCleared";
import type { LabGuide } from "./types/LabGuide";
import type { LabMade } from "./types/LabMade";
import type { LabOnceDone } from "./types/LabOnceDone";
import type { LabOnceForm } from "./types/LabOnceForm";
import type { LabOnceGuide } from "./types/LabOnceGuide";
import type { LabOnceIndex } from "./types/LabOnceIndex";
import type { LabRule } from "./types/LabRule";
import type { LabRuleBody } from "./types/LabRuleBody";
import type { LabRules } from "./types/LabRules";
import type { LabRuleView } from "./types/LabRuleView";
import type { LabStopped } from "./types/LabStopped";

const enc = encodeURIComponent;
const post = (body: unknown = {}): RequestInit => ({ method: "POST", body: JSON.stringify(body), headers: { "Content-Type": "application/json" } });

export const LAB_PATHS = {
  rules: "/api/v1/lab/rules",
  check: "/api/v1/lab/rules/check",
  rule: (id: number) => `/api/v1/lab/rules/${id}`,
  stopAll: "/api/v1/lab/stop-all",
  clearEnded: "/api/v1/lab/history/clear",
  guide: "/api/v1/lab/guide",
  once: "/api/v1/lab/once",
  onceGuide: "/api/v1/lab/once/guide",
  onceClear: "/api/v1/lab/once/history/clear",
  onceKind: (kind: string) => `/api/v1/lab/once/${enc(kind)}`,
  onceForm: (kind: string, choice: string | null, look: boolean) => `/api/v1/lab/once/${enc(kind)}/form?${[choice ? `choice=${enc(choice)}` : "", look ? "look=yes" : ""].filter(Boolean).join("&")}`,
} as const;

/** The rules in force and those that ended. Read again after every change made here, and by the refresh choice. */
export function useLabRules() {
  const refetchInterval = usePollInterval();
  return useQuery({ queryKey: ["lab", "rules"], queryFn: ({ signal }) => client.api<LabRules>(LAB_PATHS.rules, {}, signal), refetchInterval });
}

/** The words of the Lab: what it is, and one lesson for each rehearsal. Fixed for the life of the process. */
export function useLabGuide() {
  return useQuery({ queryKey: ["lab", "guide"], queryFn: ({ signal }) => client.api<LabGuide>(LAB_PATHS.guide, {}, signal), staleTime: Number.POSITIVE_INFINITY });
}

/** What a rule would be, said in a sentence with its problems in words: asked as the form changes (the caller debounces). */
export function useLabCheck(body: LabRuleBody | null, enabled = true) {
  return useQuery({
    queryKey: ["lab", "check", body],
    queryFn: ({ signal }) => client.api<LabCheck>(LAB_PATHS.check, post(body), signal),
    enabled: enabled && body !== null,
    placeholderData: keepPreviousData,
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}

/** The rule and the calls it touched that Live still holds. */
export function useLabRule(id: number | null) {
  return useQuery({ queryKey: ["lab", "rule", id], queryFn: ({ signal }) => client.api<LabRuleView>(LAB_PATHS.rule(id ?? 0), {}, signal), enabled: id !== null, retry: false });
}

function useChanged() {
  const qc = useQueryClient();
  return () => void qc.invalidateQueries({ queryKey: ["lab"] });
}

/** Makes a rule: `made` with the rule, or `refused` with what is wrong with it. */
export function useMakeRule() {
  const changed = useChanged();
  return useMutation({ mutationFn: (body: LabRuleBody) => client.api<LabMade>(LAB_PATHS.rules, post(body)), onSuccess: changed });
}

export function useSwitchRule() {
  const changed = useChanged();
  return useMutation({ mutationFn: ({ id, enabled }: { id: number; enabled: boolean }) => client.api<LabRule>(`${LAB_PATHS.rule(id)}/switch`, post({ enabled })), onSuccess: changed });
}

export function useRemoveRule() {
  const changed = useChanged();
  return useMutation({ mutationFn: (id: number) => client.api<LabRule>(`${LAB_PATHS.rule(id)}/remove`, post()), onSuccess: changed });
}

export function useStopAll() {
  const changed = useChanged();
  return useMutation({ mutationFn: () => client.api<LabStopped>(LAB_PATHS.stopAll, post()), onSuccess: changed });
}

export function useClearEnded() {
  const changed = useChanged();
  return useMutation({ mutationFn: () => client.api<LabCleared>(LAB_PATHS.clearEnded, post()), onSuccess: changed });
}

/** The kinds of forced event this run can carry out, and what was forced lately. */
export function useOnceIndex() {
  const refetchInterval = usePollInterval();
  return useQuery({ queryKey: ["lab", "once"], queryFn: ({ signal }) => client.api<LabOnceIndex>(LAB_PATHS.once, {}, signal), refetchInterval });
}

export function useOnceGuide() {
  return useQuery({ queryKey: ["lab", "once-guide"], queryFn: ({ signal }) => client.api<LabOnceGuide>(LAB_PATHS.onceGuide, {}, signal), staleTime: Number.POSITIVE_INFINITY });
}

/** What a forced event can act on in the region chosen. Reading it is a call to the service (a resting one is not called unless `look`). */
export function useOnceForm(kind: string | null, choice: string | null, look: boolean) {
  return useQuery({
    queryKey: ["lab", "once-form", kind, choice, look],
    queryFn: ({ signal }) => client.api<LabOnceForm>(LAB_PATHS.onceForm(kind ?? "", choice, look), {}, signal),
    enabled: kind !== null,
    placeholderData: keepPreviousData,
    staleTime: 0,
    retry: false,
  });
}

/** Carries a forced event out. A refusal in words is an answer (`ok: false`), not an error. Only the Lab's own lists are read again: what Resources
 * lists is not, because reading it is a call, and a call would start the service that was just rested. */
export function useForce(kind: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: { choice: string; time_wake?: boolean }) => client.api<LabOnceDone>(LAB_PATHS.onceKind(kind), post(body)),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["lab"] });
    },
  });
}

export function useClearOnce() {
  const changed = useChanged();
  return useMutation({ mutationFn: () => client.api<LabOnceIndex>(LAB_PATHS.onceClear, post()), onSuccess: changed });
}

// The queries and actions of Home, Access, Lab and Settings (crates/aws-console/src/api/{home,access,lab,settings}.rs).
// Home and Access read the Live record and IAM's own state, which make no AWS call; Home's checks and every write go through
// the services' bus as the caller `console`, which Live shows (hidden by default).
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { usePollInterval } from "@doze/ui/lib/refresh";
import { client, PATHS } from "./client";
import type { AccessView } from "./types/AccessView";
import type { Attached } from "./types/Attached";
import type { HeldPolicies } from "./types/HeldPolicies";
import type { HomeChecks } from "./types/HomeChecks";
import type { HomeView } from "./types/HomeView";
import type { LabAnswer } from "./types/LabAnswer";
import type { LabIndex } from "./types/LabIndex";
import type { LabOperation } from "./types/LabOperation";
import type { PrincipalsResponse } from "./types/PrincipalsResponse";
import type { ResetDone } from "./types/ResetDone";
import type { SettingsView } from "./types/SettingsView";

const enc = encodeURIComponent;

export const SPACE_PATHS = {
  home: "/api/v1/home",
  checks: "/api/v1/home/checks",
  principals: "/api/v1/principals",
  access: "/api/v1/access",
  attach: "/api/v1/access/attach",
  policies: (kind: string, name: string) => `/api/v1/access/policies?kind=${kind}&name=${encodeURIComponent(name)}`,
  settings: "/api/v1/settings",
  reset: "/api/v1/settings/reset",
  lab: "/api/v1/lab",
  op: (target: string, op: string) => `/api/v1/lab/${enc(target)}/${enc(op)}`,
} as const;

const post = (body: unknown): RequestInit => ({ method: "POST", body: JSON.stringify(body), headers: { "Content-Type": "application/json" } });

/** `GET /api/v1/home`: from the Live record (no call is made), read again when a call arrives (see {@link useArrivals}). */
export function useHome() {
  const refetchInterval = usePollInterval();
  return useQuery({ queryKey: ["home"], queryFn: ({ signal }) => client.api<HomeView>(SPACE_PATHS.home, {}, signal), refetchInterval });
}

/** `GET /api/v1/home/checks`: calls the services apps called (Live shows them as the console's). Read when Home opens and when
 * the person refreshes, never on a timer of its own. */
export function useHomeChecks() {
  return useQuery({ queryKey: ["home", "checks"], queryFn: ({ signal }) => client.api<HomeChecks>(SPACE_PATHS.checks, {}, signal), staleTime: 30_000 });
}

/** Every caller in the Live record, named as IAM knows it. `keys` is what the page holds: a caller it has not seen asks again. */
export function usePrincipals(keys: string) {
  return useQuery({ queryKey: ["principals", keys], queryFn: ({ signal }) => client.api<PrincipalsResponse>(SPACE_PATHS.principals, {}, signal), placeholderData: keepPreviousData, staleTime: 60_000, enabled: keys !== "" });
}

export function useAccess() {
  const refetchInterval = usePollInterval();
  return useQuery({ queryKey: ["access"], queryFn: ({ signal }) => client.api<AccessView>(SPACE_PATHS.access, {}, signal), refetchInterval });
}

/** The policies a user or a role holds (inline with their documents, managed attached), and the groups not read. */
export function useHeldPolicies(kind: "user" | "role" | null, name: string | null) {
  return useQuery({ queryKey: ["access", "policies", kind, name], queryFn: ({ signal }) => client.api<HeldPolicies>(SPACE_PATHS.policies(kind ?? "", name ?? ""), {}, signal), enabled: kind !== null && !!name, staleTime: 5_000 });
}

export type AttachInput = { kind: "user" | "role"; name: string; policy_name: string; document: string };

export function useAttach() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: AttachInput) => client.api<Attached>(SPACE_PATHS.attach, post(input)),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["access"] });
      void qc.invalidateQueries({ queryKey: ["resources"] });
    },
  });
}

export function useSettings() {
  return useQuery({ queryKey: ["settings"], queryFn: ({ signal }) => client.api<SettingsView>(SPACE_PATHS.settings, {}, signal) });
}

export function useReset() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (dryRun: boolean) => client.api<ResetDone>(SPACE_PATHS.reset, post({ dry_run: dryRun })),
    onSuccess: (done) => {
      if (!done.dry_run) void qc.invalidateQueries();
    },
  });
}

export function useLab() {
  return useQuery({ queryKey: ["lab"], queryFn: ({ signal }) => client.api<LabIndex>(SPACE_PATHS.lab, {}, signal), staleTime: Number.POSITIVE_INFINITY });
}

export function useLabOperation(target: string, op: string) {
  return useQuery({ queryKey: ["lab", target, op], queryFn: ({ signal }) => client.api<LabOperation>(SPACE_PATHS.op(target, op), {}, signal), enabled: target !== "" && op !== "", staleTime: Number.POSITIVE_INFINITY });
}

export function useLabSend() {
  return useMutation({ mutationFn: ({ target, op, input }: { target: string; op: string; input: unknown }) => client.api<LabAnswer>(SPACE_PATHS.op(target, op), post(input)) });
}

/** Calls `onCalls` each time calls arrive in the Live record, while the page is on screen: the tail's stream, read for its
 * events only. A stream that ends is opened again after a pause; nothing runs once the page is left. */
export function useArrivals(onCalls: () => void) {
  useEffect(() => {
    const ac = new AbortController();
    const { signal } = ac;
    void (async () => {
      for (let failures = 0; !signal.aborted; failures += 1) {
        try {
          await client.streamEvents(
            PATHS.tail,
            (e) => {
              if (e.event === "calls") onCalls();
              if (e.event === "ready") failures = 0;
            },
            signal,
          );
        } catch {
          /* tried again below */
        }
        if (signal.aborted) return;
        await new Promise<void>((resolve) => {
          const t = setTimeout(resolve, Math.min(10_000, 1_000 * 2 ** failures));
          signal.addEventListener("abort", () => (clearTimeout(t), resolve()), { once: true });
        });
      }
    })();
    return () => ac.abort();
  }, [onCalls]);
}

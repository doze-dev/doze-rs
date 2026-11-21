// What the IAM pages read and write: IAM's own operations, made through the kinds' operation lists (crates/aws-console/src/resources/
// iam_ops.rs) as the caller `console`, and the calls the Live record holds. A write makes every IAM read stale.
import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { client, PATHS } from "../../../api/client";
import type { CallRecord } from "../../../api/types/CallRecord";
import type { CallsPage } from "../../../api/types/CallsPage";
import { callKindOp, type Kind, useKindOpRead } from "../../../kit";

export const USERS: Kind = { service: "iam", kind: "users" };
export const GROUPS: Kind = { service: "iam", kind: "groups" };
export const ROLES: Kind = { service: "iam", kind: "roles" };
export const POLICIES: Kind = { service: "iam", kind: "policies" };

/** One operation of a kind, read when the page opens and again when its input changes. */
export function useRead<T>(kind: Kind, operation: string, input: object = {}, enabled = true) {
  return useKindOpRead<T>(kind, operation, input, enabled);
}

const keyOf = (kind: Kind, operation: string, input: object) => ["resources", "kindop", kind.service, kind.kind, operation, input];

/** The same operation once for each of `inputs` (a user's keys for every user of a list): the answers in the order of the inputs. */
export function useEach<T>(kind: Kind, operation: string, inputs: readonly object[], enabled = true) {
  return useQueries({
    queries: inputs.map((input) => ({ queryKey: keyOf(kind, operation, input), queryFn: ({ signal }: { signal: AbortSignal }) => callKindOp<T>(kind, operation, input, signal), enabled, retry: false })),
  });
}

/** Reads everything IAM again: the lists, the pages of resources, their tags, and what the pages read through operations. `gone` is a
 * resource that was just deleted: what is read about it is marked stale but not read again (IAM would answer NoSuchEntity). */
export function useRefresh(): (gone?: string) => void {
  const qc = useQueryClient();
  return (gone) => {
    const iam = (key: readonly unknown[]) => (key[0] === "resources" && key[2] === "iam") || key[0] === "iam";
    const about = (key: readonly unknown[]) => gone !== undefined && key[1] !== "list" && JSON.stringify(key).includes(JSON.stringify(gone).slice(1, -1));
    void qc.invalidateQueries({ predicate: (q) => iam(q.queryKey) && !about(q.queryKey) });
    if (gone !== undefined) void qc.invalidateQueries({ predicate: (q) => iam(q.queryKey) && about(q.queryKey), refetchType: "none" });
  };
}

/** One write of a kind. What IAM read is stale after it. */
export function useWrite<T = unknown, I extends object = object>(kind: Kind, operation: string) {
  const refresh = useRefresh();
  return useMutation({ mutationFn: (input: I) => callKindOp<T>(kind, operation, input), onSuccess: () => refresh() });
}

/** The calls the record holds that IAM or STS answered, newest first (the record is read once and kept a moment). */
export function useIamCalls() {
  return useQuery({ queryKey: ["iam", "calls"], queryFn: ({ signal }) => client.api<CallsPage>(`${PATHS.calls}?limit=500`, {}, signal), retry: false, staleTime: 5_000 });
}

export const iamCalls = (page: CallsPage | undefined): CallRecord[] => (page?.calls ?? []).filter((c) => c.service === "iam" || c.service === "sts").reverse();

/** The names of the IAM entities of a kind, from a list answer: what a form's "already exists" hint says. */
export const namesOf = (rows: readonly { name: string }[] | undefined): string[] => (rows ?? []).map((r) => r.name);

/** The sessions the record shows taken of each role: how many, how many in the last hour, and the IAM users whose keys took them. */
export type Assumed = { count: number; lastHour: number; by: string[] };

export function assumedOf(calls: readonly CallRecord[], role: string, names: ReadonlyMap<string, string>, now: number): Assumed {
  const mine = calls.filter((c) => c.service === "sts" && c.action === "AssumeRole" && c.status < 400 && c.caller !== "console" && (c.resource ?? "").split("/").pop() === role && (c.resource ?? "").includes(":role/"));
  return { count: mine.length, lastHour: mine.filter((c) => now - c.at_ms <= 3_600_000).length, by: [...new Set(mine.map((c) => (c.caller ? names.get(c.caller) : undefined)).filter((n): n is string => Boolean(n)))] };
}

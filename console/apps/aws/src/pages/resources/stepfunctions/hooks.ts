// What the Step Functions pages read besides a resource's own detail: how many executions of each machine are running, the
// account's roles and log groups for the studio, and where the pages go.
import { useNavigate } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { client, PATHS } from "../../../api/client";
import { useAbout } from "../../../api/queries";
import type { CallsPage } from "../../../api/types/CallsPage";
import { callOp, on } from "../deep/api";

/** How many executions of each of these machines are running (ListExecutions with the RUNNING filter, a machine at a time). An
 * Express machine keeps none to list, so it is not asked. */
export function useRunning(machines: readonly { name: string; express: boolean }[], enabled = true) {
  const names = machines.filter((m) => !m.express).map((m) => m.name);
  return useQuery({
    queryKey: ["stepfunctions", "running", ...names],
    enabled: enabled && names.length > 0,
    retry: false,
    queryFn: async ({ signal }) => {
      const out: Record<string, number> = {};
      await Promise.all(
        names.slice(0, 25).map(async (name) => {
          const r = await callOp<{ executions?: unknown[] }>(on.machine(name), "ListExecutions", { statusFilter: "RUNNING", maxResults: 100 }, signal);
          out[name] = r.executions?.length ?? 0;
        }),
      );
      return out;
    },
  });
}

/** The region and account the console is for. */
export function useWhere(): { region: string; account: string } {
  const about = useAbout().data;
  return { region: about?.region ?? "us-east-1", account: about?.account ?? "000000000000" };
}

/** Where the pages of Step Functions go. */
export function useGoTo() {
  const navigate = useNavigate();
  return {
    list: (search: Record<string, string> = {}) => void navigate({ to: "/resources/$service/$second", params: { service: "stepfunctions", second: "machines" }, search }),
    activities: (search: Record<string, string> = {}) => void navigate({ to: "/resources/$service/$second", params: { service: "stepfunctions", second: "activities" }, search }),
    machine: (name: string, search: Record<string, string> = {}) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "stepfunctions", kind: "machines", name }, search }),
    execution: (arn: string, search: Record<string, string> = {}) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "stepfunctions", kind: "executions", name: arn }, search }),
  };
}

export const machineHref = (name: string, search: Record<string, string> = {}): string => {
  const q = new URLSearchParams(search).toString();
  return `/resources/stepfunctions/machines/${encodeURIComponent(name)}${q ? `?${q}` : ""}`;
};

type Listed = { executions?: { executionArn: string; name: string; status: string }[] };

/** What a machine's sentence says of its runs: how many are running, whether it ever ran, and the last to fail with the state it
 * stopped at and its error (read from DescribeExecution and the end of its history). An Express machine keeps no history to read. */
export type Runs = { running: number; never: boolean; failed: { name: string; arn: string; error?: string; state?: string } | null };

export function useMachineRuns(name: string, express: boolean, enabled = true) {
  const t = on.machine(name);
  return useQuery({
    queryKey: ["resources", "deep", "stepfunctions", "machines", name, "runs"],
    enabled: enabled && !express,
    retry: false,
    queryFn: async ({ signal }): Promise<Runs> => {
      const [running, failed, any] = await Promise.all([
        callOp<Listed>(t, "ListExecutions", { statusFilter: "RUNNING", maxResults: 100 }, signal),
        callOp<Listed>(t, "ListExecutions", { statusFilter: "FAILED", maxResults: 1 }, signal),
        callOp<Listed>(t, "ListExecutions", { maxResults: 1 }, signal),
      ]);
      const last = failed.executions?.[0];
      let detail: Runs["failed"] = null;
      if (last) {
        const [d, h] = await Promise.all([
          callOp<{ error?: string }>(t, "DescribeExecution", { executionArn: last.executionArn }, signal),
          callOp<{ events?: { type: string; stateEnteredEventDetails?: { name: string } }[] }>(t, "GetExecutionHistory", { executionArn: last.executionArn, reverseOrder: true, maxResults: 100 }, signal),
        ]);
        detail = { name: last.name, arn: last.executionArn, error: d.error, state: h.events?.find((e) => e.stateEnteredEventDetails)?.stateEnteredEventDetails?.name };
      }
      return { running: running.executions?.length ?? 0, never: (any.executions?.length ?? 0) === 0, failed: detail };
    },
  });
}

export type VersionRow = { arn: string; number: string; description: string; created: number | string; revisionId: string };
export type AliasRow = { arn: string; name: string; description: string; routing: { stateMachineVersionArn: string; weight: number }[]; created: number | string; updated: number | string };

/** A machine's versions, the newest first, each read for its description (a listing names only the ARN and the date). */
export function useVersions(name: string, enabled = true) {
  return useQuery({
    queryKey: ["resources", "deep", "stepfunctions", "machines", name, "versions"],
    enabled,
    retry: false,
    queryFn: async ({ signal }): Promise<VersionRow[]> => {
      const t = on.machine(name);
      const list = await callOp<{ stateMachineVersions?: { stateMachineVersionArn: string; creationDate: number | string }[] }>(t, "ListStateMachineVersions", { maxResults: 100 }, signal);
      return Promise.all(
        (list.stateMachineVersions ?? []).map(async (v) => {
          const one = await callOp<{ description?: string; revisionId?: string }>(t, "DescribeStateMachine", { stateMachineArn: v.stateMachineVersionArn }, signal);
          return { arn: v.stateMachineVersionArn, number: v.stateMachineVersionArn.split(":").pop() ?? "", description: one.description ?? "", created: v.creationDate, revisionId: one.revisionId ?? "" };
        }),
      );
    },
  });
}

/** A machine's aliases, each read for its description, routing and update date. */
export function useAliases(name: string) {
  return useQuery({
    queryKey: ["resources", "deep", "stepfunctions", "machines", name, "aliases"],
    retry: false,
    queryFn: async ({ signal }): Promise<AliasRow[]> => {
      const t = on.machine(name);
      const list = await callOp<{ stateMachineAliases?: { stateMachineAliasArn: string; creationDate: number | string }[] }>(t, "ListStateMachineAliases", { maxResults: 100 }, signal);
      return Promise.all(
        (list.stateMachineAliases ?? []).map(async (a) => {
          const one = await callOp<{ description?: string; routingConfiguration?: AliasRow["routing"]; updateDate?: number | string }>(t, "DescribeStateMachineAlias", { stateMachineAliasArn: a.stateMachineAliasArn }, signal);
          return { arn: a.stateMachineAliasArn, name: a.stateMachineAliasArn.split(":").pop() ?? "", description: one.description ?? "", routing: one.routingConfiguration ?? [], created: a.creationDate, updated: one.updateDate ?? a.creationDate };
        }),
      );
    },
  });
}

/** The newest calls the record holds that Step Functions answered, newest first (the worker calls of an activity are among them). */
export function useWorkerCalls() {
  return useQuery({ queryKey: ["stepfunctions", "worker-calls"], queryFn: ({ signal }) => client.api<CallsPage>(`${PATHS.calls}?limit=200&console=no`, {}, signal), retry: false });
}

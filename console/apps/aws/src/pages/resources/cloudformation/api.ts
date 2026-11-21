// CloudFormation's reads and writes from the pages: each is one operation the stacks' kind lists (crates/aws-console/src/resources/
// cfn_ops.rs), made through `POST /resources/cloudformation/stacks/op/<Operation>` as the caller `console`.
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { callKindOp } from "../../../kit";
import type { ChangeSet } from "./changeset-parts";
import { inProgress, nobodyImports, type Stack, type StackEvent, type StackResource } from "./model";

export const STACKS = { service: "cloudformation", kind: "stacks" } as const;

/** The kinds of this service, for the lists and the invalidation of them. */
export const EXPORTS = { service: "cloudformation", kind: "exports" } as const;

/** A read of one operation, made when the page opens and when its input changes. `every`: read again that often while it says so. */
export function useCfnRead<T>(operation: string, input: object, { enabled = true, every }: { enabled?: boolean; every?: (data: T | undefined) => number | false } = {}) {
  return useQuery({
    queryKey: ["cfn", operation, input],
    queryFn: ({ signal }) => callKindOp<T>(STACKS, operation, input, signal),
    enabled,
    retry: false,
    refetchInterval: every ? (q) => every(q.state.data as T | undefined) : false,
  });
}

/** The stack by its name (or its id, for a deleted one), read again every second and a half while it is being worked on. */
export function useStack(nameOrId: string, enabled = true) {
  return useCfnRead<{ Stacks?: Stack[] }>("DescribeStacks", { StackName: nameOrId }, { enabled, every: (d) => (inProgress(d?.Stacks?.[0]?.StackStatus ?? "") ? 1500 : false) });
}

export const useResources = (name: string, enabled = true) => useCfnRead<{ StackResourceSummaries?: StackResource[] }>("ListStackResources", { StackName: name }, { enabled });
export const useTemplate = (name: string, stage: "Original" | "Processed" = "Original", enabled = true) => useCfnRead<{ TemplateBody?: string | object; StagesAvailable?: string[] }>("GetTemplate", { StackName: name, TemplateStage: stage }, { enabled });
export const useEvents = (nameOrId: string, token?: string, enabled = true) => useCfnRead<{ StackEvents?: StackEvent[]; NextToken?: string }>("DescribeStackEvents", { StackName: nameOrId, ...(token ? { NextToken: token } : {}) }, { enabled });

/** The text of a template as `GetTemplate` answers: a JSON template may come as an object. */
export const templateText = (body: string | object | undefined): string => (typeof body === "string" ? body : body === undefined ? "" : JSON.stringify(body, null, 2));

/** Who imports each of these exports: the stacks that use `Fn::ImportValue` on it, none when CloudFormation says nobody does. */
export function useImporters(exports: readonly string[]) {
  return useQuery({
    queryKey: ["cfn", "importers", exports],
    queryFn: async ({ signal }) => {
      const out: Record<string, string[]> = {};
      for (const name of exports) {
        try {
          const r = await callKindOp<{ Imports?: string[] }>(STACKS, "ListImports", { ExportName: name }, signal);
          out[name] = r.Imports ?? [];
        } catch (e) {
          if (!nobodyImports(e)) throw e;
          out[name] = [];
        }
      }
      return out;
    },
    enabled: exports.length > 0,
    retry: false,
  });
}

/** The stacks that exist, by name (a name that is taken cannot be used again). */
export function useStackNames() {
  return useCfnRead<{ Stacks?: Stack[] }>("DescribeStacks", {});
}

/** Calls `refresh` every `ms` while `active`: a page of a stack that is still being worked on. */
export function usePoll(active: boolean, refresh: () => void, ms = 1500): void {
  useEffect(() => {
    if (!active) return;
    const t = setInterval(refresh, ms);
    return () => clearInterval(t);
  }, [active, refresh, ms]);
}

/** After a write to a stack, what the pages read of it is stale. */
export function useRefreshStack() {
  const qc = useQueryClient();
  return () => {
    void qc.invalidateQueries({ queryKey: ["cfn"] });
    void qc.invalidateQueries({ queryKey: ["resources", "one", "cloudformation"] });
    void qc.invalidateQueries({ queryKey: ["resources", "list", "cloudformation"] });
  };
}

/** A write: one operation of the stacks' kind; every read of a stack is stale after it, whatever came back. */
export function useCfnWrite<T, I extends object = object>(operation: string) {
  const refresh = useRefreshStack();
  return useMutation({ mutationFn: (input: I) => callKindOp<T>(STACKS, operation, input), onSettled: refresh });
}

/** The preview of an update: a change set the console makes while the review step is open (made again when the choices change) and
 * removes when the page is left, unless `keep.current` (the person went on to read it). `input` is `null` while there is nothing to preview. */
export function useCfnPreview(stack: string, id: string, key: string, input: Record<string, unknown> | null, keep: React.RefObject<boolean>) {
  const q = useQuery({
    queryKey: ["cfn", "preview", stack, id, key],
    queryFn: async () => {
      await callKindOp(STACKS, "DeleteChangeSet", { StackName: stack, ChangeSetName: id }).catch(() => undefined);
      await callKindOp(STACKS, "CreateChangeSet", input ?? {});
      return callKindOp<ChangeSet>(STACKS, "DescribeChangeSet", { StackName: stack, ChangeSetName: id });
    },
    enabled: input !== null,
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
    gcTime: 0,
  });
  useEffect(
    () => () => {
      if (!keep.current && stack) void callKindOp(STACKS, "DeleteChangeSet", { StackName: stack, ChangeSetName: id }).catch(() => undefined);
    },
    [stack, id, keep],
  );
  return { data: q.data, pending: input !== null && q.isPending, error: q.error };
}

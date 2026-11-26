// CloudFormation's reads and writes from the pages: each is one operation the stacks' kind lists (crates/aws-console/src/resources/
// cfn_ops.rs), made through `POST /resources/cloudformation/stacks/op/<Operation>` as the caller `console`.
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { callKindOp, forgetDeleted, scopeOf } from "../../../kit";
import { useNow } from "../../card";
import type { ChangeSet } from "./changeset-parts";
import { inProgress, type Stack, type StackEvent, type StackResource } from "./model";
import { BEAT_MS, beat, stalePreviews, unbeat } from "./preview-sweep";
import { keptPreview } from "./wizard-keep";

export const STACKS = { service: "cloudformation", kind: "stacks" } as const;

/** The kinds of this service, for the lists and the invalidation of them. */
export const EXPORTS = { service: "cloudformation", kind: "exports" } as const;

/** A read of one operation, made when the page opens and when its input changes. `every`: read again that often while it says so.
 * `scope`: a word that is part of the read's identity but not of what is asked (a read made under another scope is made afresh). */
export function useCfnRead<T>(operation: string, input: object, { enabled = true, every, scope }: { enabled?: boolean; every?: (data: T | undefined) => number | false; scope?: string } = {}) {
  return useQuery({
    queryKey: scope === undefined ? ["cfn", operation, input] : ["cfn", operation, input, scope],
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
/** A stack's events, newest first. Read again while the stack is being worked on, and under the status it settles at: the status flips
 * before a page has the last events (the stack's own UPDATE_COMPLETE), so a page that read them under the working status says only what
 * the work in progress had written, and is read afresh when the status says it is done. */
export function useEvents(nameOrId: string, token?: string, enabled = true) {
  const status = useStack(nameOrId, enabled).data?.Stacks?.[0]?.StackStatus ?? "";
  const working = inProgress(status);
  return useCfnRead<{ StackEvents?: StackEvent[]; NextToken?: string }>("DescribeStackEvents", { StackName: nameOrId, ...(token ? { NextToken: token } : {}) }, { enabled, scope: working ? "working" : status, every: () => (working ? 1500 : false) });
}

/** The text of a template as `GetTemplate` answers: a JSON template may come as an object. */
export const templateText = (body: string | object | undefined): string => (typeof body === "string" ? body : body === undefined ? "" : JSON.stringify(body, null, 2));

/** Who imports each of these exports: the stacks that use `Fn::ImportValue` on it, none when CloudFormation says nobody does. */
export function useImporters(exports: readonly string[]) {
  return useQuery({
    queryKey: ["cfn", "importers", exports],
    queryFn: async ({ signal }) => {
      const out: Record<string, string[]> = {};
      for (const name of exports) {
        // An export nobody imports answers an empty list (the console reads CloudFormation's refusal that says so as one).
        const r = await callKindOp<{ Imports?: string[] }>(STACKS, "ListImports", { ExportName: name }, signal);
        out[name] = r.Imports ?? [];
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

/** A write: one operation of the stacks' kind; every read of a stack is stale after it, whatever came back. A write that deletes
 * (`gone` names what it deleted, from its input) leaves the reads of what is gone unmade: they would be refused. */
export function useCfnWrite<T, I extends object = object>(operation: string, gone?: (input: I) => string[]) {
  const refresh = useRefreshStack();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: I) => callKindOp<T>(STACKS, operation, input),
    onSettled: (_, error, input) => (gone && !error ? forgetDeleted(qc, scopeOf("cloudformation"), gone(input)) : refresh()),
  });
}

/** The preview of an update: a change set the console makes while the review step is open (made again when the choices change) and
 * removes when the page is left, unless `keep.current` (the person went on to read it). `input` is `null` while there is nothing to preview. */
export function useCfnPreview(stack: string, id: string, key: string, input: Record<string, unknown> | null, keep: React.RefObject<boolean>, earlier = false) {
  // Whether a change set of this id is there to be replaced: one this hook made before the choices changed (or `earlier`, one a page left by
  // this id). The first preview of an id has nothing to delete, and asking would only be refused.
  const made = useRef(earlier);
  const left = useRef(false);
  const q = useQuery({
    // Not under ["cfn"]: a write to the stack (the update the person submits) would re-make the preview just as the page goes.
    queryKey: ["cfn-preview", stack, id, key],
    queryFn: async () => {
      if (made.current) await callKindOp(STACKS, "DeleteChangeSet", { StackName: stack, ChangeSetName: id }).catch(() => undefined);
      await callKindOp(STACKS, "CreateChangeSet", input ?? {});
      made.current = true;
      // The page was left while it was being made: it is removed now, nobody is going to read it.
      if (left.current && !keep.current) {
        made.current = false;
        await callKindOp(STACKS, "DeleteChangeSet", { StackName: stack, ChangeSetName: id }).catch(() => undefined);
        throw new Error("the page was left");
      }
      return callKindOp<ChangeSet>(STACKS, "DescribeChangeSet", { StackName: stack, ChangeSetName: id });
    },
    enabled: input !== null,
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
    gcTime: 0,
  });
  // While the review step is open its preview says so, so a page that finds one nobody says anything of knows to remove it.
  const live = input !== null;
  useEffect(() => {
    if (!live) return;
    const say = () => beat(id, Date.now());
    say();
    const t = setInterval(say, BEAT_MS);
    return () => {
      clearInterval(t);
      unbeat(id);
    };
  }, [live, id]);
  // Asked when the page is left, not when it opened: the person may have chosen View change set since.
  const kept = () => keep.current;
  useEffect(() => {
    left.current = false;
    return () => {
      left.current = true;
      // Only a preview that was made is removed (a page left before the review step made none, and asking would be refused).
      if (!kept() && made.current && stack) {
        made.current = false;
        void callKindOp(STACKS, "DeleteChangeSet", { StackName: stack, ChangeSetName: id }).catch(() => undefined);
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `kept` reads a ref: its identity changes with every render and says nothing
  }, [stack, id, keep]);
  return { data: q.data, pending: input !== null && q.isPending, error: q.error };
}

/** Whether `on` has held for less than `ms`: true while it does, and false from `ms` after it began (a page that has just written). */
export function useWhile(on: boolean, ms: number): boolean {
  const [live, setLive] = useState(true);
  useEffect(() => {
    if (!on) return;
    const t = setTimeout(() => setLive(false), ms);
    return () => clearTimeout(t);
  }, [on, ms]);
  return on && live;
}

/** The previews already asked to be removed: the page and its Change sets tab both look, and a set is deleted once. */
const ASKED = new Set<string>();

/** Removes from `stack` the preview change sets nobody is using (one a closed tab left behind), and says their names: the lists leave them out
 * meanwhile. `names`: the stack's change sets, once read. */
export function useStalePreviews(stack: string, names: readonly string[] | undefined): string[] {
  const qc = useQueryClient();
  const now = useNow();
  const stale = useMemo(() => stalePreviews(names ?? [], now, keptPreview()), [names, now]);
  useEffect(() => {
    for (const id of stale) {
      if (ASKED.has(`${stack}/${id}`)) continue;
      ASKED.add(`${stack}/${id}`);
      void callKindOp(STACKS, "DeleteChangeSet", { StackName: stack, ChangeSetName: id })
        .catch(() => undefined)
        .then(() => forgetDeleted(qc, scopeOf("cloudformation"), [id]));
    }
  }, [stale, stack, qc]);
  return stale;
}

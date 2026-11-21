// What the function's pages read besides its detail: its policy, its settings as Lambda answers them, the layers of the account,
// and where the pages go. A read that Lambda refuses with "not found" (no policy, no URL, no async configuration) is an answer.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useMemo } from "react";
import { ApiFailure } from "@doze/ui/lib/api";
import { useAbout } from "../../../api/queries";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { useResourceList } from "../../../api/resources";
import { on, useOpRead } from "../deep/api";
import { type Fn, fnOf, statementsOf, triggersOf } from "./model";

export const FUNCTIONS = { service: "lambda", kind: "functions" } as const;
export const LAYERS = { service: "lambda", kind: "layers" } as const;

/** `ResourceNotFoundException`: what Lambda says of a policy, a URL or a configuration the function has none of. */
export const notFound = (e: unknown): boolean => e instanceof ApiFailure && (e.code === "ResourceNotFoundException" || e.status === 404);

/** `{ code, message }` of an error that came from the service, for the call panel's "what came back". */
export function refusalOf(e: unknown): { code: string; message: string } | null {
  if (!e) return null;
  return e instanceof ApiFailure ? { code: e.code, message: e.message } : { code: "Error", message: (e as Error).message };
}

/** The function's resource-based policy: its statements, and whether it has one. */
export function usePolicy(name: string) {
  const q = useOpRead<{ Policy?: string }>(on.fn(name), "GetPolicy", {});
  const statements = useMemo(() => statementsOf(q.data?.Policy), [q.data]);
  return { ...q, statements, none: notFound(q.error) || (q.data !== undefined && statements.length === 0) };
}

/** The triggers of a function: its event source mappings (read with its detail) and the services its policy lets invoke it. */
export function useTriggers(d: ResourceDetail) {
  const policy = usePolicy(d.name);
  const mappings = useMemo(() => d.subs.find((s) => s.title === "Event source mappings")?.rows ?? [], [d.subs]);
  const triggers = useMemo(() => triggersOf(mappings, policy.statements), [mappings, policy.statements]);
  return { triggers, policy, mappings, loading: policy.isPending };
}

/** What the function is, as its detail gives it. */
export const useFn = (d: ResourceDetail): Fn => useMemo(() => fnOf(d), [d]);

export type LayerRow = { name: string; latest: string; description: string; runtimes: string[]; archs: string[]; created: string; arn: string };

/** The account's layers, read from the layers list. */
export function useLayers(): { rows: LayerRow[]; pending: boolean; error: unknown } {
  const list = useResourceList("lambda", "layers");
  const arr = (v: string | null | undefined): string[] => {
    try {
      return v ? (JSON.parse(v) as string[]) : [];
    } catch {
      return [];
    }
  };
  const rows = (list.data?.pages.flatMap((p) => p.rows) ?? []).map((r): LayerRow => ({ name: r.name, latest: r.cells[0] ?? "", description: r.cells[1] ?? "", runtimes: arr(r.cells[2]), archs: arr(r.cells[3]), created: r.cells[4] ?? "", arn: r.cells[5] ?? "" }));
  return { rows, pending: list.isPending, error: list.error };
}

/** Where the pages of one function go: its address with `tab`, `p` (the Configuration section), `view`, `done` and `sel`. */
export type Go = { tab?: string; p?: string; view?: string; done?: string; sel?: string };

export function useGo(name: string): (to: Go) => void {
  const navigate = useNavigate();
  return (to) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "lambda", kind: "functions", name }, search: to });
}

export type FunctionSearch = Go & { create?: string; gone?: string };
export const useFunctionSearch = (): FunctionSearch => useSearch({ strict: false }) as FunctionSearch;

/** The region and account the console is for. */
export function useWhere(): { region: string; account: string } {
  const about = useAbout().data;
  return { region: about?.region ?? "us-east-1", account: about?.account ?? "000000000000" };
}

export const crumbsOf = (d: ResourceDetail, ...rest: { label: string; to: string }[]) => [
  { label: "Resources", to: "/resources" },
  { label: d.group, to: "/resources" },
  { label: d.service_title, to: `/resources/${d.service}` },
  { label: d.kind_title, to: `/resources/${d.service}/${d.kind}` },
  { label: d.name, to: `/resources/${d.service}/${d.kind}/${encodeURIComponent(d.name)}` },
  ...rest,
];

/** The address of one function's page, with its search. */
export const fnHref = (name: string, search: Go = {}): string => {
  const q = new URLSearchParams(Object.entries(search).filter(([, v]) => v) as [string, string][]).toString();
  return `/resources/lambda/functions/${encodeURIComponent(name)}${q ? `?${q}` : ""}`;
};

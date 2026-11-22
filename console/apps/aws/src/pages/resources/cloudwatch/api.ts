// CloudWatch's pages read and write through the service's own calls, made as the console by the kinds' operations
// (crates/aws-console/src/resources/cloudwatch/): DescribeAlarms, DescribeAlarmHistory, ListMetrics, GetMetricData, the dashboards.
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { callKindOp, useKindOpRead } from "../../../kit";
import type { Alarm, HistoryItem } from "./model";

export const ALARMS = { service: "cloudwatch", kind: "alarms" } as const;
export const METRICS = { service: "cloudwatch", kind: "metrics" } as const;
export const DASHBOARDS = { service: "cloudwatch", kind: "dashboards" } as const;

export type MetricRef = { Namespace: string; MetricName: string; Dimensions?: { Name: string; Value: string }[] };
export type DashboardEntry = { DashboardName: string; DashboardArn?: string; LastModified?: number | string; Size?: number };
export type DashboardAnswer = { DashboardName: string; DashboardArn?: string; DashboardBody: string };

/** Reads every page of a call that pages by `NextToken`, up to `pages` of them. */
async function all<T>(k: { service: string; kind: string }, operation: string, input: object, items: string, pages = 10): Promise<T[]> {
  const out: T[] = [];
  let token: string | undefined;
  for (let i = 0; i < pages; i++) {
    const answer = await callKindOp<Record<string, unknown>>(k, operation, token ? { ...input, NextToken: token } : input);
    out.push(...((answer[items] as T[] | undefined) ?? []));
    token = answer.NextToken as string | undefined;
    if (!token) break;
  }
  return out;
}

/** Every alarm of the Region, metric and composite. */
export function useAlarms(enabled = true) {
  return useQuery({
    queryKey: ["resources", "kindop", "cloudwatch", "alarms", "DescribeAlarms", "all"],
    queryFn: async () => {
      const out: Alarm[] = [];
      let token: string | undefined;
      for (let i = 0; i < 10; i++) {
        const a = await callKindOp<{ MetricAlarms?: Alarm[]; CompositeAlarms?: Alarm[]; NextToken?: string }>(ALARMS, "DescribeAlarms", { AlarmTypes: ["MetricAlarm", "CompositeAlarm"], MaxRecords: 100, ...(token ? { NextToken: token } : {}) });
        out.push(...(a.MetricAlarms ?? []), ...(a.CompositeAlarms ?? []));
        token = a.NextToken;
        if (!token) break;
      }
      return out;
    },
    enabled,
    retry: false,
  });
}

/** One alarm, as DescribeAlarms gives it (`null`: no alarm of that name). */
export function useAlarm(name: string, enabled = true) {
  const q = useKindOpRead<{ MetricAlarms?: Alarm[]; CompositeAlarms?: Alarm[] }>(ALARMS, "DescribeAlarms", { AlarmNames: [name], AlarmTypes: ["MetricAlarm", "CompositeAlarm"] }, enabled);
  return { ...q, alarm: q.data ? ((q.data.MetricAlarms?.[0] ?? q.data.CompositeAlarms?.[0]) ?? null) : undefined };
}

export type HistoryFilter = { type: string; start: number | null; end: number | null };
/** An alarm's history, newest first. */
export function useHistory(name: string, f: HistoryFilter = { type: "", start: null, end: null }, enabled = true) {
  const input = { AlarmName: name, MaxRecords: 100, ...(f.type ? { HistoryItemType: f.type } : {}), ...(f.start ? { StartDate: Math.floor(f.start / 1000) } : {}), ...(f.end ? { EndDate: Math.ceil(f.end / 1000) } : {}) };
  const q = useKindOpRead<{ AlarmHistoryItems?: HistoryItem[]; NextToken?: string }>(ALARMS, "DescribeAlarmHistory", input, enabled);
  return { ...q, items: q.data?.AlarmHistoryItems ?? [], more: Boolean(q.data?.NextToken) };
}

/** Metrics that have data, as ListMetrics lists them (every page, up to 5,000). */
export function useMetrics(filter: { Namespace?: string } = {}, enabled = true) {
  return useQuery({
    queryKey: ["resources", "kindop", "cloudwatch", "metrics", "ListMetrics", "all", filter],
    queryFn: () => all<MetricRef>(METRICS, "ListMetrics", filter, "Metrics", 10),
    enabled,
    retry: false,
  });
}

export function useDashboards(enabled = true) {
  return useQuery({ queryKey: ["resources", "kindop", "cloudwatch", "dashboards", "ListDashboards", "all"], queryFn: () => all<DashboardEntry>(DASHBOARDS, "ListDashboards", {}, "DashboardEntries"), enabled, retry: false });
}

/** A dashboard's body, as GetDashboard gives it. */
export function useDashboard(name: string, enabled = true) {
  return useKindOpRead<DashboardAnswer>(DASHBOARDS, "GetDashboard", { DashboardName: name }, enabled);
}

/** What a write of alarms or dashboards leaves stale: their reads are marked, and the ones on screen made again, except the
 * `gone` ones (a page of a resource just deleted is not read again to be refused). */
export function useStale() {
  const qc = useQueryClient();
  return (kind: "alarms" | "dashboards", gone?: string) => {
    void qc.invalidateQueries({ queryKey: ["resources", "kindop", "cloudwatch", kind], predicate: (q) => !gone || !JSON.stringify(q.queryKey).includes(JSON.stringify(gone)) });
    void qc.invalidateQueries({ queryKey: ["resources", "list", "cloudwatch", kind] });
    void qc.invalidateQueries({ queryKey: ["resources", "index"] });
    if (gone) void qc.invalidateQueries({ queryKey: ["resources", "one", "cloudwatch", kind, gone], refetchType: "none" });
  };
}

/** PutDashboard (the whole body replaces what was there). */
export function usePutDashboard() {
  const stale = useStale();
  return useMutation({
    mutationFn: (v: { name: string; body: string }) => callKindOp<{ DashboardValidationMessages?: { DataPath?: string; Message?: string }[] }>(DASHBOARDS, "PutDashboard", { DashboardName: v.name, DashboardBody: v.body }),
    onSuccess: () => stale("dashboards"),
  });
}

export function useDeleteDashboards() {
  const stale = useStale();
  return useMutation({
    mutationFn: (names: string[]) => callKindOp(DASHBOARDS, "DeleteDashboards", { DashboardNames: names }),
    onSuccess: (_a, names) => stale("dashboards", names.length === 1 ? names[0] : undefined),
  });
}

export function useDeleteAlarms() {
  const stale = useStale();
  return useMutation({
    mutationFn: (names: string[]) => callKindOp(ALARMS, "DeleteAlarms", { AlarmNames: names }),
    onSuccess: (_a, names) => stale("alarms", names.length === 1 ? names[0] : undefined),
  });
}

export function usePutAlarm() {
  const stale = useStale();
  return useMutation({
    mutationFn: (input: object) => callKindOp(ALARMS, "PutMetricAlarm", input),
    onSuccess: () => stale("alarms"),
  });
}

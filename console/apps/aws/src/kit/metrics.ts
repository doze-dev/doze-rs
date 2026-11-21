// Metric data for a Chart: CloudWatch's own GetMetricData, asked through the console (contract §7), read into the Chart's series.
import type { Point } from "@doze/ui/components/app/chart-scale";
import type { Series } from "@doze/ui/components/app/chart";
import { useKindOpRead } from "./ops";

export type MetricQuery = { id: string; label: string; namespace: string; metric: string; dimensions?: Record<string, string>; stat?: string; period?: number };
export type Range = { from: number; to: number };

/** The ranges the board offers, in ms back from now. */
export const RANGES: { id: string; label: string; ms: number }[] = [
  { id: "1h", label: "1h", ms: 3_600_000 },
  { id: "3h", label: "3h", ms: 3 * 3_600_000 },
  { id: "12h", label: "12h", ms: 12 * 3_600_000 },
  { id: "1d", label: "1d", ms: 86_400_000 },
  { id: "3d", label: "3d", ms: 3 * 86_400_000 },
  { id: "1w", label: "1w", ms: 7 * 86_400_000 },
];

/** A period that gives about 60 points over the span, as CloudWatch allows (a multiple of 60 s). */
export function periodFor(spanMs: number): number {
  const s = Math.max(60, Math.round(spanMs / 1000 / 60));
  return Math.ceil(s / 60) * 60;
}

/** The service's input for `queries` over `range`. */
export function metricInput(queries: MetricQuery[], range: Range) {
  return {
    StartTime: Math.floor(range.from / 1000),
    EndTime: Math.floor(range.to / 1000),
    ScanBy: "TimestampAscending",
    MetricDataQueries: queries.map((q) => ({
      Id: q.id,
      Label: q.label,
      MetricStat: { Metric: { Namespace: q.namespace, MetricName: q.metric, Dimensions: Object.entries(q.dimensions ?? {}).map(([Name, Value]) => ({ Name, Value })) }, Period: q.period ?? periodFor(range.to - range.from), Stat: q.stat ?? "Sum" },
      ReturnData: true,
    })),
  };
}

type Answer = { MetricDataResults?: { Id?: string; Label?: string; Timestamps?: (number | string)[]; Values?: number[] }[] };

/** The service's answer as Chart series, one per query, in the queries' order; timestamps (seconds, or ISO text) become ms. */
export function toSeries(answer: Answer | undefined, queries: MetricQuery[]): Series[] {
  return queries.map((q) => {
    const r = answer?.MetricDataResults?.find((x) => x.Id === q.id);
    const points: Point[] = (r?.Timestamps ?? []).map((t, i) => ({ t: typeof t === "number" ? (t < 1e11 ? t * 1000 : t) : Date.parse(t), v: r?.Values?.[i] ?? null }));
    points.sort((a, b) => a.t - b.t);
    return { id: q.id, label: r?.Label ?? q.label, points };
  });
}

/** The series of `queries` over `range`: one GetMetricData, made when the queries or the range change. */
export function useMetricData(queries: MetricQuery[], range: Range, enabled = true) {
  const q = useKindOpRead<Answer>({ service: "cloudwatch", kind: "metrics" }, "GetMetricData", metricInput(queries, range), enabled && queries.length > 0);
  return { ...q, series: toSeries(q.data, queries) };
}

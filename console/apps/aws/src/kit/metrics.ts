// Metric data for a Chart: CloudWatch's own GetMetricData, asked through the console (contract §7), read into the Chart's series.
import type { Point } from "@doze/ui/components/app/chart-scale";
import type { Series } from "@doze/ui/components/app/chart";
import { useQueries } from "@tanstack/react-query";
import { callKindOp, useKindOpRead } from "./ops";

/** One query: a metric (`namespace`, `metric`), or metric math (`expression` over the ids of the other queries; `hide`: used by another query and not drawn). */
export type MetricQuery = { id: string; label: string; namespace: string; metric: string; dimensions?: Record<string, string>; stat?: string; period?: number; expression?: string; hide?: boolean };
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

/** The EndTime, in seconds, for a range that ends at `ms`. EndTime is exclusive and a minute's data point is stamped with the start of its
 * minute, so an end rounded down to the second that is also the start of a minute (`now` at 20:27:00.4, an invocation at 20:27:00.1)
 * would leave out the minute that is running: the end is rounded up. */
export function endSecond(ms: number): number {
  return Math.ceil(ms / 1000);
}

/** The service's input for `queries` over `range`. */
export function metricInput(queries: MetricQuery[], range: Range) {
  return {
    StartTime: Math.floor(range.from / 1000),
    EndTime: endSecond(range.to),
    ScanBy: "TimestampAscending",
    MetricDataQueries: queries.map((q) =>
      q.expression !== undefined
        ? { Id: q.id, Label: q.label, Expression: q.expression, ReturnData: !q.hide }
        : {
            Id: q.id,
            Label: q.label,
            MetricStat: { Metric: { Namespace: q.namespace, MetricName: q.metric, Dimensions: Object.entries(q.dimensions ?? {}).map(([Name, Value]) => ({ Name, Value })) }, Period: q.period ?? periodFor(range.to - range.from), Stat: q.stat ?? "Sum" },
            ReturnData: !q.hide,
          },
    ),
  };
}

type Answer = { MetricDataResults?: { Id?: string; Label?: string; Timestamps?: (number | string)[]; Values?: number[] }[] };

/** The service's answer as Chart series, one per query, in the queries' order; timestamps (seconds, or ISO text) become ms. */
export function toSeries(answer: Answer | undefined, queries: MetricQuery[]): Series[] {
  return queries.filter((q) => !q.hide).map((q) => {
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

const METRICS = { service: "cloudwatch", kind: "metrics" } as const;

/** The GetMetricStatistics input that tells a metric's unit: GetMetricData answers values only, no unit, so the unit is read from the
 * data points of the metric itself, which carry the one it was put with. */
export function unitInput(q: MetricQuery, range: Range) {
  return { Namespace: q.namespace, MetricName: q.metric, Dimensions: Object.entries(q.dimensions ?? {}).map(([Name, Value]) => ({ Name, Value })), StartTime: Math.floor(range.from / 1000), EndTime: endSecond(range.to), Period: periodFor(range.to - range.from), Statistics: ["SampleCount"] };
}

/** The one unit every answered metric is in (`Count`, `Milliseconds`), or undefined when they differ, when there is none ("None" is no unit)
 * or when a metric has not answered yet. */
export function sharedUnit(units: readonly (string | undefined)[]): string | undefined {
  const named = new Set(units.map((u) => (u === undefined || u === "None" ? "" : u)));
  const [only] = [...named];
  return named.size === 1 && only ? only : undefined;
}

/** The unit of the metrics of `queries` (math has none): what the axis, the readout and a number say after a value. At most four metrics are asked. */
export function useUnit(queries: MetricQuery[], range: Range, enabled = true): string | undefined {
  const own = queries.filter((q) => q.expression === undefined && !q.hide).slice(0, 4);
  const answers = useQueries({
    queries: own.map((q) => {
      const input = unitInput(q, range);
      return { queryKey: ["resources", "kindop", "cloudwatch", "metrics", "GetMetricStatistics", input], queryFn: ({ signal }: { signal: AbortSignal }) => callKindOp<{ Datapoints?: { Unit?: string }[] }>(METRICS, "GetMetricStatistics", input, signal), enabled, retry: false, staleTime: 60_000 };
    }),
  });
  if (!enabled || own.length === 0 || queries.some((q) => q.expression !== undefined && !q.hide)) return undefined;
  return sharedUnit(answers.map((a) => (a.data ? (a.data.Datapoints?.[0]?.Unit ?? "None") : undefined)));
}

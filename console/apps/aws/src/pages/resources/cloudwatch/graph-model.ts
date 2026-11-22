// The metrics a graph shows (the Graphed metrics table of cloudwatch__metrics-graph and of the widget pages): kept in the address,
// read into queries for GetMetricData, and written as a dashboard widget. Pure.
import type { MetricQuery } from "../../../kit";
import { type Dimension, periodWords } from "./model";

/** One row of Graphed metrics: a metric (`ns`, `m`, `d`) or metric math (`expr`, over the ids of the other rows). */
export type GM = { id: string; ns?: string; m?: string; d?: Dimension[]; stat: string; period: number; label: string; shown: boolean; expr?: string };

export const encode = (rows: readonly GM[]): string => JSON.stringify(rows);

/** The rows an address names; none for text that is not a list of them. */
export function decode(text: string | undefined): GM[] {
  if (!text) return [];
  try {
    const raw = JSON.parse(text) as unknown;
    if (!Array.isArray(raw)) return [];
    return raw.flatMap((r: Partial<GM>) => (typeof r?.id === "string" && typeof r.label === "string" && typeof r.stat === "string" && typeof r.period === "number" ? [{ id: r.id, ns: r.ns, m: r.m, d: Array.isArray(r.d) ? r.d : undefined, stat: r.stat, period: r.period, label: r.label, shown: r.shown !== false, expr: r.expr }] : []));
  } catch {
    return [];
  }
}

/** The next id for a row: `m1`, `m2`, or `e1` for math. */
export function nextId(rows: readonly GM[], math = false): string {
  const p = math ? "e" : "m";
  let n = 1;
  while (rows.some((r) => r.id === `${p}${n}`)) n++;
  return `${p}${n}`;
}

export const dimsOf = (r: Pick<GM, "d">): Record<string, string> => Object.fromEntries((r.d ?? []).map((x) => [x.Name, x.Value]));

/** The queries that draw the rows: a row that is not shown but feeds a shown expression is asked for and not drawn. */
export function toQueries(rows: readonly GM[]): MetricQuery[] {
  const used = new Set(rows.filter((r) => r.shown && r.expr !== undefined).flatMap((r) => (r.expr?.match(/\b[a-z]+\d+\b/g) ?? [])));
  return rows
    .filter((r) => r.shown || (r.expr === undefined && used.has(r.id)))
    .map((r): MetricQuery => (r.expr !== undefined ? { id: r.id, label: r.label || r.id, namespace: "", metric: "", expression: r.expr, hide: !r.shown } : { id: r.id, label: r.label || r.m || r.id, namespace: r.ns ?? "", metric: r.m ?? "", dimensions: dimsOf(r), stat: r.stat, period: r.period, hide: !r.shown }));
}

/** `2 metrics from Larder/Orders, 5 minute periods, last 3 hours.` */
export function summaryLine(rows: readonly GM[], span: string): string {
  const metrics = rows.filter((r) => r.expr === undefined);
  if (rows.length === 0) return "";
  const spaces = [...new Set(metrics.map((r) => r.ns))];
  const periods = [...new Set(metrics.map((r) => r.period))];
  const n = rows.length;
  const from = spaces.length === 1 ? ` from ${spaces[0]}` : spaces.length > 1 ? ` from ${spaces.length} namespaces` : "";
  const per = periods.length === 1 && periods[0] !== undefined ? `, ${periodWords(periods[0]).replace(/s$/, "")} periods` : periods.length > 1 ? ", mixed periods" : "";
  return `${n} ${n === 1 ? "metric" : "metrics"}${from}${per}, ${span}.`;
}

export type WidgetView = "timeSeries" | "singleValue" | "gauge" | "bar" | "pie" | "table";

/** A metric widget for a dashboard body (`properties.metrics` rows are `[namespace, name, dim, value, ..., {options}]`). */
export function metricWidget(rows: readonly GM[], o: { view: WidgetView; stacked?: boolean; region: string; title?: string; period?: number }): Record<string, unknown> {
  const metrics = rows.map((r) => {
    if (r.expr !== undefined) return [{ expression: r.expr, label: r.label || r.id, id: r.id, ...(r.shown ? {} : { visible: false }) }];
    const dims = (r.d ?? []).flatMap((x) => [x.Name, x.Value]);
    return [r.ns, r.m, ...dims, { stat: r.stat, period: r.period, id: r.id, ...(r.label && r.label !== r.m ? { label: r.label } : {}), ...(r.shown ? {} : { visible: false }) }];
  });
  return { type: "metric", width: o.view === "timeSeries" ? 12 : 6, height: 6, properties: { ...(o.title ? { title: o.title } : {}), view: o.view, ...(o.stacked ? { stacked: true } : {}), region: o.region, period: o.period ?? rows.find((r) => r.expr === undefined)?.period ?? 300, metrics } };
}

/** The Source tab's text for the rows: the widget they make. */
export const sourceOf = (rows: readonly GM[], region: string, view: WidgetView = "timeSeries"): string => JSON.stringify(metricWidget(rows, { view, region }).properties, null, 2);

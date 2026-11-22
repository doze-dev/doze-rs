// The metrics page's browse view: the metrics that have data, grouped by namespace and then by the set of dimension names they
// were published with (cloudwatch__metrics-all). Pure.
import type { MetricRef } from "./api";

export type NsRow = { namespace: string; names: string[]; key: string; count: number };

const dimNames = (m: MetricRef): string[] => (m.Dimensions ?? []).map((d) => d.Name).sort();
export const keyOf = (names: readonly string[]): string => names.join(",");

const matches = (m: MetricRef, term: string): boolean => {
  const t = term.trim().toLowerCase();
  return t === "" || [m.Namespace, m.MetricName, ...(m.Dimensions ?? []).flatMap((d) => [d.Name, d.Value])].some((x) => x.toLowerCase().includes(t));
};

/** The namespaces with metrics matching `term`: your own, and the AWS ones; one row for each set of dimension names, counting the
 * distinct metric names in it. */
export function groupMetrics(all: readonly MetricRef[], term: string): { custom: NsRow[]; aws: NsRow[] } {
  const rows = new Map<string, { namespace: string; names: string[]; metrics: Set<string> }>();
  for (const m of all.filter((x) => matches(x, term))) {
    const names = dimNames(m);
    const k = `${m.Namespace}\u0000${keyOf(names)}`;
    const row = rows.get(k) ?? { namespace: m.Namespace, names, metrics: new Set<string>() };
    row.metrics.add(m.MetricName);
    rows.set(k, row);
  }
  const out = [...rows.values()].map((r): NsRow => ({ namespace: r.namespace, names: r.names, key: keyOf(r.names), count: r.metrics.size })).sort((a, b) => a.namespace.localeCompare(b.namespace) || a.key.localeCompare(b.key));
  return { custom: out.filter((r) => !r.namespace.startsWith("AWS/")), aws: out.filter((r) => r.namespace.startsWith("AWS/")) };
}

/** The metrics of one namespace and dimension-name set (every combination of values), matching `term`. */
export function metricsIn(all: readonly MetricRef[], namespace: string, key: string, term: string): MetricRef[] {
  return all.filter((m) => m.Namespace === namespace && keyOf(dimNames(m)) === key && matches(m, term)).sort((a, b) => a.MetricName.localeCompare(b.MetricName) || JSON.stringify(a.Dimensions).localeCompare(JSON.stringify(b.Dimensions)));
}

export const refId = (m: MetricRef): string => `${m.Namespace}\u0000${m.MetricName}\u0000${JSON.stringify(m.Dimensions ?? [])}`;

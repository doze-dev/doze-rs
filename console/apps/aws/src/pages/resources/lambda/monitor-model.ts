// The figures of the Monitor tab: what a metric's points add up to, and how they are drawn as a line. Pure.
import type { Point } from "@doze/ui/components/app/chart-scale";

export const HOUR = 3_600_000;

/** The sum of the points, `null` when there are none. */
export function total(points: readonly Point[]): number | null {
  const have = points.filter((p) => p.v !== null);
  return have.length === 0 ? null : have.reduce((n, p) => n + (p.v ?? 0), 0);
}

/** The mean of the points, `null` when there are none. */
export function mean(points: readonly Point[]): number | null {
  const have = points.filter((p) => p.v !== null);
  return have.length === 0 ? null : have.reduce((n, p) => n + (p.v ?? 0), 0) / have.length;
}

/** The sum of the points of the last hour before `now`. */
export const lastHour = (points: readonly Point[], now: number): number | null => total(points.filter((p) => p.t > now - HOUR));

/** `41 ms`, `1.2 s` for a duration given in milliseconds. */
export function milliseconds(v: number): string {
  return v >= 1000 ? `${(v / 1000).toFixed(v >= 10_000 ? 0 : 1)} s` : `${Math.round(v)} ms`;
}

/** The points as an SVG path in a `width` by `height` box (a flat line along the bottom for no data); the values are spread over the box's height. */
export function linePath(points: readonly Point[], from: number, to: number, width: number, height: number): string {
  const have = points.filter((p) => p.v !== null).sort((a, b) => a.t - b.t);
  const pad = 3;
  if (have.length === 0) return `M0 ${height - pad} L${width} ${height - pad}`;
  const top = Math.max(...have.map((p) => p.v ?? 0), 1e-9);
  const x = (t: number) => ((t - from) / Math.max(1, to - from)) * width;
  const y = (v: number) => height - pad - (v / top) * (height - 2 * pad);
  return have.map((p, i) => `${i === 0 ? "M" : "L"}${x(p.t).toFixed(1)} ${y(p.v ?? 0).toFixed(1)}`).join(" ");
}

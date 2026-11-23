// The arithmetic of the time-series Chart: scales, ticks on UTC boundaries, the point nearest the pointer, the line's path.
// Pure, so every one of them is tested without a browser.

export type Point = { /** ms since 1970. */ t: number; /** `null`: no data at that time (the line breaks). */ v: number | null };

/** A linear map from a domain to a range. A domain of one value maps to the range's middle. */
export function linear(d0: number, d1: number, r0: number, r1: number): (x: number) => number {
  if (d1 === d0) return () => (r0 + r1) / 2;
  const k = (r1 - r0) / (d1 - d0);
  return (x) => r0 + (x - d0) * k;
}

/** Round tick values over [min, max]: steps of 1, 2, 2.5 or 5 times a power of ten, about `count` of them, the first at or
 * below `min` and the last at or above `max`. An empty range (min = max) gets a range of its own so a flat line sits on a grid. */
export function niceTicks(min: number, max: number, count = 5): number[] {
  if (!Number.isFinite(min) || !Number.isFinite(max)) return [0, 1];
  let lo = min;
  let hi = max;
  if (lo === hi) {
    if (lo === 0) hi = 1;
    else {
      lo = Math.min(lo, 0);
      hi = Math.max(hi, 0) || 1;
    }
  }
  const raw = (hi - lo) / Math.max(1, count - 1);
  const pow = 10 ** Math.floor(Math.log10(raw));
  const step = ([1, 2, 2.5, 5, 10].map((m) => m * pow).find((s) => s >= raw) ?? 10 * pow) as number;
  const first = Math.floor(lo / step + 1e-9) * step;
  const out: number[] = [];
  for (let v = first; v < hi + step - 1e-9; v += step) out.push(Number(v.toPrecision(12)));
  return out;
}

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
/** The steps a time axis uses, smallest first. */
export const STEPS = [MIN, 5 * MIN, 10 * MIN, 15 * MIN, 30 * MIN, HOUR, 2 * HOUR, 3 * HOUR, 6 * HOUR, 12 * HOUR, DAY, 2 * DAY, 7 * DAY];

/** The tick step for a span that should have about `max` ticks: the smallest of {@link STEPS} that gives no more. */
export function timeStep(span: number, max = 6): number {
  return STEPS.find((s) => span / s <= max) ?? STEPS[STEPS.length - 1]!;
}

/** Ticks on UTC multiples of the step (a 3-hour step ticks at 00:00, 03:00, ...), those within [from, to]. */
export function timeTicks(from: number, to: number, max = 6): { step: number; ticks: number[] } {
  const step = timeStep(to - from, max);
  const out: number[] = [];
  // A week is counted from the epoch's own Thursday: a multiple of the step is all a UTC tick needs to be.
  for (let t = Math.ceil(from / step) * step; t <= to; t += step) out.push(t);
  return { step, ticks: out };
}

const pad = (n: number) => String(n).padStart(2, "0");
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** A tick's label in UTC: `07:00` for steps under a day (`Nov 8` on the tick at midnight, so a day is named where it starts),
 * `Nov 8` for a day or more. */
export function tickLabel(t: number, step: number): string {
  const d = new Date(t);
  const day = `${MONTHS[d.getUTCMonth()]} ${d.getUTCDate()}`;
  if (step >= DAY) return day;
  if (d.getUTCHours() === 0 && d.getUTCMinutes() === 0) return day;
  return `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`;
}

/** Index of the point whose time is nearest `t` in `points` (sorted by time), `-1` when there are none. */
export function nearest(points: readonly Point[], t: number): number {
  if (points.length === 0) return -1;
  let lo = 0;
  let hi = points.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (points[mid]!.t < t) lo = mid + 1;
    else hi = mid;
  }
  if (lo > 0 && Math.abs(points[lo - 1]!.t - t) <= Math.abs(points[lo]!.t - t)) return lo - 1;
  return lo;
}

/** The line through the points as an SVG path; a `null` value ends the run, and the next value starts a new one (a gap is a gap,
 * never a line to zero). A run of one point has no line: {@link isolatedPoints} finds it, to be drawn as a dot. */
export function linePath(points: readonly Point[], x: (t: number) => number, y: (v: number) => number): string {
  let d = "";
  let open = false;
  for (const p of points) {
    if (p.v === null) {
      open = false;
      continue;
    }
    d += `${open ? "L" : "M"}${x(p.t).toFixed(1)} ${y(p.v).toFixed(1)}`;
    open = true;
  }
  return d;
}

/** The area under each run of the line, down to `base`: one closed path per run. */
export function areaPath(points: readonly Point[], x: (t: number) => number, y: (v: number) => number, base: number): string {
  let d = "";
  let run: Point[] = [];
  const flush = () => {
    if (run.length > 1) d += `M${x(run[0]!.t).toFixed(1)} ${base}${run.map((p) => `L${x(p.t).toFixed(1)} ${y(p.v!).toFixed(1)}`).join("")}L${x(run[run.length - 1]!.t).toFixed(1)} ${base}Z`;
    run = [];
  };
  for (const p of points) {
    if (p.v === null) flush();
    else run.push(p);
  }
  flush();
  return d;
}

/** The value range of the series (and a threshold line): from 0 when nothing is negative (a count's axis starts at zero, so a
 * flat 3 is not drawn as a mountain), padded at the top. `null` when there is no value at all. */
export function valueExtent(series: readonly { points: readonly Point[] }[], extra: readonly number[] = []): { min: number; max: number } | null {
  let min = Number.POSITIVE_INFINITY;
  let max = Number.NEGATIVE_INFINITY;
  for (const s of series) for (const p of s.points) if (p.v !== null) {
    min = Math.min(min, p.v);
    max = Math.max(max, p.v);
  }
  if (min === Number.POSITIVE_INFINITY) return null;
  for (const e of extra) {
    min = Math.min(min, e);
    max = Math.max(max, e);
  }
  return { min: min >= 0 ? 0 : min, max };
}

/** A value for an axis or a readout: `1,204`, `12.5`, `3.2k` past ten thousand, `1.5M`; at most 3 significant decimals. */
export function axisNumber(v: number): string {
  const a = Math.abs(v);
  if (a >= 1e9) return `${Number((v / 1e9).toPrecision(3))}G`;
  if (a >= 1e6) return `${Number((v / 1e6).toPrecision(3))}M`;
  if (a >= 1e4) return `${Number((v / 1e3).toPrecision(3))}k`;
  if (Number.isInteger(v)) return v.toLocaleString("en-US");
  return Number(v.toPrecision(3)).toString();
}

/** The points that have no neighbour to join: a value between two gaps (or alone), which a line cannot show, so it is drawn as a dot. */
export function isolatedPoints(points: readonly Point[]): Point[] {
  return points.filter((p, i) => p.v !== null && points[i - 1]?.v == null && points[i + 1]?.v == null);
}

/** The value range with room for a threshold's label: when the bad side is above, the range goes a little past the larger of the data and the
 * line (a twelfth of the range), so the line is never on the top edge and its label, drawn above it, sits clear of the data. */
export function extentWithRoom(extent: { min: number; max: number } | null, threshold?: { value: number; bad: "above" | "below" }): { min: number; max: number } | null {
  if (!extent || !threshold || threshold.bad !== "above") return extent;
  const top = Math.max(extent.max, threshold.value);
  const span = top - extent.min || Math.abs(top) || 1;
  return { min: extent.min, max: top + span / 12 };
}

// The shapes of the dashboard's number and pie widgets, as SVG path text. Pure, so each is tested without a browser.
import { type Point, linear } from "@doze/ui/components/app/chart-scale";

/** The line of a number widget's sparkline through `points`, inside a `w` by `h` box with `pad` around it; a gap ends the run. Empty when
 * fewer than two points have a value (one point is no line). */
export function sparkPath(points: readonly Point[], w: number, h: number, pad = 2): string {
  const known = points.filter((p) => p.v !== null);
  if (known.length < 2) return "";
  const vs = known.map((p) => p.v as number);
  const lo = Math.min(...vs);
  const hi = Math.max(...vs);
  const x = linear(points[0]!.t, points[points.length - 1]!.t, pad, w - pad);
  // A flat line sits in the middle of the box.
  const y = hi === lo ? () => h / 2 : linear(lo, hi, h - pad, pad);
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

/** The share of each value in the whole, as a start and an end in turns (0 to 1); a negative value counts as none. Empty when nothing is above zero. */
export function slices(values: readonly (number | null)[]): { from: number; to: number }[] {
  const parts = values.map((v) => Math.max(0, v ?? 0));
  const sum = parts.reduce((a, b) => a + b, 0);
  if (sum === 0) return [];
  let at = 0;
  return parts.map((p) => {
    const from = at;
    at += p / sum;
    return { from, to: at };
  });
}

/** The path of the slice of a pie between two turns (0 is the top, turns go clockwise); a whole turn is two half arcs, as one arc to its own start draws nothing. */
export function slicePath(cx: number, cy: number, r: number, from: number, to: number): string {
  const at = (turn: number) => [cx + r * Math.sin(turn * 2 * Math.PI), cy - r * Math.cos(turn * 2 * Math.PI)] as const;
  const [x0, y0] = at(from);
  if (to - from >= 0.9999) {
    const [x1, y1] = at(from + 0.5);
    return `M${cx} ${cy}L${x0.toFixed(2)} ${y0.toFixed(2)}A${r} ${r} 0 1 1 ${x1.toFixed(2)} ${y1.toFixed(2)}A${r} ${r} 0 1 1 ${x0.toFixed(2)} ${y0.toFixed(2)}Z`;
  }
  const [x1, y1] = at(to);
  return `M${cx} ${cy}L${x0.toFixed(2)} ${y0.toFixed(2)}A${r} ${r} 0 ${to - from > 0.5 ? 1 : 0} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}Z`;
}

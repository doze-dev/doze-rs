import { useEffect, useMemo, useRef, useState } from "react";
import { dateTimeUtc } from "../../lib/format";
import { cn } from "../../lib/utils";
import { areaPath, axisNumber, extentWithRoom, isolatedPoints, linear, linePath, nearest, niceTicks, type Point, tickLabel, timeTicks, valueExtent } from "./chart-scale";

export type SeriesTone = "cool" | "warn" | "good" | "refused" | "bad";
export type Series = { id: string; label: string; points: Point[]; /** Default: the next colour in order. */ tone?: SeriesTone; /** The area under the line is tinted. */ area?: boolean };
export type Threshold = { value: number; /** `Alarm at 5`. */ label: string; /** Which side is the bad one: the band is drawn there. */ bad: "above" | "below" };

// Static class names, so the stylesheet sees them. A series is a role (never a colour): the theme sets each, light and dark.
const TONES: Record<SeriesTone, { line: string; dot: string; swatch: string; area: string }> = {
  cool: { line: "stroke-cool", dot: "fill-cool", swatch: "bg-cool", area: "fill-cool/10" },
  warn: { line: "stroke-warn", dot: "fill-warn", swatch: "bg-warn", area: "fill-warn/10" },
  good: { line: "stroke-good", dot: "fill-good", swatch: "bg-good", area: "fill-good/10" },
  refused: { line: "stroke-refused", dot: "fill-refused", swatch: "bg-refused", area: "fill-refused/10" },
  bad: { line: "stroke-bad", dot: "fill-bad", swatch: "bg-bad", area: "fill-bad/10" },
};
const ORDER: SeriesTone[] = ["cool", "warn", "good", "refused", "bad"];
const MARGIN = { top: 12, right: 14, bottom: 28, left: 46 };

/** A time series, drawn by hand in SVG (no chart library): lines (with gaps where there is no data), an area, a threshold band,
 * a y axis of round numbers, an x axis on UTC ticks, a legend, and a readout of the point nearest the pointer (or the arrow
 * keys: the chart takes the focus). It fades in; with `prefers-reduced-motion` it appears at once. Every size is the
 * container's width and `height`, so it reflows with the window. */
export function Chart({ series, from, to, height = 260, threshold, unit, empty = "There are no data points in this range.", loading = false, "aria-label": ariaLabel, className, testId = "chart" }: { series: Series[]; /** The range drawn, ms; by default the first and last point. */ from?: number; to?: number; height?: number; threshold?: Threshold; /** After each value in the readout (`messages`, `ms`). */ unit?: string; empty?: React.ReactNode; loading?: boolean; "aria-label": string; className?: string; testId?: string }) {
  // With a unit, a row above the axis names it (`Count`, `Seconds`).
  const M = { ...MARGIN, top: unit ? 26 : MARGIN.top };
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [hover, setHover] = useState<number | null>(null);
  useEffect(() => {
    const el = box.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const seen = new ResizeObserver(() => setWidth(el.clientWidth));
    seen.observe(el);
    setWidth(el.clientWidth);
    return () => seen.disconnect();
  }, []);

  const times = useMemo(() => [...new Set(series.flatMap((s) => s.points.map((p) => p.t)))].sort((a, b) => a - b), [series]);
  const extent = extentWithRoom(valueExtent(series, threshold ? [threshold.value] : []), threshold);
  const has = extent !== null && times.length > 0;
  const t0 = from ?? times[0] ?? 0;
  const t1 = to ?? times[times.length - 1] ?? t0 + 1;
  const ticksY = has ? niceTicks(extent.min, extent.max, Math.max(3, Math.round(height / 60))) : [0, 1];
  const top = ticksY[ticksY.length - 1]!;
  const bottom = ticksY[0]!;
  const w = Math.max(width, 120);
  const x = linear(t0, t1, M.left, w - M.right);
  const y = linear(bottom, top, height - M.bottom, M.top);
  const { step, ticks } = timeTicks(t0, t1, Math.max(2, Math.floor((w - M.left - M.right) / 110)));
  const tones = series.map((s, i) => TONES[s.tone ?? ORDER[i % ORDER.length]!]);
  const at = hover !== null ? times[hover] : undefined;
  const readout = at === undefined ? [] : series.map((s, i) => ({ s, tone: tones[i]!, p: s.points.find((p) => p.t === at) })).filter((r) => r.p);

  const move = (clientX: number) => {
    const el = box.current;
    if (!el || times.length === 0) return;
    const px = clientX - el.getBoundingClientRect().left;
    const t = t0 + ((px - M.left) / (w - M.left - M.right)) * (t1 - t0);
    setHover(nearest(times.map((tt) => ({ t: tt, v: 0 })), t));
  };
  const key = (e: React.KeyboardEvent) => {
    if (times.length === 0) return;
    const cur = hover ?? (e.key === "ArrowLeft" ? times.length : -1);
    if (e.key === "ArrowRight") setHover(Math.min(times.length - 1, cur + 1));
    else if (e.key === "ArrowLeft") setHover(Math.max(0, cur - 1));
    else if (e.key === "Home") setHover(0);
    else if (e.key === "End") setHover(times.length - 1);
    else if (e.key === "Escape") setHover(null);
    else return;
    e.preventDefault();
  };

  return (
    <div className={cn("w-full min-w-0", className)} data-slot="chart" data-testid={testId}>
      <div ref={box} className="relative outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 rounded-lg" style={{ height }} tabIndex={has ? 0 : -1} onKeyDown={key} onBlur={() => setHover(null)} aria-label={`${ariaLabel}. Use the arrow keys to read each point.`} role="group">
        {loading ? (
          <div className="size-full animate-pulse rounded-lg bg-muted motion-reduce:animate-none" data-testid={`${testId}-loading`} />
        ) : !has ? (
          <div className="flex size-full items-center justify-center rounded-lg border border-dashed text-[13.5px] text-muted-foreground" data-testid={`${testId}-empty`}>
            {empty}
          </div>
        ) : (
          <>
            <svg width={w} height={height} role="img" aria-label={ariaLabel} className="animate-in fade-in duration-500 motion-reduce:animate-none" onPointerMove={(e) => move(e.clientX)} onPointerLeave={() => setHover(null)}>
              {unit && (
                <text x={4} y={11} className="fill-muted-foreground text-[11.5px]" data-testid={`${testId}-unit`}>
                  {unit}
                </text>
              )}
              {ticksY.map((v) => (
                <g key={v}>
                  <line x1={M.left} x2={w - M.right} y1={y(v)} y2={y(v)} className="stroke-border" strokeWidth={1} />
                  <text x={M.left - 8} y={y(v)} dy="0.32em" textAnchor="end" className="fill-muted-foreground text-[11.5px] tabular-nums">
                    {axisNumber(v)}
                  </text>
                </g>
              ))}
              {ticks.map((t) => (
                <text key={t} x={x(t)} y={height - 8} textAnchor="middle" className="fill-muted-foreground text-[11.5px] tabular-nums">
                  {tickLabel(t, step)}
                </text>
              ))}
              {threshold && (
                <g data-testid={`${testId}-threshold`}>
                  <rect x={M.left} width={w - M.left - M.right} y={threshold.bad === "above" ? M.top : y(threshold.value)} height={Math.max(0, threshold.bad === "above" ? y(threshold.value) - M.top : height - M.bottom - y(threshold.value))} className="fill-bad/10" />
                  <line x1={M.left} x2={w - M.right} y1={y(threshold.value)} y2={y(threshold.value)} className="stroke-bad" strokeWidth={1.25} strokeDasharray="5 4" />
                  <text x={w - M.right - 4} y={y(threshold.value) + (threshold.bad === "above" ? -6 : 14)} textAnchor="end" className="fill-bad text-[11.5px] font-medium">
                    {threshold.label}
                  </text>
                </g>
              )}
              {series.map((s, i) => (
                <g key={s.id} data-series={s.id}>
                  {(s.area ?? i === 0) && series.length <= 2 && <path d={areaPath(s.points, (t) => x(t), y, y(bottom))} className={tones[i]!.area} />}
                  <path d={linePath(s.points, (t) => x(t), y)} fill="none" className={tones[i]!.line} strokeWidth={1.75} strokeLinejoin="round" strokeLinecap="round" />
                  {isolatedPoints(s.points).map((p) => (
                    <circle key={p.t} cx={x(p.t)} cy={y(p.v!)} r={3} className={tones[i]!.dot} />
                  ))}
                </g>
              ))}
              {at !== undefined && (
                <g pointerEvents="none">
                  <line x1={x(at)} x2={x(at)} y1={M.top} y2={height - M.bottom} className="stroke-muted-foreground" strokeWidth={1} strokeDasharray="3 3" />
                  {readout.map((r) => r.p!.v !== null && <circle key={r.s.id} cx={x(at)} cy={y(r.p!.v)} r={4} className={cn(r.tone.dot, "stroke-card")} strokeWidth={2} />)}
                </g>
              )}
            </svg>
            {at !== undefined && (
              <div className="pointer-events-none absolute top-2 z-10 min-w-40 rounded-lg border bg-popover px-3 py-2 text-[12.5px] text-popover-foreground shadow-md" style={x(at) > w / 2 ? { right: w - x(at) + 12 } : { left: x(at) + 12 }} data-testid={`${testId}-readout`} role="status">
                <div className="mb-1 font-mono text-[11.5px] text-muted-foreground">{dateTimeUtc(at)}</div>
                {readout.map((r) => (
                  <div key={r.s.id} className="flex items-center gap-2">
                    <span aria-hidden className={cn("size-2 rounded-full", r.tone.swatch)} />
                    <span className="min-w-0 flex-1 truncate">{r.s.label}</span>
                    <span className="font-medium text-bright tabular-nums">{r.p!.v === null ? "no data" : `${axisNumber(r.p!.v)}${unit ? ` ${unit}` : ""}`}</span>
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </div>
      {series.length > 0 && (
        <ul className="mt-3 flex flex-wrap items-center gap-x-6 gap-y-1.5 px-1 text-[13.5px]" aria-label="Legend" data-testid={`${testId}-legend`}>
          {series.map((s, i) => (
            <li key={s.id} className="flex min-w-0 max-w-full items-center gap-2">
              <span aria-hidden className={cn("size-2 shrink-0 rounded-full", tones[i]!.swatch)} />
              <span className="min-w-0 break-words">{s.label}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

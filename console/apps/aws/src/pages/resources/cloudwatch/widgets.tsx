// The widgets of a dashboard drawn (cloudwatch__dashboard-view): metric widgets as a line, stacked area, number, gauge, bar or pie,
// alarm status widgets and Markdown text, on a 24-column grid. A widget with no coordinates takes the next free place.
import { MoreHorizontalIcon } from "lucide-react";
import { useMemo } from "react";
import { Chart, type Series } from "@doze/ui/components/app/chart";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Button } from "@doze/ui/components/ui/button";
import { cn } from "@doze/ui/lib/utils";
import { type Range, useMetricData, useUnit } from "../../../kit";
import { type Alarm } from "./model";
import { type Placed, type Widget, alarmNames, headingSplit, numberLabel, ownRange, place, queriesOf, titleOf } from "./dashboard-model";
import { Markdown } from "./markdown";
import { SWATCH } from "./metrics-graphed";
import { slicePath, slices, sparkPath } from "./shapes";
import { stateTone } from "./words";

// Static class names for the stylesheet: the colour of a pie's slice, in the order of the swatches beside it.
const FILL = ["fill-cool", "fill-warn", "fill-good", "fill-refused", "fill-bad"];
const fmt = (v: number): string => v.toLocaleString("en-US", { maximumFractionDigits: 2 });
const latest = (s: Series): number | null => [...s.points].reverse().find((p) => p.v !== null)?.v ?? null;
const prop = <T,>(w: Widget, k: string): T | undefined => w.properties?.[k] as T | undefined;

function Frame({ title, children, onRemove, source, testId }: { title: string; children: React.ReactNode; onRemove?: () => void; source: string; testId: string }) {
  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col rounded-[14px] border bg-card p-5" data-testid={testId}>
      <div className="mb-3 flex items-start justify-between gap-2">
        <h3 className="min-w-0 truncate text-[14.5px] font-semibold text-bright">{title}</h3>
        <DropdownMenu>
          <DropdownMenuTrigger asChild><Button type="button" variant="ghost" size="icon" className="-mt-1 -mr-2 size-8" aria-label={`Widget menu for ${title}`} data-testid="widget-menu"><MoreHorizontalIcon /></Button></DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onSelect={() => void navigator.clipboard?.writeText(source)} data-testid="widget-copy">Copy widget JSON</DropdownMenuItem>
            {onRemove && <DropdownMenuItem onSelect={onRemove} data-testid="widget-remove">Remove widget</DropdownMenuItem>}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <div className="min-h-0 flex-1">{children}</div>
    </section>
  );
}

function MetricWidget({ w, range, height }: { w: Widget; range: Range; height: number }) {
  const queries = useMemo(() => queriesOf(w), [w]);
  const view = prop<string>(w, "view") ?? "timeSeries";
  const own = useMemo(() => ownRange(w, range.to), [w, range.to]);
  const data = useMetricData(queries, own ?? range, queries.length > 0);
  const unit = useUnit(queries, own ?? range, queries.length > 0);
  const series = data.series;
  const stacked = prop<boolean>(w, "stacked") === true;
  if (queries.length === 0) return <p className="text-[13.5px] text-muted-foreground" data-testid="widget-no-metrics">This widget names no metric.</p>;
  if (view === "timeSeries")
    return <Chart series={series.map((s) => ({ ...s, area: stacked || series.length === 1 }))} from={(own ?? range).from} to={(own ?? range).to} height={Math.max(120, height - 150)} unit={unit} loading={data.isPending} aria-label={`${titleOf(w)} over the range`} empty="No data points in this range." testId="widget-chart" />;
  const values = series.map((s) => ({ label: s.label, v: latest(s) }));
  if (view === "singleValue" || view === "gauge") {
    const [first] = values;
    const min = prop<{ left?: { min?: number; max?: number } }>(w, "yAxis")?.left?.min ?? 0;
    const max = prop<{ left?: { min?: number; max?: number } }>(w, "yAxis")?.left?.max ?? 100;
    const line = sparkPath(series[0]?.points ?? [], 240, 36);
    return (
      <div className="grid gap-2" data-testid="widget-number">
        <span className="text-[44px] leading-[48px] font-light text-bright tabular-nums" data-testid="widget-value">{first?.v === null || first === undefined ? "–" : fmt(first.v)}{unit && first?.v != null && <span className="ml-2 text-[15px] font-normal text-muted-foreground" data-testid="widget-unit">{unit}</span>}</span>
        <span className="text-[13.5px] text-muted-foreground">{first ? numberLabel(first.label, queries[0]) : ""}{queries[0]?.stat ? `, ${queries[0].stat}` : ""}</span>
        {line && view === "singleValue" && <svg viewBox="0 0 240 36" preserveAspectRatio="none" className="h-9 w-full" role="img" aria-label="The value over the range" data-testid="widget-spark"><path d={line} fill="none" className="stroke-cool" strokeWidth={1.5} strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" /></svg>}
        {view === "gauge" && first?.v != null && <div className="h-2 overflow-hidden rounded-full bg-muted" role="img" aria-label={`${fmt(first.v)} between ${min} and ${max}`}><div className="h-full rounded-full bg-primary" style={{ width: `${Math.min(100, Math.max(0, ((first.v - min) / (max - min)) * 100))}%` }} /></div>}
      </div>
    );
  }
  const top = Math.max(...values.map((x) => x.v ?? 0), 1e-9);
  const sum = values.reduce((n, x) => n + (x.v ?? 0), 0) || 1;
  const withUnit = (v: number) => (unit ? `${fmt(v)} ${unit}` : fmt(v));
  if (view === "table")
    return (
      <table className="w-full border-collapse text-left text-[13px]" data-testid="widget-table">
        <thead><tr className="border-b text-[11.5px] tracking-[0.07em] text-muted-foreground uppercase"><th className="py-2 pr-3 font-medium">Metric</th><th className="py-2 text-right font-medium">Latest value</th></tr></thead>
        <tbody>{values.map((x, i) => <tr key={x.label + i} className="border-b last:border-b-0"><td className="py-2 pr-3"><span className="flex items-center gap-2"><span aria-hidden className={cn("size-2 rounded-full", SWATCH[i % SWATCH.length])} />{x.label}</span></td><td className="py-2 text-right font-medium text-bright tabular-nums">{x.v === null ? "–" : withUnit(x.v)}</td></tr>)}</tbody>
      </table>
    );
  const cuts = view === "pie" ? slices(values.map((x) => x.v)) : [];
  return (
    <div className={view === "pie" ? "flex flex-wrap items-center gap-6" : undefined}>
      {view === "pie" && cuts.length > 0 && (
        <svg viewBox="0 0 100 100" className="size-32 shrink-0" role="img" aria-label="Each metric's share of the total" data-testid="widget-pie-chart">
          {cuts.map((c, i) => c.to > c.from && <path key={i} d={slicePath(50, 50, 48, c.from, c.to)} className={cn(FILL[i % FILL.length], "stroke-card")} strokeWidth={1.5} />)}
        </svg>
      )}
      <ul className="grid min-w-0 flex-1 gap-2.5" data-testid={`widget-${view}`}>
        {values.map((x, i) => (
          <li key={x.label + i} className="grid gap-1 text-[13px]">
            <span className="flex items-center justify-between gap-3"><span className="flex items-center gap-2 truncate"><span aria-hidden className={cn("size-2 rounded-full", SWATCH[i % SWATCH.length])} /><span className="truncate">{x.label}</span></span><span className="font-medium text-bright tabular-nums">{x.v === null ? "–" : view === "pie" ? `${fmt(((x.v ?? 0) / sum) * 100)}%` : withUnit(x.v)}</span></span>
            {view === "bar" && <span className="h-2 overflow-hidden rounded-full bg-muted"><span className={cn("block h-full rounded-full", SWATCH[i % SWATCH.length])} style={{ width: `${((x.v ?? 0) / top) * 100}%` }} /></span>}
          </li>
        ))}
      </ul>
    </div>
  );
}

function AlarmWidget({ w, alarms }: { w: Widget; alarms: readonly Alarm[] | undefined }) {
  const names = alarmNames(w);
  return (
    <ul className="grid gap-3" data-testid="widget-alarms">
      {names.map((n) => {
        const a = alarms?.find((x) => x.AlarmName === n);
        return (
          <li key={n} className="flex items-center justify-between gap-3 text-[13.5px]"><span className="truncate font-mono text-[13px]">{n}</span>{a ? <StatusDot tone={stateTone(a.StateValue)}>{a.StateValue}</StatusDot> : <span className="text-[12.5px] text-muted-foreground">{alarms ? "No such alarm" : "…"}</span>}</li>
        );
      })}
      {names.length === 0 && <li className="text-[13.5px] text-muted-foreground">This widget lists no alarm.</li>}
    </ul>
  );
}

const ROW = 40;
const GAP = 16;

export function Widgets({ widgets, range, alarms, onRemove }: { widgets: readonly Widget[]; range: Range; alarms: readonly Alarm[] | undefined; onRemove?: (index: number) => void }) {
  const placed = useMemo(() => place(widgets), [widgets]);
  const px = (p: Placed) => p.h * ROW + (p.h - 1) * GAP;
  return (
    <div className="grid grid-cols-1 gap-4 md:[grid-template-columns:repeat(24,minmax(0,1fr))] md:[grid-auto-rows:40px]" data-testid="dashboard-grid">
      {placed.map((p) => {
        const w = p.widget;
        // A text widget with no title of its own takes its first Markdown heading as the card's title.
        const split = w.type === "text" && !prop<string>(w, "title") ? headingSplit(prop<string>(w, "markdown") ?? "") : null;
        const title = split ? split.title : w.type === "text" && !prop<string>(w, "title") ? "" : titleOf(w);
        const frame = { title, source: JSON.stringify(w, null, 2), onRemove: onRemove ? () => onRemove(p.index) : undefined, testId: `widget-${p.index}` };
        return (
          <div key={p.index} className="flex min-w-0 flex-col md:[grid-column:var(--c)] md:[grid-row:var(--r)]" style={{ "--c": `${p.x + 1} / span ${p.w}`, "--r": `${p.y + 1} / span ${p.h}`, minHeight: px(p) } as React.CSSProperties}>
            <Frame {...frame}>
              {w.type === "metric" ? <MetricWidget w={w} range={range} height={px(p)} /> : w.type === "alarm" ? <AlarmWidget w={w} alarms={alarms} /> : w.type === "text" ? <Markdown source={split ? split.rest : (prop<string>(w, "markdown") ?? "")} /> : <p className="text-[13.5px] text-muted-foreground" data-testid="widget-unknown">doze-aws keeps this <span className="font-mono">{w.type ?? "widget"}</span> widget in the body; the console does not draw this type.</p>}
            </Frame>
          </div>
        );
      })}
    </div>
  );
}

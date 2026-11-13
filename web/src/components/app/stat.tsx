import { useLayoutEffect, useRef, useState } from "react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

export type Stat = {
  label: string;
  value: string;
  /** A word after the figure (`records`, `opened on first use`). */
  unit?: string;
  /** A static tooltip (the exact value, how long ago it was at render). */
  tip?: string;
  /** The value's reserved width in characters (default 8): a changing number never moves anything. */
  ch?: number;
  mono?: boolean;
};

/** One figure: a label, a value in a slot of reserved width with tabular figures, an optional unit (CON-805: a live value is
 * never in prose). Its three parts are rows of the strip's grid (subgrid), so in a strip every value sits at the same height
 * even when one label takes two lines. */
export function StatTile({ stat, className, style, dense = false }: { stat: Stat; className?: string; style?: React.CSSProperties; /** Less height: a page head whose body is the point. */ dense?: boolean }) {
  const value = (
    <dd
      className={cn("truncate text-[22px] leading-8 font-medium tracking-tight text-bright tabular-nums", stat.mono && "font-mono text-[19px]", dense && "text-[20px] leading-7", dense && stat.mono && "text-[17px]")}
      // Reserved width, but never wider than its tile: a strip being laid out again (a window resized) never overflows.
      style={{ minWidth: `min(${stat.ch ?? 8}ch, 100%)`, maxWidth: "100%" }}
      data-slot="stat-value"
    >
      {stat.value}
    </dd>
  );
  return (
    <div className={cn("row-span-3 grid min-w-0 grid-rows-subgrid gap-0 rounded-xl border bg-card px-5 py-4", dense && "px-4 py-3", className)} style={style} data-slot="stat">
      <dt className="self-end text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">{stat.label}</dt>
      {stat.tip ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <div tabIndex={0} className="mt-1 outline-none focus-visible:ring-2 focus-visible:ring-ring/50 rounded-sm">{value}</div>
          </TooltipTrigger>
          <TooltipContent>{stat.tip}</TooltipContent>
        </Tooltip>
      ) : (
        <div className="mt-1">{value}</div>
      )}
      <div className="truncate text-[12.5px] text-muted-foreground" title={stat.unit}>
        {stat.unit ?? " "}
      </div>
    </div>
  );
}

/** The least a tile is wide (px) before the strip breaks into more rows. */
const TILE_MIN = 150;
const GAP = 12;
/** Columns of the grid a strip lays its rows out on: 60 is divided evenly by 1 to 6 tiles a row. */
const UNITS = 60;

/** How many tiles each row holds for `n` tiles in `width` px: all in one row when they fit; else as few rows as fit, as even
 * as they can be (5 in two rows is 3 and 2, never 4 and an orphan). */
export function rowsOf(n: number, width: number): number[] {
  const fit = Math.max(1, Math.floor((width + GAP) / (TILE_MIN + GAP)));
  const rows = Math.ceil(n / fit);
  const per = Math.ceil(n / rows);
  const out: number[] = [];
  for (let left = n; left > 0; left -= per) out.push(Math.min(per, left));
  return out;
}

/** A row of figures: never a sentence. Balanced (every row fills the width, no lone tile at the end), and the values of a
 * row at one height (the tiles are a subgrid: a label on two lines moves no value). Laid out before the first paint. */
export function StatStrip({ stats, className, dense = false }: { stats: Stat[]; className?: string; dense?: boolean }) {
  const box = useRef<HTMLDListElement>(null);
  const [width, setWidth] = useState(1200);
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    setWidth(el.clientWidth);
    const ro = new ResizeObserver(([e]) => setWidth(Math.round(e?.contentRect.width ?? el.clientWidth)));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const rows = rowsOf(stats.length, width);
  const units = rows.every((r) => UNITS % r === 0) ? UNITS : rows.reduce((a, r) => a * r, 1);
  const spans: number[] = rows.flatMap((r) => Array.from({ length: r }, () => units / r));
  return (
    <dl ref={box} className={cn("grid gap-x-3 gap-y-3", className)} style={{ gridTemplateColumns: `repeat(${units}, minmax(0, 1fr))` }} data-slot="stat-strip" data-rows={rows.join("+")}>
      {stats.map((s, i) => (
        <StatTile key={s.label} stat={s} dense={dense} className="[row-gap:0]" style={{ gridColumn: `span ${spans[i]} / span ${spans[i]}` }} />
      ))}
    </dl>
  );
}

export type Figure = { label: string; value: string; /** Reserved width of the value in characters (default 4). */ ch?: number; tip?: string; testId?: string };

/** A compact row of labelled figures (a label over its value, the value in a slot of reserved width with tabular figures): for
 * counts that would otherwise be a sentence with numbers in it (CON-805). One line where it fits; wraps whole figures. */
export function Figures({ figures, className, "aria-label": label }: { figures: Figure[]; className?: string; "aria-label"?: string }) {
  return (
    <dl className={cn("flex flex-wrap gap-x-7 gap-y-3", className)} aria-label={label} data-slot="figures">
      {figures.map((f) => (
        <div key={f.label} className="min-w-0" data-testid={f.testId} title={f.tip}>
          <dt className="text-[11.5px] font-medium tracking-[0.06em] whitespace-nowrap text-muted-foreground uppercase">{f.label}</dt>
          <dd className="mt-0.5 text-[17px] leading-6 font-medium text-bright tabular-nums" style={{ minWidth: `${f.ch ?? 4}ch` }} data-slot="figure-value">
            {f.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}

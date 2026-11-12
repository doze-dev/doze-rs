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
 * never in prose). */
export function StatTile({ stat, className }: { stat: Stat; className?: string }) {
  const value = (
    <dd
      className={cn("truncate text-[22px] leading-8 font-medium tracking-tight text-bright tabular-nums", stat.mono && "font-mono text-[19px]")}
      style={{ minWidth: `${stat.ch ?? 8}ch`, maxWidth: "100%" }}
      data-slot="stat-value"
    >
      {stat.value}
    </dd>
  );
  return (
    <div className={cn("min-w-0 rounded-xl border bg-card px-5 py-4", className)} data-slot="stat">
      <dt className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">{stat.label}</dt>
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

/** A row of figures that wraps into a grid (at least 170 px each): never a sentence. */
export function StatStrip({ stats, className }: { stats: Stat[]; className?: string }) {
  return (
    <dl className={cn("grid grid-cols-[repeat(auto-fit,minmax(170px,1fr))] gap-3", className)} data-slot="stat-strip">
      {stats.map((s) => (
        <StatTile key={s.label} stat={s} />
      ))}
    </dl>
  );
}

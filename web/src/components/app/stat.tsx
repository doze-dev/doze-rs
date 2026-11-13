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
    <dl className={cn("flex flex-wrap gap-3", className)} data-slot="stat-strip">
      {stats.map((s) => (
        <StatTile key={s.label} stat={s} className="flex-1 basis-[170px]" />
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

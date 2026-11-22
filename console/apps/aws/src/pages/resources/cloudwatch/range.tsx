// The time range of a graph (1h to 1w, or a custom span in UTC) and its refresh options: shared by the alarm's graph, the metrics
// page, and a dashboard. The range is `now` minus the span, `now` being when it was chosen or last refreshed.
import { RefreshCwIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { UtcDateTimePicker } from "@doze/ui/components/app/utc-datetime-picker";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@doze/ui/components/ui/popover";
import { ToggleGroup, ToggleGroupItem } from "@doze/ui/components/ui/toggle-group";
import { cn } from "@doze/ui/lib/utils";
import { RANGES, type Range } from "../../../kit";

export type RangeState = { id: string; now: number; custom: Range | null; every: number };

export const initialRange = (id = "3h"): RangeState => ({ id, now: Date.now(), custom: null, every: 0 });

/** The span a state means. */
export function rangeOf(s: RangeState): Range {
  if (s.id === "custom" && s.custom) return s.custom;
  const ms = RANGES.find((r) => r.id === s.id)?.ms ?? 3 * 3_600_000;
  return { from: s.now - ms, to: s.now };
}

/** The range state of a page: the range, and a refresh on the interval it names. */
export function useRange(id = "3h"): [RangeState, (patch: Partial<RangeState>) => void, Range] {
  const [s, setS] = useState<RangeState>(() => initialRange(id));
  const patch = (p: Partial<RangeState>) => setS((x) => ({ ...x, ...p }));
  useEffect(() => {
    if (!s.every || s.id === "custom") return;
    const t = setInterval(() => setS((x) => ({ ...x, now: Date.now() })), s.every * 1000);
    return () => clearInterval(t);
  }, [s.every, s.id]);
  const range = useMemo(() => rangeOf(s), [s]);
  return [s, patch, range];
}

const EVERY = [
  ["0", "Off"],
  ["10", "10 seconds"],
  ["60", "1 minute"],
  ["300", "5 minutes"],
] as const;

export function RangeBar({ state, onChange, className, compact = false }: { state: RangeState; onChange: (patch: Partial<RangeState>) => void; className?: string; compact?: boolean }) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<Range>(() => rangeOf(state));
  return (
    <div className={cn("flex flex-wrap items-center gap-3", className)} data-testid="range-bar">
      <ToggleGroup type="single" value={state.id} onValueChange={(v) => v && v !== "custom" && onChange({ id: v, now: Date.now() })} aria-label="Time range" className="gap-0.5 rounded-lg border bg-muted p-0.5">
        {RANGES.map((r) => (
          <ToggleGroupItem key={r.id} value={r.id} className="h-8 rounded-md px-3.5 text-[13.5px] font-normal text-muted-foreground data-[state=on]:bg-card data-[state=on]:font-medium data-[state=on]:text-bright data-[state=on]:shadow-xs" data-testid={`range-${r.id}`}>
            {r.label}
          </ToggleGroupItem>
        ))}
        <Popover open={open} onOpenChange={(o) => { setOpen(o); if (o) setDraft(rangeOf(state)); }}>
          <PopoverTrigger asChild>
            <ToggleGroupItem value="custom" className="h-8 rounded-md px-3.5 text-[13.5px] font-normal text-muted-foreground data-[state=on]:bg-card data-[state=on]:font-medium data-[state=on]:text-bright data-[state=on]:shadow-xs" data-testid="range-custom" onClick={(e) => { e.preventDefault(); setOpen(true); }}>
              Custom
            </ToggleGroupItem>
          </PopoverTrigger>
          <PopoverContent align="start" className="grid w-80 gap-3 p-4" data-testid="range-custom-popover">
            <label className="grid gap-1 text-[13px] text-muted-foreground">Start, UTC<UtcDateTimePicker value={draft.from} onChange={(from) => setDraft((d) => ({ ...d, from }))} aria-label="Start, UTC" /></label>
            <label className="grid gap-1 text-[13px] text-muted-foreground">End, UTC<UtcDateTimePicker value={draft.to} onChange={(to) => setDraft((d) => ({ ...d, to }))} aria-label="End, UTC" /></label>
            {draft.from >= draft.to && <p className="text-[12.5px] text-bad" data-testid="range-custom-problem">The start must be before the end.</p>}
            <Button type="button" disabled={draft.from >= draft.to} onClick={() => { onChange({ id: "custom", custom: draft }); setOpen(false); }} data-testid="range-custom-apply">Apply</Button>
          </PopoverContent>
        </Popover>
      </ToggleGroup>
      {!compact && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button type="button" variant="outline" className="bg-card" data-testid="refresh-options"><RefreshCwIcon /> Refresh options</Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="min-w-52">
            <DropdownMenuRadioGroup value={String(state.every)} onValueChange={(v) => onChange({ every: Number(v), now: Date.now() })}>
              {EVERY.map(([v, label]) => <DropdownMenuRadioItem key={v} value={v} data-testid={`refresh-${v}`}>{v === "0" ? "Refresh only when I ask" : `Refresh every ${label}`}</DropdownMenuRadioItem>)}
            </DropdownMenuRadioGroup>
            <div className="border-t p-1.5"><Button type="button" variant="ghost" size="sm" className="w-full justify-start" onClick={() => onChange({ now: Date.now() })} data-testid="refresh-now">Refresh now</Button></div>
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </div>
  );
}

/** `last 3 hours`, `last 7 days`: the span of a range, said. */
export function spanWords(s: RangeState): string {
  if (s.id === "custom") return "custom range";
  const ms = RANGES.find((x) => x.id === s.id)?.ms ?? 3 * 3_600_000;
  const [n, unit] = ms >= 86_400_000 ? [ms / 86_400_000, "day"] : [ms / 3_600_000, "hour"];
  return `last ${n === 1 ? "" : `${n} `}${unit}${n === 1 ? "" : "s"}`;
}

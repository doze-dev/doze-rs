import { RefreshCwIcon } from "lucide-react";
import { useEffect } from "react";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { clockUtc } from "@/lib/format";
import { INTERVALS, type IntervalValue, parseInterval, useRefreshInterval } from "@/lib/refresh";
import { cn } from "@/lib/utils";

/** Whether a key press belongs to a field (so `r` types an r there). */
export function typing(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  return !!t && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName) || t.closest("[role=dialog],[role=listbox],[role=menu]") !== null);
}

/** The Refresh control of a data page (live-design.md): the time of the data on screen, the interval (Off by default, one
 * preference for every page, remembered), and a Refresh button (key `r`). The stamp changes only when new data arrived.
 *
 * `interval={false}`: the stamp and the button only, for a page whose Snapshot | Live tail switch owns its updates (Live, a
 * topic's Messages): there the switch is the way to keep a page current, so there is no interval to pick. */
export function RefreshControl({ updatedAt, fetching, onRefresh, className, interval: withInterval = true }: { updatedAt: number; fetching: boolean; onRefresh: () => void; className?: string; interval?: boolean }) {
  const [interval, setInterval] = useRefreshInterval();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "r" && !e.metaKey && !e.ctrlKey && !e.altKey && !typing(e)) {
        e.preventDefault();
        onRefresh();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onRefresh]);
  return (
    <div className={cn("flex items-center gap-2", className)} data-slot="refresh-control">
      <Tooltip>
        <TooltipTrigger asChild>
          <span tabIndex={0} className={cn("inline-block text-right text-[12.5px] whitespace-nowrap text-muted-foreground tabular-nums outline-none", withInterval && "min-w-[9.5rem]")} data-testid="refresh-stamp">
            {updatedAt > 0 ? `Updated ${clockUtc(updatedAt)} UTC` : "Not loaded yet"}
          </span>
        </TooltipTrigger>
        <TooltipContent>The time of the data on this page. It changes only when new data arrives.</TooltipContent>
      </Tooltip>
      {withInterval && (
      <Select value={interval} onValueChange={(v) => setInterval(parseInterval(v) as IntervalValue)}>
        <SelectTrigger size="sm" className="w-[9.25rem] bg-card" aria-label="Refresh every" data-testid="refresh-interval">
          <SelectValue />
        </SelectTrigger>
        <SelectContent align="end">
          {INTERVALS.map((i) => (
            <SelectItem key={i.value} value={i.value}>
              {i.value === "off" ? "Refresh: Off" : `Every ${i.label}`}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      )}
      <Tooltip>
        <TooltipTrigger asChild>
          <Button variant="outline" size="sm" className="bg-card" onClick={onRefresh} data-testid="refresh-now">
            <RefreshCwIcon className={cn(fetching && "animate-spin [animation-duration:1.2s]")} />
            Refresh
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          Fetch this page again <kbd className="ml-1 rounded border px-1 font-mono text-[11px]">r</kbd>
        </TooltipContent>
      </Tooltip>
    </div>
  );
}

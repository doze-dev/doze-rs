import { CheckIcon, RefreshCwIcon } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "../ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { Tooltip, TooltipContent, TooltipTrigger } from "../ui/tooltip";
import { clockUtc } from "../../lib/format";
import { INTERVALS, type IntervalValue, parseInterval, useRefreshInterval } from "../../lib/refresh";
import { cn } from "../../lib/utils";

/** Whether a key press belongs to a field (so `r` types an r there). */
export function typing(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  return !!t && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName) || t.closest("[role=dialog],[role=listbox],[role=menu]") !== null);
}

/** How long the icon turns at least, and how long the check stays: a local answer comes back in a few milliseconds, which is too quick to be
 * seen, so a press would feel as if nothing had happened. */
const MIN_SPIN_MS = 700;
const DONE_MS = 1500;

/** The Refresh control of a data page (live-design.md): the time of the data on screen, the interval (Off by default, one
 * preference for every page, remembered), and a Refresh button (key `r`). The stamp changes only when new data arrived.
 *
 * `interval={false}`: the stamp and the button only, for a page whose Snapshot | Live tail switch owns its updates (Live, a
 * topic's Messages): there the switch is the way to keep a page current, so there is no interval to pick. */
export function RefreshControl({ updatedAt, fetching, onRefresh, className, interval: withInterval = true }: { updatedAt: number; fetching: boolean; onRefresh: () => void; className?: string; interval?: boolean }) {
  const [interval, setInterval] = useRefreshInterval();
  // A press is felt: the icon turns for at least a moment (whatever the answer's speed), then a check shows that it came, and the time of the data
  // takes the ink for as long. (The interval's own refreshes are not announced this way: only a press is.)
  const [phase, setPhase] = useState<"idle" | "turning" | "done">("idle");
  const pressed = useRef(0);
  const press = useCallback(() => {
    pressed.current = performance.now();
    setPhase("turning");
    onRefresh();
  }, [onRefresh]);
  useEffect(() => {
    if (phase !== "turning" || fetching) return;
    const t = setTimeout(() => setPhase("done"), Math.max(0, MIN_SPIN_MS - (performance.now() - pressed.current)));
    return () => clearTimeout(t);
  }, [phase, fetching]);
  useEffect(() => {
    if (phase !== "done") return;
    const t = setTimeout(() => setPhase("idle"), DONE_MS);
    return () => clearTimeout(t);
  }, [phase]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "r" && !e.metaKey && !e.ctrlKey && !e.altKey && !typing(e)) {
        e.preventDefault();
        press();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [press]);
  return (
    <div className={cn("flex items-center gap-2", className)} data-slot="refresh-control">
      <Tooltip>
        <TooltipTrigger asChild>
          <span tabIndex={0} className={cn("inline-block text-right text-[12.5px] whitespace-nowrap tabular-nums transition-colors duration-300 outline-none focus-visible:rounded-sm focus-visible:ring-[3px] focus-visible:ring-ring/50", withInterval && "min-w-[9.5rem]", phase === "done" ? "font-medium text-good" : "text-muted-foreground")} data-testid="refresh-stamp">
            {updatedAt > 0 ? `Updated ${clockUtc(updatedAt)} UTC` : "Not loaded yet"}
          </span>
        </TooltipTrigger>
        <TooltipContent>The time of the data on this page. It changes only when new data arrives.</TooltipContent>
      </Tooltip>
      <span role="status" className="sr-only">{phase === "done" ? "Refreshed" : ""}</span>
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
          <Button variant="outline" size="sm" className="bg-card" onClick={press} data-testid="refresh-now" data-phase={phase}>
            {phase === "done" ? <CheckIcon className="text-good" aria-hidden /> : <RefreshCwIcon className={cn((fetching || phase === "turning") && "animate-spin [animation-duration:0.7s]")} aria-hidden />}
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

// The controls every tail shares (Live, a topic's Messages): one speed control (one preference, one set of words, usable in
// Snapshot too: the speed is chosen before the tail is switched on), and the held pill.
import { ArrowUpIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { number } from "@/lib/format";
import { cn } from "@/lib/utils";
import { SPEEDS, type Speed, parseSpeed, useSpeed } from "@/pages/live/tail";

/** The tail's speed: Realtime, Relaxed or Slow, each with its pace (`Relaxed · 3 s`), remembered per browser for every tail. */
export function TailSpeed({ testId = "tail-speed" }: { testId?: string }) {
  const [speed, setSpeed] = useSpeed();
  return (
    <Select value={speed} onValueChange={(v) => setSpeed(parseSpeed(v) as Speed)}>
      <SelectTrigger size="sm" className="w-[9.5rem] bg-card" aria-label="Tail speed" data-testid={testId}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent align="end">
        {SPEEDS.map((s) => (
          <SelectItem key={s.value} value={s.value} title={s.tip}>
            {s.label} · {s.secs} s
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

/** "↑ 37 new · Show": the rows a tail holds while the person reads. It sits in the tail's own bar, in a slot it always has
 * (hidden when there is nothing held), so it covers no row and moves nothing when it comes and goes. */
export function HeldPill({ count, onShow, className }: { count: number; onShow: () => void; className?: string }) {
  return (
    <Button size="sm" onClick={onShow} className={cn("min-w-[8.5rem] rounded-full", count === 0 && "invisible", className)} aria-hidden={count === 0 || undefined} tabIndex={count === 0 ? -1 : undefined} data-testid={count > 0 ? "held-pill" : undefined}>
      <ArrowUpIcon /> <span className="tabular-nums">{number(count)}</span> new · Show
    </Button>
  );
}

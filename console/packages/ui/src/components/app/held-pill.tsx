import { ArrowUpIcon } from "lucide-react";
import { number } from "../../lib/format";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";

/** "↑ 37 new · Show": the rows a tail holds while the person reads. It sits in the tail's own bar, in a slot it always has
 * (hidden when there is nothing held), so it covers no row and moves nothing when it comes and goes. */
export function HeldPill({ count, onShow, className }: { count: number; onShow: () => void; className?: string }) {
  return (
    <Button size="sm" onClick={onShow} className={cn("min-w-[8.5rem] rounded-full", count === 0 && "invisible", className)} aria-hidden={count === 0 || undefined} tabIndex={count === 0 ? -1 : undefined} data-testid={count > 0 ? "held-pill" : undefined}>
      <ArrowUpIcon /> <span className="tabular-nums">{number(count)}</span> new · Show
    </Button>
  );
}

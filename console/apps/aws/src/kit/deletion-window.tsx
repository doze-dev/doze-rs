import { RotateCcwIcon } from "lucide-react";
import { useId } from "react";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { cn } from "@doze/ui/lib/utils";

/** The days a key or a secret waits before it is gone: AWS's own limits (7 to 30 for KMS keys and for Secrets Manager's
 * recovery window). */
export const WINDOW = { min: 7, max: 30, default: 30 } as const;

/** The window as a whole number of days inside the limits, or `null` for anything that is not a whole number. */
export function windowDays(text: string, lim: { min: number; max: number } = WINDOW): number | null {
  if (!/^\d+$/.test(text.trim())) return null;
  const n = Number(text);
  return n >= lim.min && n <= lim.max ? n : null;
}

/** When it will be gone: the day the window ends, as `Nov 15, 2026` (UTC), counting `days` from `from` (ms). */
export function purgeDate(from: number, days: number): string {
  const d = new Date(from + days * 86_400_000);
  return `${MONTHS[d.getUTCMonth()]} ${d.getUTCDate()}, ${d.getUTCFullYear()}`;
}
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** Choosing the window (`choose`: a number of days, and the date it ends) or a deletion already scheduled (`scheduled`: the date,
 * and Cancel deletion, which restores it). The words are the same for a key and a secret; `noun` names which. */
export function DeletionWindow(props: ({ mode: "choose"; days: string; onDays: (text: string) => void; now: number } | { mode: "scheduled"; /** ms. */ on: number; onRestore: () => void; restoring?: boolean }) & { noun: string; limits?: { min: number; max: number }; className?: string; /** The label over the field, when it is not `Waiting period, in days`. */ label?: string; /** Only the field (and what is wrong with it): the page says the rest in one note of its own. */ bare?: boolean }) {
  const id = useId();
  const lim = props.limits ?? WINDOW;
  if (props.mode === "scheduled") {
    return (
      <div className={cn("flex flex-wrap items-center justify-between gap-3 rounded-xl border border-warn/30 bg-warn/10 px-4 py-3 text-[14px]", props.className)} data-slot="deletion-window" data-testid="deletion-scheduled">
        <p>
          <b className="font-semibold text-bright">This {props.noun} is scheduled for deletion on {dateTimeUtc(props.on)}.</b> Until then it cannot be used, and Cancel deletion brings it back.
        </p>
        <Button type="button" variant="outline" className="bg-card" onClick={props.onRestore} disabled={props.restoring} data-testid="deletion-restore">
          <RotateCcwIcon /> {props.restoring ? "Restoring…" : "Cancel deletion"}
        </Button>
      </div>
    );
  }
  const n = windowDays(props.days, lim);
  return (
    <div className={cn("grid gap-1.5", props.className)} data-slot="deletion-window" data-testid="deletion-window">
      <Label htmlFor={id}>{props.label ?? "Waiting period, in days"}</Label>
      <div className="flex flex-wrap items-center gap-3">
        <div className="relative w-32">
          <Input id={id} inputMode="numeric" value={props.days} onChange={(e) => props.onDays(e.target.value)} aria-invalid={n === null} aria-describedby={`${id}-help`} className="pr-12 font-mono tabular-nums" data-testid="deletion-days" />
          <span aria-hidden className="pointer-events-none absolute inset-y-0 right-3 flex items-center text-[13px] text-muted-foreground">days</span>
        </div>
        {(n === null || !props.bare) && (
          <span className="text-[13.5px] text-muted-foreground" data-testid="deletion-ends">
            {n === null ? <span className="text-bad">Enter a whole number of days from {lim.min} to {lim.max}.</span> : <>With {n}, the {props.noun} is deleted on {purgeDate(props.now, n)}.</>}
          </span>
        )}
      </div>
      {!props.bare && (
        <p id={`${id}-help`} className="border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground">
          From <b className="font-semibold text-foreground">{lim.min}</b> to <b className="font-semibold text-foreground">{lim.max}</b> days. Until then the {props.noun} cannot be used, and Cancel deletion restores it.
        </p>
      )}
    </div>
  );
}

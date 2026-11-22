import { CircleCheckIcon, CircleDashedIcon, CircleXIcon, type LucideIcon, ShieldAlertIcon, ShieldXIcon } from "lucide-react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@doze/ui/components/ui/tooltip";
import { cn } from "@doze/ui/lib/utils";
import type { CallRecord } from "../../api/types/CallRecord";
import { OUTCOMES, type OutcomeTone, WOULD_BE_DENIED } from "../../words";

/** Each outcome its own look. Error and denied share the `--bad` hue; denied is told apart by its shield and an outlined word.
 * Not built is muted: doze-aws has not built it, nobody did anything wrong. */
const TONE: Record<OutcomeTone, { ink: string; icon: LucideIcon; word?: string }> = {
  good: { ink: "text-good", icon: CircleCheckIcon },
  bad: { ink: "text-bad", icon: CircleXIcon },
  denied: { ink: "text-bad", icon: ShieldXIcon, word: "rounded-[4px] border border-current px-1 py-px leading-none" },
  muted: { ink: "text-muted-foreground", icon: CircleDashedIcon },
};

/** A request the service refuses (an answer under 500 that is not IAM's): AWS itself would refuse it, so it is REFUSED, in the
 * refused hue; the same answer from a service that failed (5xx) stays ERROR. */
export const isRefused = (c: Pick<CallRecord, "outcome" | "status">) => c.outcome === "error" && c.status < 500;

type ResultCall = Pick<CallRecord, "outcome" | "status" | "error_code" | "would_be_denied">;

/** How a call ended, on one line: an icon and the status for a success; for anything else the contract's word in capitals
 * (ERROR, DENIED, NOT BUILT), the status and the error code. The word and the icon carry the meaning; the colour repeats it.
 * A call `--iam soft` let through that it would refuse carries a small marker, explained on hover and focus. */
export function Result({ call, className }: { call: ResultCall; className?: string }) {
  const o = OUTCOMES[call.outcome];
  const refused = isRefused(call);
  // Soft IAM let it through: the word is what real AWS would have said, the status is what happened.
  const soft = call.would_be_denied && call.outcome === "ok";
  const t: { ink: string; icon: LucideIcon; word?: string } = refused ? { ink: "text-refused", icon: CircleXIcon } : TONE[o.tone];
  const word = refused ? "REFUSED" : o.label;
  return (
    <span className={cn("flex max-w-full min-w-0 items-center gap-1.5 whitespace-nowrap", className)} data-outcome={call.outcome} data-slot="result">
      {soft ? <ShieldAlertIcon aria-hidden className="size-3.5 shrink-0 text-warn" /> : <t.icon aria-hidden className={cn("size-3.5 shrink-0", t.ink)} />}
      {soft ? (
        <WouldBeDenied status={call.status} />
      ) : call.outcome === "ok" ? (
        <span className={cn("font-semibold tabular-nums", t.ink)}>
          <span className="sr-only">OK </span>
          {call.status}
        </span>
      ) : (
        <>
          <span className={cn("shrink-0 text-[11.5px] font-bold tracking-[0.05em]", t.ink, t.word)}>{word}</span>
          <span className="shrink-0 font-mono text-[12px] tabular-nums">{call.status}</span>
          {call.error_code && (
            <span className="min-w-0 truncate font-mono text-[12px]" title={call.error_code}>
              {call.error_code}
            </span>
          )}
        </>
      )}
      {call.would_be_denied && !soft && <WouldBeDenied status={call.status} />}
    </span>
  );
}

/** The marker on a call soft IAM let through. */
function WouldBeDenied({ status }: { status: number }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span tabIndex={0} className="inline-flex shrink-0 items-center gap-1.5 rounded-sm text-[11.5px] font-bold tracking-[0.05em] text-warn uppercase outline-none focus-visible:ring-2 focus-visible:ring-ring" data-slot="would-be-denied" data-status={status} aria-label={`${WOULD_BE_DENIED.short}. ${WOULD_BE_DENIED.sentence}`}>
          would be denied
          <span className="font-sans font-normal tracking-normal text-muted-foreground normal-case">soft</span>
        </span>
      </TooltipTrigger>
      <TooltipContent className="max-w-[280px]">{WOULD_BE_DENIED.sentence}</TooltipContent>
    </Tooltip>
  );
}

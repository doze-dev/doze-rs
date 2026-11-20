import { BanIcon, CircleCheckIcon, CircleXIcon, type LucideIcon, ShieldXIcon } from "lucide-react";
import { cn } from "@doze/ui/lib/utils";
import type { CallRecord } from "../../api/types/CallRecord";
import { OUTCOMES, type OutcomeTone } from "../../words";

const TONE: Record<OutcomeTone, { ink: string; icon: LucideIcon }> = {
  good: { ink: "text-good", icon: CircleCheckIcon },
  bad: { ink: "text-bad", icon: CircleXIcon },
  warn: { ink: "text-warn", icon: ShieldXIcon },
  refused: { ink: "text-refused", icon: BanIcon },
};

/** How a call ended, on one line: an icon and the status for a success; for anything else the contract's word in capitals
 * (ERROR, DENIED, REFUSED), the status and the error code. The word and the icon carry the meaning; the colour repeats it. */
export function Result({ call, className }: { call: Pick<CallRecord, "outcome" | "status" | "error_code">; className?: string }) {
  const o = OUTCOMES[call.outcome];
  const t = TONE[o.tone];
  return (
    <span className={cn("inline-flex min-w-0 items-center gap-1.5 whitespace-nowrap", className)} data-outcome={call.outcome} data-slot="result">
      <t.icon aria-hidden className={cn("size-3.5 shrink-0", t.ink)} />
      {call.outcome === "ok" ? (
        <span className={cn("font-semibold tabular-nums", t.ink)}>
          <span className="sr-only">OK </span>
          {call.status}
        </span>
      ) : (
        <>
          <span className={cn("text-[11.5px] font-bold tracking-[0.05em]", t.ink)}>{o.label}</span>
          <span className="font-mono text-[12px] tabular-nums">{call.status}</span>
          {call.error_code && <span className="truncate font-mono text-[12px]">{call.error_code}</span>}
        </>
      )}
    </span>
  );
}

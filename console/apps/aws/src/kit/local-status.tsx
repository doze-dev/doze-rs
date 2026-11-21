import { cn } from "@doze/ui/lib/utils";

/** What doze-aws does with a setting it keeps but does not act on. `stored`: kept and returned, never applied (encryption of
 * messages). `inactive`: the feature itself is not active locally (a trigger that never fires). The words are fixed, so the same
 * thing is said the same way on every page; `reason` is the one line that says why, for this setting. */
export type LocalState = "stored" | "inactive";

export const LOCAL_WORDS: Record<LocalState, string> = { stored: "Stored, not applied locally", inactive: "Not active locally" };

export function LocalStatus({ state, reason, className }: { state: LocalState; reason?: React.ReactNode; className?: string }) {
  return (
    <p className={cn("text-[13px] leading-5 text-muted-foreground", className)} data-slot="local-status" data-state={state}>
      <b className="font-semibold text-foreground">{LOCAL_WORDS[state]}.</b>
      {reason && <> {reason}</>}
    </p>
  );
}

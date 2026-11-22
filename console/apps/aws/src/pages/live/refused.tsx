import { BanIcon } from "lucide-react";
import type { CallRecord } from "../../api/types/CallRecord";
import { isRefused } from "./outcome";
import { whatWouldPass } from "./refusal";

/** Why a request was refused: the service's own words for the rule that was broken, the code, and what would pass. The words are
 * the record's (the service said them when it refused); what would pass is said from the code. */
export function Refused({ call }: { call: CallRecord }) {
  if (!isRefused(call)) return null;
  return (
    <section className="mt-5 rounded-xl border bg-card p-4" aria-label="Why it was refused" data-testid="refused">
      <h3 className="flex items-center gap-2 text-[15px] font-semibold text-bright">
        <BanIcon aria-hidden className="size-4 text-refused" /> Why it was refused
      </h3>
      <div className="mt-3 text-[11.5px] font-medium tracking-wide text-muted-foreground uppercase">The rule that fired</div>
      {call.message ? (
        <p className="mt-1 border-l-2 border-refused pl-3 text-[13.5px]" data-testid="refused-message">
          {call.message}
        </p>
      ) : (
        <p className="mt-1 text-[13px] text-muted-foreground" data-testid="refused-no-message">
          The service refused it before it read the request, and said nothing more than the code.
        </p>
      )}
      <div className="mt-2 flex flex-wrap gap-2 font-mono text-[11.5px]">
        <span className="rounded-md border bg-muted px-2 py-0.5">HTTP {call.status}</span>
        {call.error_code && <span className="rounded-md border bg-muted px-2 py-0.5">{call.error_code}</span>}
      </div>
      <div className="mt-4 text-[11.5px] font-medium tracking-wide text-muted-foreground uppercase">What would pass</div>
      <p className="mt-1 text-[13.5px]" data-testid="refused-pass">
        {whatWouldPass(call)}
      </p>
    </section>
  );
}

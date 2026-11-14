import { CheckIcon, RefreshCwIcon, XIcon } from "lucide-react";
import { toast } from "sonner";
import { useProbe } from "@/api/queries";
import type { ProbeView } from "@/api/types/ProbeView";
import type { Verdict } from "@/api/types/Verdict";
import { Spans } from "@/components/app/spans";
import { StatusDot, type Tone } from "@/components/app/status-dot";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

const VERDICT: Record<Verdict, [Tone, string]> = {
  reachable: ["ok", "Reachable"],
  not_reachable: ["bad", "Not reachable"],
  not_checked: ["idle", "Not checked"],
};

export function VerdictDot({ verdict }: { verdict: Verdict }) {
  const [tone, word] = VERDICT[verdict];
  return (
    <StatusDot tone={tone} data-testid="verdict">
      {word}
    </StatusDot>
  );
}

/** The three rules of the advertised-listener check, or the sentence when it did not finish. */
export function ProbeRules({ probe }: { probe: ProbeView }) {
  // Home never dials on its own (it can refresh on an interval), so before the first check, or after one that did not finish
  // in time, there is nothing to claim.
  if (probe.rules.length === 0) return <p className="text-[13px] text-muted-foreground" data-testid="probe-unchecked">Not checked: nothing is claimed until a check finishes. Check again dials the listener and the address clients are told to use.</p>;
  return (
    <ul className="flex flex-col divide-y rounded-xl border" data-testid="probe-rules">
      {probe.rules.map((r) => (
        <li key={r.title} className="flex items-start gap-3 px-4 py-3">
          <span className={cn("mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full", r.ok ? "bg-good/10 text-good" : "bg-bad/10 text-bad")}>
            {r.ok ? <CheckIcon className="size-3.5" aria-label="passed" /> : <XIcon className="size-3.5" aria-label="failed" />}
          </span>
          <div className="min-w-0 flex-1">
            <div className="text-[13.5px] font-medium text-bright">{r.title}</div>
            <div className="text-[12.5px] text-muted-foreground">
              <Spans spans={r.detail} />
            </div>
          </div>
          <span className="shrink-0 pt-0.5 font-mono text-[12px] text-muted-foreground tabular-nums">{r.value}</span>
        </li>
      ))}
    </ul>
  );
}

export function CheckAgain({ size = "sm" }: { size?: "sm" | "default" }) {
  const probe = useProbe();
  return (
    <Button
      variant="outline"
      size={size}
      className="bg-card"
      disabled={probe.isPending}
      data-testid="check-again"
      onClick={() =>
        probe.mutate(undefined, {
          onSuccess: (p) => toast.success("Checked the advertised listener", { description: VERDICT[p.verdict][1] }),
          onError: (e) => toast.error("The check could not run", { description: e.message }),
        })
      }
    >
      <RefreshCwIcon className={cn(probe.isPending && "animate-spin")} />
      Check again
    </Button>
  );
}

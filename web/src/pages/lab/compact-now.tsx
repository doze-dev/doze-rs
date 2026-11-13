// Compact now: run the cleaner's real round on one partition at once, with the planner's verdict first. The Log tab of a topic
// (slice R1a) mounts `CompactNowDialog`; the Lab's rehearsal dialog uses `CompactNowBody`.
import { useQueryClient } from "@tanstack/react-query";
import { BrushCleaningIcon, CircleCheckIcon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { ApiFailure } from "@/api/client";
import { post, useLabCompact } from "@/api/r4";
import type { LabCompactOutcome } from "@/api/types/LabCompactOutcome";
import { Spans } from "@/components/app/spans";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { number } from "@/lib/format";

/** The verdict and the Run button for one partition; `onDone` after a round ran. */
export function CompactNowBody({ topic, partition, onDone, footer = true }: { topic: string; partition: number; onDone?: () => void; footer?: boolean }) {
  const qc = useQueryClient();
  const q = useLabCompact(topic, partition, true);
  const v = q.data;
  const [refused, setRefused] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    setRefused(null);
    try {
      const out = await post<LabCompactOutcome>("/api/v1/lab/compact", { topic, partition });
      if (out.outcome === "done") {
        toast.success(`Cleaned ${topic}-${partition}`, { description: "The cleaner's round ran now." });
        void qc.invalidateQueries();
        onDone?.();
      } else setRefused(out.form.refused ?? out.form.blocked ?? "The round did not run.");
    } catch (e) {
      setRefused(e instanceof ApiFailure ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  if (q.error) {
    return (
      <p role="alert" className="text-[13.5px] text-bad">
        {q.error.message}
      </p>
    );
  }
  if (!v) return <Skeleton className="h-40 w-full" />;
  const blocked = v.off ? "The Lab is off on this broker (--lab=false), so the cleaner cannot be run from the console." : v.blocked;
  return (
    <div className="flex min-w-0 flex-col gap-4" data-testid="compact-body">
      {blocked ? (
        <p role="status" className="rounded-lg border bg-well p-3 text-[13.5px]" data-testid="compact-blocked">
          {blocked}
        </p>
      ) : (
        <>
          <p className="flex items-start gap-2 rounded-lg border border-good/30 bg-good/5 p-3 text-[13.5px]" data-testid="compact-due">
            <CircleCheckIcon className="mt-0.5 size-4 shrink-0 text-good" aria-hidden />
            <span>
              The planner finds a round due: <b className="font-semibold">{v.due}</b>.
            </span>
          </p>
          {v.rewrite.length > 0 && (
            <p className="text-[13.5px] text-muted-foreground">
              <Spans spans={v.rewrite} />
            </p>
          )}
          <p className="text-[13px] text-muted-foreground">
            The checkpoint moves from offset <code className="font-mono text-foreground">{number(v.checkpoint[0])}</code> to <code className="font-mono text-foreground">{number(v.checkpoint[1])}</code>.
          </p>
        </>
      )}
      {refused && (
        <p role="alert" className="text-[13.5px] text-bad" data-testid="compact-refused">
          {refused}
        </p>
      )}
      {footer && (
        <DialogFooter>
          <DialogClose asChild>
            <Button type="button" variant="outline">
              Cancel
            </Button>
          </DialogClose>
          <Button disabled={busy || !!blocked} onClick={() => void run()} data-testid="compact-run">
            <BrushCleaningIcon /> {busy ? "Cleaning…" : "Run the cleaner now"}
          </Button>
        </DialogFooter>
      )}
      {!footer && (
        <div className="flex justify-end border-t pt-4">
          <Button disabled={busy || !!blocked} onClick={() => void run()} data-testid="compact-run">
            <BrushCleaningIcon /> {busy ? "Cleaning…" : "Run the cleaner now"}
          </Button>
        </div>
      )}
    </div>
  );
}

/** The Compact now dialog of a partition, for the Log tab: open it with `open`, close with `onOpenChange(false)`. */
export function CompactNowDialog({ topic, partition, open, onOpenChange }: { topic: string; partition: number; open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="top-6 max-h-[calc(100vh-3rem)] translate-y-0 overflow-y-auto sm:max-w-[36rem]" data-testid="compact-dialog">
        <DialogHeader>
          <DialogTitle className="text-[20px] font-medium">
            Compact <span className="font-mono">{topic}-{partition}</span> now
          </DialogTitle>
          <DialogDescription>Runs the cleaner's own round on this partition this minute instead of at its next look. It keeps the newest record of each key.</DialogDescription>
        </DialogHeader>
        {open && <CompactNowBody topic={topic} partition={partition} onDone={() => onOpenChange(false)} />}
      </DialogContent>
    </Dialog>
  );
}

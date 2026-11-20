import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { bytes, number } from "@doze/ui/lib/format";
import { useReset } from "../../api/spaces";
import type { ResetDone } from "../../api/types/ResetDone";

/** The word typed to confirm, as `doze-aws data reset` asks. */
export const RESET_WORD = "reset";

/** Reset local data: a dry run first (what each service would lose), then the word typed, then the reset the data engine does
 * for `doze-aws data reset`. */
export function ResetDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const look = useReset();
  const reset = useReset();
  const [typed, setTyped] = useState("");
  const [plan, setPlan] = useState<ResetDone | null>(null);
  const { mutate: lookMutate } = look;
  useEffect(() => {
    lookMutate(true, { onSuccess: setPlan });
  }, [lookMutate]);
  const ready = typed.trim().toLowerCase() === RESET_WORD && plan !== null;
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!ready || reset.isPending) return;
    reset.mutate(false, {
      onSuccess: (done) => {
        onOpenChange(false);
        toast.success(`Local data reset; the disk got back ${bytes(done.services.reduce((n, s) => n + s.freed, 0))}`);
      },
    });
  };
  const failed = reset.error ?? look.error;
  const words = failed ? errorWords(failed) : null;
  const losing = plan?.services.filter((s) => s.items > 0) ?? [];
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[600px]" data-testid="reset-dialog">
        <form onSubmit={submit} className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Reset local data</DialogTitle>
            <DialogDescription className="text-[14px] leading-[22px] text-foreground">Every service&apos;s resources, settings and data are removed, as if doze-aws had never run. The running process keeps going.</DialogDescription>
          </DialogHeader>
          <div className="border-l-2 border-input pl-3.5 text-[13px] leading-5" data-testid="reset-plan">
            {!plan ? (
              "Looking at what would go…"
            ) : losing.length === 0 ? (
              "Nothing is kept yet: there is nothing to remove."
            ) : (
              <ul className="grid gap-0.5">
                {losing.map((s) => (
                  <li key={s.service}>
                    <span className="font-mono">{s.service}</span>: {number(s.items)} items, {bytes(s.bytes)}
                  </li>
                ))}
              </ul>
            )}
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor="confirm-reset">
              Type <span className="font-mono">{RESET_WORD}</span> to remove it all
            </Label>
            <Input id="confirm-reset" value={typed} onChange={(e) => setTyped(e.target.value)} autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="confirm-reset" />
          </div>
          {words && (
            <Alert tone="bad" title="The reset failed" testId="reset-error">
              {words.message}
            </Alert>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" variant="destructive" disabled={!ready || reset.isPending} data-testid="reset-submit">
              {reset.isPending ? "Working…" : "Reset"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

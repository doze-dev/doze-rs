import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";

/** A confirm that asks nothing to be typed: the board's `Delete log group(s)` and `Delete clickstream?`, where the words say what
 * goes and the button is the confirmation. `error` is the service's refusal, said in place under the words and named by `call`. */
export function PlainConfirm({ open, onOpenChange, title, description, children, verb = "Delete", call, pending = false, disabled = false, error, onConfirm, testId = "confirm-dialog" }: { /** The confirmation is off (the delete would be refused): the words say why. */ disabled?: boolean; open: boolean; onOpenChange: (open: boolean) => void; title: React.ReactNode; description?: React.ReactNode; children?: React.ReactNode; verb?: string; /** The operation that was refused, for the refusal's title (`DeleteLogGroup`). */ call?: string; pending?: boolean; error?: unknown; onConfirm: () => void; testId?: string }) {
  const failed = error ? errorWords(error) : null;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[560px]" data-testid={testId}>
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">{title}</DialogTitle>
          {description && <DialogDescription className="text-[14px] leading-[22px] text-foreground">{description}</DialogDescription>}
        </DialogHeader>
        {children}
        {failed && (
          <Alert tone="bad" title={`${call ? `${call} was refused` : "Refused"}${failed.code ? `: ${failed.code}` : ""}`} testId={`${testId}-error`}>
            {failed.message}
          </Alert>
        )}
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (!pending) onConfirm();
          }}
        >
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" variant="destructive" disabled={pending || disabled} data-testid={`${testId}-submit`}>
              {pending ? "Working…" : verb}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { CallPanel } from "./call-panel";
import type { PreviewRequest } from "./contract";

/** A small edit of one part of a resource (a bucket's CORS, a stream's retention): a dialog with the fields, the call they make
 * shown live beside them, and the service's refusal in place when it says no. It is an overlay, not a page: the resource stays
 * behind it, and the page shows the change when it closes. Controlled. */
export function SectionEdit({ open, onOpenChange, title, description, request, onSave, saveLabel = "Save changes", pending = false, error, disabled = false, disabledWhy, children, wide = false, callBelow = false, testId = "section-edit" }: { open: boolean; onOpenChange: (open: boolean) => void; title: string; description?: React.ReactNode; request: PreviewRequest | null; onSave: () => void; saveLabel?: string; pending?: boolean; /** The service's refusal of the save. */ error?: unknown; disabled?: boolean; disabledWhy?: React.ReactNode; children: React.ReactNode; /** The fields need room (a JSON document). */ wide?: boolean; /** The call under the fields, full width, in place of beside them (the boards of a long call draw it so). */ callBelow?: boolean; testId?: string }) {
  const failed = error ? errorWords(error) : null;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className={wide ? "max-h-[92vh] overflow-y-auto sm:max-w-[1040px]" : "max-h-[92vh] overflow-y-auto sm:max-w-[860px]"} data-testid={testId}>
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">{title}</DialogTitle>
          {description && <DialogDescription className="text-[14px] leading-[22px] text-muted-foreground">{description}</DialogDescription>}
        </DialogHeader>
        <form
          className="grid gap-5"
          noValidate
          onSubmit={(e) => {
            e.preventDefault();
            if (!disabled && !pending) onSave();
          }}
        >
          <div className={callBelow ? "grid gap-5" : "grid items-start gap-5 md:grid-cols-[minmax(0,1fr)_minmax(0,22rem)]"}>
            <div className="grid min-w-0 gap-4">{children}</div>
            <CallPanel request={request} refusal={failed ? { code: failed.code?.split(" ")[0] ?? "Error", message: failed.message } : null} className="bg-well" testId={`${testId}-call`} />
          </div>
          {failed && (
            <Alert tone="bad" title={`Not saved${failed.code ? `: ${failed.code}` : ""}`} testId={`${testId}-refused`}>
              {failed.message}
            </Alert>
          )}
          <DialogFooter className="items-center">
            {disabled && disabledWhy && <p className="mr-auto text-[13px] text-muted-foreground">{disabledWhy}</p>}
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={disabled || pending} data-testid={`${testId}-save`}>
              {pending ? "Saving…" : saveLabel}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

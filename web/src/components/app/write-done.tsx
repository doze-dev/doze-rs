// One pattern for every write made in a dialog (the audit found some closing with a toast and others showing the result): the
// dialog stays open and shows what the broker did, in place, with one way out, Done. The page behind already shows the
// change. Only a write that removes the page's own subject (Delete topic, Delete group) leaves for the list, with a toast.
import { CircleCheckIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DialogClose, DialogFooter } from "@/components/ui/dialog";

/** What the broker did: a calm green box. */
export function Done({ children, testId = "done" }: { children: React.ReactNode; testId?: string }) {
  return (
    <div role="status" className="flex gap-3 rounded-xl border border-good/30 bg-good/5 p-4 text-[13.5px] leading-[21px]" data-testid={testId}>
      <CircleCheckIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-good" />
      <div className="min-w-0 break-words">{children}</div>
    </div>
  );
}

/** The footer of a write that is done: Done closes the dialog (by Radix, so the focus goes back to the opener). */
export function DoneFooter({ onDone }: { onDone?: () => void }) {
  return (
    <DialogFooter className="mt-1">
      {onDone ? (
        <Button onClick={onDone} data-testid="write-done" autoFocus>
          Done
        </Button>
      ) : (
        <DialogClose asChild>
          <Button data-testid="write-done" autoFocus>
            Done
          </Button>
        </DialogClose>
      )}
    </DialogFooter>
  );
}

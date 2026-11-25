import { useId, useState } from "react";
import { Button } from "../ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { confirmed } from "./typed-confirm-state";

/** A dialog that asks for the resource's name before a change that cannot be undone. The button stays off until the name is
 * typed; the page's own words (what will be detached, what stays) go in `children`, between the description and the field.
 * Controlled; `error` shows a refusal in place (`refusal`), `pending` keeps a second click from making a second call. */
export function TypedConfirm({ open, onOpenChange, title, description, words, noun = "name", prompt, verb, children, refusal, pending = false, disabled = false, onConfirm, testId = "typed-confirm" }: { open: boolean; onOpenChange: (open: boolean) => void; title: string; description?: React.ReactNode; /** What must be typed: the resource's name, or `delete`. */ words: string; /** `role name`: the field says `Enter the role name to confirm`. */ noun?: string; /** The label in full, when the board asks otherwise (`Type delete to confirm`). */ prompt?: React.ReactNode; verb: string; children?: React.ReactNode; refusal?: React.ReactNode; pending?: boolean; /** The change is refused whatever is typed (the page says why in `children`): the button stays off. */ disabled?: boolean; onConfirm: () => void; testId?: string }) {
  const [typed, setTyped] = useState("");
  const id = useId();
  const ok = confirmed(typed, words);
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) setTyped("");
        onOpenChange(o);
      }}
    >
      <DialogContent className="sm:max-w-[560px]" data-testid={testId}>
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">{title}</DialogTitle>
          {description && <DialogDescription className="text-[14px] leading-[22px] text-foreground">{description}</DialogDescription>}
        </DialogHeader>
        {children}
        {refusal}
        <form
          className="grid gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (ok && !pending) onConfirm();
          }}
        >
          <div className="grid gap-1.5">
            <Label htmlFor={id}>{prompt ?? <>Enter the {noun} to confirm</>}</Label>
            <Input id={id} value={typed} onChange={(e) => setTyped(e.target.value)} placeholder={words} autoComplete="off" spellCheck={false} className="font-mono" data-testid={`${testId}-input`} />
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" variant="destructive" disabled={!ok || pending || disabled} data-testid={`${testId}-submit`}>
              {pending ? "Working…" : verb}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

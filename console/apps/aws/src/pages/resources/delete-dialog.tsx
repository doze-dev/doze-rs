import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { toast } from "sonner";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { useDelete } from "../../api/resources";
import type { DeleteSpec } from "../../api/types/DeleteSpec";

/** The word typed to confirm, as the design's delete dialogs ask. */
export const CONFIRM_WORD = "delete";

/** Delete (or schedule the deletion of) one resource: a dialog over its page naming it, saying what happens, and asking for the
 * word typed. Done, the kind's list opens; refused, the service's words are shown and nothing changed. */
export function DeleteDialog({ open, onOpenChange, service, kind, one, name, label, spec }: { open: boolean; onOpenChange: (o: boolean) => void; service: string; kind: string; one: string; name: string; label: string; spec: DeleteSpec }) {
  const [typed, setTyped] = useState("");
  const del = useDelete(service, kind);
  const navigate = useNavigate();
  const ready = typed.trim().toLowerCase() === CONFIRM_WORD;
  const change = (o: boolean) => {
    if (!o) {
      setTyped("");
      del.reset();
    }
    onOpenChange(o);
  };
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!ready || del.isPending) return;
    del.mutate(name, {
      onSuccess: () => {
        change(false);
        toast.success(`${label} ${spec.verb === "Delete" ? "deleted" : "scheduled for deletion"}`);
        void navigate({ to: "/resources/$service/$second", params: { service, second: kind }, search: {} });
      },
    });
  };
  const failed = del.error ? errorWords(del.error) : null;
  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent className="sm:max-w-[600px]" data-testid="delete-dialog">
        <form onSubmit={submit} className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">
              {spec.verb} {one}
            </DialogTitle>
            <DialogDescription className="text-[14px] leading-[22px] text-foreground">
              {spec.verb} <b className="font-mono font-semibold break-all text-bright">{label}</b>?
            </DialogDescription>
          </DialogHeader>
          <p className="border-l-2 border-input pl-3.5 text-[13px] leading-5 text-foreground">
            {spec.words} <span className="text-muted-foreground">Called as <span className="font-mono">{spec.operation}</span>; Live shows it.</span>
          </p>
          <div className="grid gap-1.5">
            <Label htmlFor="confirm-delete">
              Type <span className="font-mono">{CONFIRM_WORD}</span> to confirm
            </Label>
            <Input id="confirm-delete" value={typed} onChange={(e) => setTyped(e.target.value)} autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="confirm-delete" />
          </div>
          {failed && (
            <Alert tone="bad" title={failed.code ? `${spec.operation} was refused: ${failed.code}` : `${spec.operation} failed`} testId="delete-error">
              {failed.message}
            </Alert>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => change(false)}>
              Cancel
            </Button>
            <Button type="submit" variant="destructive" disabled={!ready || del.isPending} data-testid="delete-submit">
              {del.isPending ? "Working…" : spec.verb}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

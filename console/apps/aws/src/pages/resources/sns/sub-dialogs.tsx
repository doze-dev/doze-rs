// Delete subscription (sns__subscription-delete): the board asks for nothing typed; it says what stops and what is not touched.
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { endpointName } from "./sub-model";

export function DeleteSubscription({ open, onOpenChange, protocol, endpoint, topic, pending, error, onConfirm }: { open: boolean; onOpenChange: (o: boolean) => void; protocol: string; endpoint: string; topic: string; pending: boolean; error: unknown; onConfirm: () => void }) {
  const failed = error ? errorWords(error) : null;
  const kind = protocol === "sqs" ? "queue" : protocol === "lambda" ? "function" : "endpoint";
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[560px]" data-testid="delete-dialog">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">Delete subscription</DialogTitle>
          <DialogDescription className="text-[14px] leading-[22px] text-foreground">
            The {kind} <b className="font-mono font-semibold text-bright">{endpointName(protocol, endpoint)}</b> stops receiving what is published to the topic <b className="font-mono font-semibold text-bright">{topic}</b>. {protocol === "sqs" || protocol === "lambda" ? `The ${kind} itself is not touched.` : ""}
          </DialogDescription>
        </DialogHeader>
        <p className="text-[14px] leading-[22px] text-foreground">After the subscription is deleted, it cannot be recovered. Published notifications to the topic will not be delivered to that deleted subscription; you have to re-create the subscription to receive notifications.</p>
        {failed && <Alert tone="bad" title={`Unsubscribe was refused${failed.code ? `: ${failed.code}` : ""}`} testId="delete-error">{failed.message}</Alert>}
        <DialogFooter>
          <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button type="button" variant="destructive" disabled={pending} onClick={onConfirm} data-testid="delete-dialog-submit">{pending ? "Working…" : "Delete"}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

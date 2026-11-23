// Confirm subscription (sns__subscription-confirm): the SubscribeURL of the confirmation message, pasted, confirms the subscription on the
// endpoint's behalf. The token is the URL's; ConfirmSubscription takes it with the topic.
import { useState } from "react";
import { Callout } from "@doze/ui/components/app/callout";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { useKindOp } from "../../../kit";
import { type SubRow } from "./sub-model";
import { confirmationOf } from "./sub-form-model";
import { delivers } from "./protocols";

export function ConfirmSubscription({ sub, topic, onClose, onDone }: { sub: SubRow | null; topic: string; onClose: () => void; onDone: (arn: string | null) => void }) {
  const [link, setLink] = useState("");
  const confirm = useKindOp<{ SubscriptionArn?: string }>({ service: "sns", kind: "subscriptions" }, "ConfirmSubscription");
  const read = confirmationOf(link);
  const failed = confirm.error ? errorWords(confirm.error) : null;
  const wrong = link.trim() !== "" && !read ? "That is not a SubscribeURL: it carries no Token." : null;
  const mail = sub?.protocol === "email" || sub?.protocol === "email-json";
  const close = (o: boolean) => {
    if (!o) {
      setLink("");
      confirm.reset();
      onClose();
    }
  };
  return (
    <Dialog open={sub !== null} onOpenChange={close}>
      <DialogContent className="sm:max-w-[600px]" data-testid="confirm-dialog">
        <form
          className="grid gap-5"
          onSubmit={(e) => {
            e.preventDefault();
            if (!read || confirm.isPending) return;
            confirm.mutate({ TopicArn: read.topicArn ?? sub?.topicArn, Token: read.token }, { onSuccess: (r) => { setLink(""); onDone(r.SubscriptionArn ?? null); } });
          }}
        >
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Confirm subscription</DialogTitle>
            <DialogDescription className="text-[14px] leading-[22px] text-foreground">
              {sub && (mail ? <>The address <b className="font-semibold text-bright">{sub.endpoint}</b> was subscribed to <b className="font-semibold text-bright">{topic}</b>. doze-aws sends no email, so no one has a link to paste: this subscription stays pending.</> : <>The {delivers(sub.protocol) ? "webhook" : "endpoint"} <b className="font-semibold text-bright">{sub.endpoint}</b> was subscribed to <b className="font-semibold text-bright">{topic}</b>. SNS posted it a confirmation message that carries a <b className="font-semibold text-bright">SubscribeURL</b>. Paste the subscription confirmation link to confirm on the endpoint's behalf.</>)}
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-1.5">
            <Label htmlFor="confirm-link">Subscription confirmation link</Label>
            <Input id="confirm-link" value={link} onChange={(e) => setLink(e.target.value)} placeholder="Paste the SubscribeURL from the confirmation message" autoComplete="off" spellCheck={false} aria-invalid={wrong !== null} className="font-mono text-[13px]" data-testid="confirm-link" />
            {wrong && <p className="text-[13px] text-bad" role="alert" data-testid="confirm-wrong">{wrong}</p>}
          </div>
          <Callout tone="note" lead="The SubscribeURL carries the confirmation token," testId="note">which is valid for two days. Normally the endpoint confirms itself, by sending an HTTP GET request to the SubscribeURL or by calling ConfirmSubscription.</Callout>
          {failed && <Alert tone="bad" title={`ConfirmSubscription was refused${failed.code ? `: ${failed.code}` : ""}`} testId="confirm-error">{failed.message}</Alert>}
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => close(false)}>Cancel</Button>
            <Button type="submit" disabled={!read || confirm.isPending} data-testid="confirm-submit">{confirm.isPending ? "Working…" : "Confirm subscription"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

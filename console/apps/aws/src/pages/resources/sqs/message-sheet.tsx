// A message opened from a poll (sqs__message-detail): its body, its attributes, and the ReceiveMessage answer it came from, in a
// dialog that only closes (deleting is done from the table, where the message is chosen).
import { CodeBlock, JsonBlock, pretty } from "@doze/ui/components/app/code-block";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { type Message, sizeOf } from "./messages-model";

/** The ReceiveMessage answer for one message, as the service gave it. */
export const answerOf = (m: Message): string => JSON.stringify({ MessageId: m.MessageId, ReceiptHandle: m.ReceiptHandle, ...(m.MD5OfBody ? { MD5OfBody: m.MD5OfBody } : {}), Attributes: m.Attributes ?? {} }, null, 2);

const label = "text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase";

export function MessageSheet({ m, onClose }: { m: Message | null; onClose: () => void }) {
  return (
    <Dialog open={m !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90vh] gap-0 overflow-y-auto sm:max-w-[640px]" data-testid="message-sheet">
        {m && (
          <>
            <DialogHeader className="pb-5">
              <DialogTitle className="text-[21px] font-semibold">Message details</DialogTitle>
              <DialogDescription className="font-mono text-[12.5px] break-all">{m.MessageId}</DialogDescription>
            </DialogHeader>
            <div className="grid gap-6">
              <section className="grid gap-2">
                <div className="flex items-center justify-between">
                  <h3 className={label}>Body · {sizeOf(m)}</h3>
                  <CopyButton text={m.Body} what="Body copied" className="bg-card" />
                </div>
                <JsonBlock value={pretty(m.Body)} testId="message-body" />
              </section>
              {m.MessageAttributes && Object.keys(m.MessageAttributes).length > 0 && (
                <section className="grid gap-2" data-testid="message-attributes">
                  <h3 className={label}>Message attributes</h3>
                  <dl className="grid grid-cols-[minmax(8rem,12rem)_minmax(0,1fr)] text-[13.5px]">
                    {Object.entries(m.MessageAttributes).map(([k, v]) => (
                      <div key={k} className="contents">
                        <dt className="py-1.5 text-muted-foreground">{k}</dt>
                        <dd className="py-1.5 break-all text-bright">{v.DataType.split(".")[0]}, {v.StringValue ?? v.BinaryValue}</dd>
                      </div>
                    ))}
                  </dl>
                </section>
              )}
              <section className="grid gap-2">
                <h3 className={label}>Returned by ReceiveMessage</h3>
                <CodeBlock text={answerOf(m)} wrap={false} max="max-h-72" testId="message-answer" />
              </section>
              <div className="flex justify-end border-t pt-5">
                <Button onClick={onClose} data-testid="message-close">Close</Button>
              </div>
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

// A message opened from a poll (sqs__message-detail): its body, its attributes, and the ReceiveMessage answer it came from.
import { Trash2Icon } from "lucide-react";
import { CodeBlock, JsonBlock, pretty } from "@doze/ui/components/app/code-block";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Button } from "@doze/ui/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@doze/ui/components/ui/sheet";
import { type Message, sizeOf } from "./messages-model";

/** The ReceiveMessage answer for one message, as the service gave it. */
export const answerOf = (m: Message): string => JSON.stringify({ MessageId: m.MessageId, ReceiptHandle: m.ReceiptHandle, ...(m.MD5OfBody ? { MD5OfBody: m.MD5OfBody } : {}), Attributes: m.Attributes ?? {} }, null, 2);

export function MessageSheet({ m, onClose, onDelete }: { m: Message | null; onClose: () => void; onDelete: (m: Message) => void }) {
  return (
    <Sheet open={m !== null} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-[600px]" data-testid="message-sheet">
        {m && (
          <>
            <SheetHeader className="border-b px-7 pt-7 pb-5">
              <SheetTitle className="text-[21px] font-semibold">Message details</SheetTitle>
              <SheetDescription className="font-mono text-[12.5px] break-all">{m.MessageId}</SheetDescription>
            </SheetHeader>
            <div className="grid gap-6 px-7 py-6">
              <section className="grid gap-2">
                <div className="flex items-center justify-between">
                  <h3 className="text-[13.5px] text-muted-foreground">Body · {sizeOf(m)}</h3>
                  <CopyButton text={m.Body} what="Body copied" className="bg-card" />
                </div>
                <JsonBlock value={pretty(m.Body)} testId="message-body" />
              </section>
              {m.MessageAttributes && Object.keys(m.MessageAttributes).length > 0 && (
                <section className="grid gap-2" data-testid="message-attributes">
                  <h3 className="text-[13.5px] text-muted-foreground">Message attributes</h3>
                  <dl className="grid grid-cols-[minmax(8rem,12rem)_minmax(0,1fr)] text-[13.5px]">
                    {Object.entries(m.MessageAttributes).map(([k, v]) => (
                      <div key={k} className="contents">
                        <dt className="border-b py-2 font-mono text-muted-foreground">{k}</dt>
                        <dd className="border-b py-2 font-mono break-all text-bright">{v.DataType.split(".")[0]}, {v.StringValue ?? v.BinaryValue}</dd>
                      </div>
                    ))}
                  </dl>
                </section>
              )}
              <section className="grid gap-2">
                <h3 className="text-[13.5px] text-muted-foreground">Returned by ReceiveMessage</h3>
                <CodeBlock text={answerOf(m)} wrap={false} max="max-h-72" testId="message-answer" />
              </section>
              <div className="flex justify-between border-t pt-5">
                <Button variant="outline" className="bg-card" onClick={onClose}>Close</Button>
                <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => onDelete(m)} data-testid="message-delete">
                  <Trash2Icon /> Delete
                </Button>
              </div>
            </div>
          </>
        )}
      </SheetContent>
    </Sheet>
  );
}
